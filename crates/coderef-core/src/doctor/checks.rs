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
