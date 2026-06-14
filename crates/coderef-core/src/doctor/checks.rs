//! Individual doctor check implementations.

use std::collections::HashSet;

use fancy_regex::Regex;

use super::Diagnostic;
use crate::config::{Config, Pattern};
use crate::severity::Severity;
use crate::variables::{parse_segments, Segment};

/// Look up the effective severity for `check_id`. Resolution order:
///
/// 1. `Pattern.severity[check_id]` — per-pattern override (most
///    specific).
/// 2. `Config.severity[check_id]` — workspace-level override.
/// 3. `default` — the check's hardcoded fallback.
///
/// A `Severity::Off` at either override layer suppresses emission.
pub(super) fn resolve_severity(
    cfg: &Config,
    p: &Pattern,
    check_id: &str,
    default: Severity,
) -> Severity {
    if let Some(s) = p.severity.get(check_id).copied() {
        return s;
    }
    if let Some(s) = cfg.severity.get(check_id).copied() {
        return s;
    }
    default
}

/// Push a diagnostic, respecting the per-pattern + workspace
/// `severity` overrides. A check resolved to `Severity::Off` skips
/// emission entirely; everything else uses the resolved severity.
#[allow(clippy::too_many_arguments)] // each arg is a distinct, small value; bundling them
                                     // behind a struct would add boilerplate without clarity.
fn push_diag(
    out: &mut Vec<Diagnostic>,
    cfg: &Config,
    p: &Pattern,
    pattern_id: &str,
    check_id: &'static str,
    default_severity: Severity,
    message: String,
    hint: Option<String>,
) {
    let sev = resolve_severity(cfg, p, check_id, default_severity);
    if sev == Severity::Off {
        return;
    }
    out.push(Diagnostic {
        check: check_id.into(),
        severity: sev,
        pattern_id: Some(pattern_id.into()),
        message,
        hint,
    });
}

/// Default ceiling on patterns sharing `category: "other"` before
/// doctor emits a `category.tooBroadOther` info. DESIGN.md §5.7.4
/// defers per-workspace override to the v0.3 visual editor's
/// `integrity.maxOtherPatterns` setting; v0.2 hard-codes the default.
const MAX_OTHER_PATTERNS_DEFAULT: usize = 5;

/// `category.tooBroadOther` — too many patterns are bucketed in
/// `other`, defeating category-based grouping. Counts both
/// explicitly-declared `"other"` and patterns that *infer* to `other`
/// (i.e. `kind: "url"` without a declared `category`); the latter
/// already get a `category.unset` info so this check is the aggregate
/// signal.
pub(super) fn check_too_broad_other(cfg: &Config, out: &mut Vec<Diagnostic>) {
    let mut others: Vec<&str> = Vec::new();
    for (id, p) in &cfg.patterns {
        let resolved = p
            .category
            .as_deref()
            .unwrap_or_else(|| crate::category::infer_category(p.kind));
        if resolved == "other" {
            others.push(id.as_str());
        }
    }
    if others.len() > MAX_OTHER_PATTERNS_DEFAULT {
        // No per-pattern owner; attach the diagnostic without a
        // pattern_id. Severity goes through the workspace-level
        // override (no per-pattern override applies here).
        let sev = cfg
            .severity
            .get("category.tooBroadOther")
            .copied()
            .unwrap_or(Severity::Info);
        if sev == Severity::Off {
            return;
        }
        out.push(Diagnostic {
            check: "category.tooBroadOther".into(),
            severity: sev,
            pattern_id: None,
            message: format!(
                "{count} patterns share category `other` (default ceiling {ceiling}); the \
                 references browser loses semantic grouping when too many fall here",
                count = others.len(),
                ceiling = MAX_OTHER_PATTERNS_DEFAULT,
            ),
            hint: Some(format!(
                "declare a `category` on each: {}",
                others.join(", ")
            )),
        });
    }
}

/// `commitMessage.allDisabled` — every pattern resolves to
/// `EffectiveScope::Skip` for commit-message linting, which means
/// `coderef commit-msg` would be a no-op on this config. Usually a
/// signal that someone disabled the wrong knob; surface as info so
/// the next consumer notices. DESIGN.md §16.1.1.
pub(super) fn check_commit_message_all_disabled(cfg: &Config, out: &mut Vec<Diagnostic>) {
    if cfg.patterns.is_empty() {
        return;
    }
    let all_disabled = cfg.patterns.values().all(|p| {
        crate::config::resolve_commit_message_scope(p)
            == crate::config::EffectiveCommitMessageScope::Skip
    });
    if !all_disabled {
        return;
    }
    let sev = cfg
        .severity
        .get("commitMessage.allDisabled")
        .copied()
        .unwrap_or(Severity::Info);
    if sev == Severity::Off {
        return;
    }
    out.push(Diagnostic {
        check: "commitMessage.allDisabled".into(),
        severity: sev,
        pattern_id: None,
        message: "every pattern resolves to `scope.commitMessage: false` (kind default or \
                  explicit); `coderef commit-msg` would be a no-op on this config"
            .into(),
        hint: Some(
            "set `scope.commitMessage: true` (or `\"required\"`) on at least one pattern, \
             or rely on the kind-based default by leaving `commitMessage` unset on a \
             url/local pattern"
                .into(),
        ),
    });
}

/// `commitMessage.ifchangeMisconfigured` — a `kind: "ifchange"`
/// pattern explicitly opts itself into commit-message scanning. The
/// IfChange/ThenChange semantics only make sense over a multi-file
/// diff, not a single commit message; the marker would parse but
/// never produce a meaningful block. DESIGN.md §16.1.1.
pub(super) fn check_commit_message_ifchange_misconfigured(cfg: &Config, out: &mut Vec<Diagnostic>) {
    for (id, p) in &cfg.patterns {
        if p.kind != crate::config::PatternKind::IfChange {
            continue;
        }
        // Only fire when the pattern *explicitly* sets commitMessage to
        // true or "required" — the kind-based default for ifchange is
        // Skip, which is the correct behaviour.
        let declared = p.scope.as_ref().and_then(|s| s.commit_message);
        let opted_in = matches!(
            declared,
            Some(
                crate::config::CommitMessageScope::Bool(true)
                    | crate::config::CommitMessageScope::Tag(
                        crate::config::CommitMessageTag::Required
                    )
            )
        );
        if !opted_in {
            continue;
        }
        push_diag(
            out,
            cfg,
            p,
            id,
            "commitMessage.ifchangeMisconfigured",
            Severity::Warning,
            format!(
                "pattern `{id}` has `kind: \"ifchange\"` and explicitly sets \
                 `scope.commitMessage` to opt in; IfChange/ThenChange blocks only verify \
                 against a multi-file diff, not a single commit message"
            ),
            Some(
                "remove `scope.commitMessage` from this pattern (the kind-based default \
                 already skips it) — or change the pattern's `kind` if a single-message \
                 reference is what you actually want"
                    .into(),
            ),
        );
    }
}

/// Scan-dependent thresholds. DESIGN §5.7.4 / §6.3.4 / §10.7
/// document these as user-configurable in v0.3 via
/// `integrity.<check>`; v0.2 hard-codes the defaults.
const CATEGORY_MISMATCH_RATIO_THRESHOLD: f64 = 0.8;
const CATEGORY_MISMATCH_MIN_SAMPLES: usize = 3;
const COMPOSABLE_TYPO_LEVENSHTEIN: u32 = 1;

/// `category.mismatch` (DESIGN §5.7.4) — a pattern's matched
/// references consistently look like one category while the pattern
/// declares another. Heuristic: if ≥ 80% of `matched_text` starts
/// with a category sigil (`@` → people, `/` → files, `http` →
/// urls) and that contradicts the declared/inferred category, warn.
///
/// Requires `CATEGORY_MISMATCH_MIN_SAMPLES` (default 3) refs per
/// pattern to fire — small samples skew the ratio.
pub(super) fn check_category_mismatch(
    cfg: &Config,
    refs: &[crate::reference::Reference],
    out: &mut Vec<Diagnostic>,
) {
    use std::collections::HashMap;
    let mut by_pattern: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in refs {
        by_pattern
            .entry(r.pattern_id.as_str())
            .or_default()
            .push(r.matched_text.as_str());
    }
    for (id, samples) in by_pattern {
        if samples.len() < CATEGORY_MISMATCH_MIN_SAMPLES {
            continue;
        }
        let Some(pattern) = cfg.patterns.get(id) else {
            continue;
        };
        let resolved = pattern
            .category
            .as_deref()
            .unwrap_or_else(|| crate::category::infer_category(pattern.kind));
        // Tally sigils.
        let n = samples.len();
        let mut at = 0usize;
        let mut slash = 0usize;
        let mut http = 0usize;
        for s in &samples {
            let s = s.trim_start();
            // Strip a leading wrapper like `TODO(` so the *captured*
            // sigil is what we count (e.g. `TODO(@user)` → `@user`).
            let inside = s.split_once('(').map_or(s, |(_, rest)| rest);
            if inside.starts_with('@') {
                at += 1;
            } else if inside.starts_with('/') {
                slash += 1;
            } else if inside.starts_with("http://") || inside.starts_with("https://") {
                http += 1;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let n_f = n as f64;
        let ratio = |k: usize| -> f64 {
            #[allow(clippy::cast_precision_loss)]
            let k_f = k as f64;
            k_f / n_f
        };
        let (suspected, sample_sigil) = if ratio(at) >= CATEGORY_MISMATCH_RATIO_THRESHOLD {
            ("people", "@")
        } else if ratio(slash) >= CATEGORY_MISMATCH_RATIO_THRESHOLD {
            ("files", "/")
        } else if ratio(http) >= CATEGORY_MISMATCH_RATIO_THRESHOLD {
            ("urls", "http(s)://")
        } else {
            continue;
        };
        if resolved == suspected {
            continue;
        }
        let sev = resolve_severity(cfg, pattern, "category.mismatch", Severity::Warning);
        if sev == Severity::Off {
            continue;
        }
        out.push(Diagnostic {
            check: "category.mismatch".into(),
            severity: sev,
            pattern_id: Some(id.to_string()),
            message: format!(
                "pattern `{id}` is declared as `category: \"{resolved}\"` but its matches \
                 consistently start with `{sample_sigil}` (≥ {pct:.0}% of {n} samples), which \
                 looks like `{suspected}`",
                pct = CATEGORY_MISMATCH_RATIO_THRESHOLD * 100.0,
            ),
            hint: Some(format!(
                "update `patterns.{id}.category` to `\"{suspected}\"`, or — if the matches \
                 really aren't `{suspected}` — silence this check via \
                 `patterns.{id}.severity: {{ \"category.mismatch\": \"off\" }}`"
            )),
        });
    }
}

/// `anchor.skippedExt` (DESIGN §6.3.4) — a `local` reference carries
/// a `#anchor` suffix but the target file's extension isn't one of
/// the supported Markdown extensions (`.md`, `.markdown`). The
/// reference still resolves; the anchor just isn't verified. Surface
/// as info so authors notice (e.g. they may have intended `.md` but
/// typed `.text`).
///
/// Host-only: doesn't need to reach `crate::anchor` directly but is
/// only ever called from `run_doctor_with_workspace`, which itself
/// is gated.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn check_anchor_skipped_ext(
    cfg: &Config,
    refs: &[crate::reference::Reference],
    out: &mut Vec<Diagnostic>,
) {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    for r in refs {
        if r.pattern_kind != crate::config::PatternKind::Local {
            continue;
        }
        let Some(hash_idx) = r.target.find('#') else {
            continue;
        };
        let path = &r.target[..hash_idx];
        if path.is_empty() {
            continue;
        }
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(ext.as_str(), "md" | "markdown") {
            continue;
        }
        // Dedup by (pattern_id, ext) so the diagnostic is one per
        // affected pattern+extension combo rather than per match.
        let key = format!("{}:{ext}", r.pattern_id);
        if !seen.insert(key) {
            continue;
        }
        let Some(pattern) = cfg.patterns.get(&r.pattern_id) else {
            continue;
        };
        let sev = resolve_severity(cfg, pattern, "anchor.skippedExt", Severity::Info);
        if sev == Severity::Off {
            continue;
        }
        let ext_display = if ext.is_empty() {
            "(no extension)".to_string()
        } else {
            format!(".{ext}")
        };
        out.push(Diagnostic {
            check: "anchor.skippedExt".into(),
            severity: sev,
            pattern_id: Some(r.pattern_id.clone()),
            message: format!(
                "pattern `{p}` resolves to a target with extension `{ext_display}` and a \
                 `#anchor` suffix; anchor verification is only implemented for `.md` / \
                 `.markdown` files in v0.2",
                p = r.pattern_id,
            ),
            hint: Some(
                "remove the `#anchor` portion from this reference, or convert the target \
                 file to Markdown, or silence this check via \
                 `severity: { \"anchor.skippedExt\": \"off\" }`"
                    .into(),
            ),
        });
    }
}

/// `anchor.styleMismatch` (DESIGN §6.3.4) — a `local` reference's
/// target file mixes Pandoc-style explicit `{#id}` heading
/// annotations with un-annotated headings, but the pattern's
/// configured slugifier is `github`. The author may be relying on
/// Pandoc anchors that github won't honour.
///
/// Heuristic: warn when a target file contains *both* explicit `{#id}`
/// and un-annotated headings, and the resolve config doesn't pin the
/// slugifier to `pandoc`. The check reads each unique target file
/// once.
///
/// Host-only: reads target files from disk and calls
/// `crate::anchor::extract_headings`, both unavailable on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn check_anchor_style_mismatch(
    cfg: &Config,
    refs: &[crate::reference::Reference],
    root: &std::path::Path,
    out: &mut Vec<Diagnostic>,
) {
    use std::collections::HashSet;
    let mut probed: HashSet<String> = HashSet::new();
    for r in refs {
        if r.pattern_kind != crate::config::PatternKind::Local {
            continue;
        }
        let Some(hash_idx) = r.target.find('#') else {
            continue;
        };
        let path_part = &r.target[..hash_idx];
        let ext = path_part
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "md" | "markdown") {
            continue;
        }
        let Some(pattern) = cfg.patterns.get(&r.pattern_id) else {
            continue;
        };
        let slugifier = pattern
            .resolve
            .as_ref()
            .and_then(|res| res.slugifier.as_ref())
            .and_then(serde_json::Value::as_str)
            .unwrap_or("github");
        // Only warn on github (the default); if the user picked
        // pandoc/gitlab/etc explicitly, they know what they're doing.
        if slugifier != "github" {
            continue;
        }
        let key = format!("{}:{path_part}", r.pattern_id);
        if !probed.insert(key) {
            continue;
        }
        let abs = root.join(path_part.trim_start_matches('/'));
        let Ok(content) = std::fs::read_to_string(&abs) else {
            continue;
        };
        let headings = crate::anchor::extract_headings(&content);
        if headings.is_empty() {
            continue;
        }
        let has_explicit = headings.iter().any(|h| h.explicit_id.is_some());
        let has_implicit = headings.iter().any(|h| h.explicit_id.is_none());
        if !(has_explicit && has_implicit) {
            continue;
        }
        let sev = resolve_severity(cfg, pattern, "anchor.styleMismatch", Severity::Warning);
        if sev == Severity::Off {
            continue;
        }
        out.push(Diagnostic {
            check: "anchor.styleMismatch".into(),
            severity: sev,
            pattern_id: Some(r.pattern_id.clone()),
            message: format!(
                "target file `{path_part}` mixes Pandoc-style `{{#id}}` headings with \
                 un-annotated headings; pattern `{p}` uses the `github` slugifier (the \
                 default), which honours `{{#id}}` overrides but renders un-annotated \
                 headings differently than Pandoc would",
                p = r.pattern_id,
            ),
            hint: Some(
                "set `patterns.<id>.resolve.slugifier: \"pandoc\"` to use Pandoc's algorithm \
                 consistently, or remove the `{#id}` overrides if github-style slugs are \
                 what the docs are rendered with"
                    .into(),
            ),
        });
    }
}

/// `coupled.composableTypo` (DESIGN §10.7) — an `IfChange(<id>)`
/// block has an id text that *almost* matches one of the workspace's
/// `kind: "url"` / `kind: "local"` patterns (Levenshtein within
/// `COMPOSABLE_TYPO_LEVENSHTEIN`, default 1) but fails to resolve.
/// Usually a typo on a Shape C composable id.
///
/// Host-only: consumes `crate::ifchange` blocks, which only exist on
/// non-wasm targets.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn check_coupled_composable_typo(
    cfg: &Config,
    blocks: &[crate::ifchange::IfChangeBlock],
    out: &mut Vec<Diagnostic>,
) {
    use std::collections::HashSet;
    // Collect every unique IfChange-id text from the workspace.
    // IfChange markers come from the dedicated block scanner, not the
    // reference scanner — they have their own grammar.
    let mut id_texts: HashSet<&str> = HashSet::new();
    for b in blocks {
        if let Some(ref id) = b.id {
            if !id.is_empty() {
                id_texts.insert(id.as_str());
            }
        }
    }
    for id in id_texts {
        // If it resolves, no typo.
        if crate::ifchange::resolve_composable_id(cfg, id).is_some() {
            continue;
        }
        // Otherwise, see if any url/local pattern is *almost* a match.
        let mut nearest: Option<&str> = None;
        for (pat_id, p) in &cfg.patterns {
            if !matches!(
                p.kind,
                crate::config::PatternKind::Url | crate::config::PatternKind::Local
            ) {
                continue;
            }
            // Look at the pattern's first capture group as the
            // "near-match" candidate. We approximate via a brute-force
            // suffix probe: if removing or adding one char makes the
            // id match the pattern, it's a typo.
            let Ok(re) = fancy_regex::Regex::new(&p.regex) else {
                continue;
            };
            // Try one-char-edit neighbours.
            if levenshtein_near_match(&re, id, COMPOSABLE_TYPO_LEVENSHTEIN) {
                nearest = Some(pat_id.as_str());
                break;
            }
        }
        let Some(near_id) = nearest else {
            continue;
        };
        // Top-level severity override (no specific pattern id to scope
        // against — this is a global doctor signal).
        let sev = cfg
            .severity
            .get("coupled.composableTypo")
            .copied()
            .unwrap_or(Severity::Info);
        if sev == Severity::Off {
            continue;
        }
        out.push(Diagnostic {
            check: "coupled.composableTypo".into(),
            severity: sev,
            pattern_id: None,
            message: format!(
                "IfChange id `{id}` doesn't resolve through any pattern, but a small edit \
                 would match pattern `{near_id}` — likely a typo"
            ),
            hint: Some(
                "double-check the id text; if it's intentional, declare it as a literal \
                 Shape B id (no parens) or escalate this check via \
                 `severity: { \"coupled.composableTypo\": \"off\" }`"
                    .into(),
            ),
        });
    }
}

/// Does a Levenshtein-`max`-edit neighbour of `text` match `re`?
/// v0.2 only handles `max == 1`; larger thresholds would explode the
/// neighbour-enumeration cost without much practical value.
///
/// Only used by the host-only `check_coupled_composable_typo`.
#[cfg(not(target_arch = "wasm32"))]
fn levenshtein_near_match(re: &fancy_regex::Regex, text: &str, max: u32) -> bool {
    if max != 1 {
        return false;
    }
    let chars: Vec<char> = text.chars().collect();
    // Substitution: replace each char with each ASCII letter/digit/`-`/`_`.
    let probe = "abcdefghijklmnopqrstuvwxyz0123456789-_";
    for i in 0..chars.len() {
        for sub in probe.chars() {
            if sub == chars[i] {
                continue;
            }
            let mut candidate = chars.clone();
            candidate[i] = sub;
            let s: String = candidate.iter().collect();
            if re.is_match(&s).unwrap_or(false) {
                return true;
            }
        }
    }
    // Deletion: drop each char.
    for i in 0..chars.len() {
        let mut candidate = chars.clone();
        candidate.remove(i);
        let s: String = candidate.iter().collect();
        if re.is_match(&s).unwrap_or(false) {
            return true;
        }
    }
    // Insertion: add each probe char at each position.
    for i in 0..=chars.len() {
        for ins in probe.chars() {
            let mut candidate = chars.clone();
            candidate.insert(i, ins);
            let s: String = candidate.iter().collect();
            if re.is_match(&s).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// Run every per-pattern check, appending diagnostics in place.
#[allow(clippy::too_many_lines)] // every check is small; one fn keeps the order obvious
pub fn check_pattern(id: &str, p: &Pattern, cfg: &Config, out: &mut Vec<Diagnostic>) {
    // 1) target / targets validity. Block-kind patterns don't resolve
    // to anything (the match itself is the diagnostic), so we skip
    // target validation for them — except we still flag the
    // unambiguous misuse of declaring `targets[]` on a block pattern,
    // which is rejected at compile time.
    let has_target = p.target.is_some();
    let has_targets = !p.targets.is_empty();
    if p.kind == crate::config::PatternKind::Block {
        if has_targets {
            push_diag(
                out,
                cfg,
                p,
                id,
                "pattern.blockKindCannotHaveTargets",
                Severity::Error,
                format!("pattern `{id}` has `kind: \"block\"` and may not declare `targets[]`"),
                Some(
                    "remove `targets[]` from block patterns — the matched text is the diagnostic"
                        .into(),
                ),
            );
        }
    } else {
        match (has_target, has_targets) {
            (false, false) => {
                push_diag(
                    out,
                    cfg,
                    p,
                    id,
                    "pattern.targetMissing",
                    Severity::Error,
                    format!("pattern `{id}` has neither `target` nor `targets[]`"),
                    Some("set `target: \"...\"` (single) or `targets: [...]` (multi)".into()),
                );
            }
            (true, true) => {
                push_diag(
                    out,
                    cfg,
                    p,
                    id,
                    "pattern.targetsBothFieldsSet",
                    Severity::Error,
                    format!("pattern `{id}` declares both `target` and `targets[]`"),
                    Some(
                        "pick one form: drop `target` to use multi-target, or empty `targets`"
                            .into(),
                    ),
                );
            }
            _ => {}
        }
    }

    // 2) regex compile.
    let compiled = match Regex::new(&p.regex) {
        Ok(r) => r,
        Err(e) => {
            push_diag(
                out,
                cfg,
                p,
                id,
                "pattern.regexInvalid",
                Severity::Error,
                format!("pattern `{id}` has an invalid regex: {e}"),
                None,
            );
            // Bail regardless of override: subsequent checks need a
            // compiled regex. Disabling the diagnostic via severity:
            // off doesn't change the fact we can't proceed.
            return;
        }
    };

    let regex_caps: HashSet<String> = compiled
        .capture_names()
        .flatten()
        .map(String::from)
        .collect();

    // 3) Walk every template string (target + title + per-target urls) and
    // collect references to captures / variables.
    let mut referenced_captures: HashSet<String> = HashSet::new();
    let mut templates: Vec<(&'static str, &str)> = Vec::new();
    if let Some(t) = &p.target {
        templates.push(("target", t.as_str()));
    }
    if let Some(t) = &p.title {
        templates.push(("title", t.as_str()));
    }
    for (i, t) in p.targets.iter().enumerate() {
        let _ = i;
        templates.push(("targets[].url", t.url.as_str()));
    }

    for (field, template) in templates {
        let segments = match parse_segments(template) {
            Ok(s) => s,
            Err(e) => {
                push_diag(
                    out,
                    cfg,
                    p,
                    id,
                    "variable.invalidSyntax",
                    Severity::Error,
                    format!("pattern `{id}` field `{field}` has invalid `${{...}}` syntax: {e}"),
                    None,
                );
                continue;
            }
        };

        for seg in segments {
            let Segment::Variable { namespace, name } = seg else {
                continue;
            };
            match namespace.as_deref() {
                None => {
                    // Bare name — could be a builtin or a capture.
                    if regex_caps.contains(&name) {
                        referenced_captures.insert(name);
                    }
                }
                Some("capture") => {
                    if regex_caps.contains(&name) {
                        referenced_captures.insert(name);
                    } else {
                        push_diag(
                            out,
                            cfg,
                            p,
                            id,
                            "pattern.captureUnknown",
                            Severity::Error,
                            format!(
                                "pattern `{id}` field `{field}` references `${{capture:{name}}}` \
                                 but the regex has no such named capture"
                            ),
                            Some(format!(
                                "either add `(?<{name}>...)` to the regex, or fix the template to \
                                 reference an existing capture: {}",
                                regex_caps.iter().cloned().collect::<Vec<_>>().join(", ")
                            )),
                        );
                    }
                }
                Some("config") => {
                    if !cfg.variables.contains_key(&name) {
                        push_diag(
                            out,
                            cfg,
                            p,
                            id,
                            "pattern.variableConfigUnknown",
                            Severity::Error,
                            format!(
                                "pattern `{id}` field `{field}` references `${{config:{name}}}` \
                                 but the config has no such variable"
                            ),
                            Some(format!(
                                "add `variables.{name}` to the config, or fix the template"
                            )),
                        );
                    }
                }
                Some("git" | "blame") => {
                    let ns = namespace.as_deref().unwrap_or("");
                    let expected_version = if ns == "blame" { "v0.3" } else { "v0.2" };
                    push_diag(
                        out,
                        cfg,
                        p,
                        id,
                        "pattern.variableNamespaceFuture",
                        Severity::Warning,
                        format!(
                            "pattern `{id}` field `{field}` references `${{{ns}:{name}}}` — \
                             namespace `{ns}:` is not implemented in v0.1 (expected in \
                             {expected_version})"
                        ),
                        Some(format!(
                            "this pattern will fail at scan time until {expected_version}"
                        )),
                    );
                }
                Some(_) => {
                    // env, file, ref, ide — accepted at static time;
                    // runtime checks them.
                }
            }
        }
    }

    // 4) category.unset — `kind: "url"` without a declared `category`.
    //    DESIGN.md §5.7.4. The inferred category for url is `other`,
    //    which is fine but loses semantic grouping; we surface it as
    //    `Info` so users see the suggestion without breaking CI.
    if p.kind == crate::config::PatternKind::Url && p.category.is_none() {
        push_diag(
            out,
            cfg,
            p,
            id,
            "category.unset",
            Severity::Info,
            format!(
                "pattern `{id}` declares `kind: \"url\"` without a `category`; the references \
                 browser will group it under `other`"
            ),
            Some(
                "declare one of: `people`, `tickets`, `standards`, `urls` — or a user-defined \
                 category like `slack-channels`. See DESIGN.md §5.7."
                    .into(),
            ),
        );
    }

    // 5) captureUnused: any named capture that wasn't referenced.
    for cap in &regex_caps {
        if !referenced_captures.contains(cap) {
            push_diag(
                out,
                cfg,
                p,
                id,
                "pattern.captureUnused",
                Severity::Warning,
                format!(
                    "pattern `{id}` captures `{cap}` but no template (target/title/targets[].url) \
                     references it"
                ),
                Some(format!(
                    "remove the capture group or reference it as `${{{cap}}}`"
                )),
            );
        }
    }
}
