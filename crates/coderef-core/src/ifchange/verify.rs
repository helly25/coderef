//! Pass 3 — verify co-change.
//!
//! Given the parsed `IfChangeBlock`s + a `ChangedLines` overlay,
//! emit violations for every changed block whose required peers /
//! targets are not also touched.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::diff::ChangedLines;
use super::parse::{IfChangeBlock, MarkerParseError, Target};
use crate::severity::Severity;

/// One violation surfaced by the coupled-change verifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub kind: ViolationKind,
    /// Severity of the violation. `Error` for hard mismatches
    /// (default); `Warning` for soft glob targets that ship the
    /// `{soft}` modifier (DESIGN §10.2). Soft warnings surface in
    /// reports but do not flip the exit code.
    pub severity: Severity,
    /// File where the violation was *detected* — i.e. the changed
    /// block whose peer/target wasn't.
    pub file: String,
    /// 1-indexed line of the changed block's `IfChange` marker.
    pub line: u32,
    /// Human-readable description (already formatted; CLI prints
    /// without further work).
    pub message: String,
}

/// Kind of coupled-change violation. Mirrors DESIGN §10.9 codes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViolationKind {
    /// Shape A: an explicit `ThenChange` target wasn't touched by the
    /// diff.
    MissingTarget,
    /// Shape B: a sibling block (same id, different file or position)
    /// wasn't touched.
    MissingPeer,
}

/// Aggregate output of `verify_changes`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangesReport {
    /// All parsed blocks in the workspace, sorted by file then line.
    pub block_count: usize,
    /// Number of blocks the diff actually touched.
    pub changed_block_count: usize,
    /// Number of blocks that emitted at least one violation.
    pub violating_block_count: usize,
    /// Number of NoVerify-skipped blocks (logged for the audit trail).
    pub no_verify_block_count: usize,
    /// Marker-parse errors collected while scanning (per-file orphans,
    /// malformed targets). Surfaced so the CLI can mention them.
    pub parse_errors: Vec<ParseErrorReport>,
    /// One entry per detected violation.
    pub violations: Vec<ViolationReport>,
}

impl ChangesReport {
    /// `true` iff no failing violations and no parse errors. What
    /// `coderef changes` exits zero on. Soft (warning-severity)
    /// violations are surfaced in the report but don't flip the
    /// exit code — they're advisory.
    #[must_use]
    pub fn passed(&self) -> bool {
        !self.violations.iter().any(|v| v.severity.is_failure()) && self.parse_errors.is_empty()
    }
}

/// Serialisable mirror of `Violation` (kind is stringified for JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViolationReport {
    pub kind: String,
    /// `"error"` (default) or `"warning"` for soft glob mismatches.
    /// Earlier JSON consumers that ignore unknown fields keep working —
    /// `severity` is additive.
    pub severity: Severity,
    pub file: String,
    pub line: u32,
    pub message: String,
}

/// Serialisable mirror of `MarkerParseError`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParseErrorReport {
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub message: String,
}

/// Pass 3. `blocks` is the workspace-wide collection (already parsed
/// per-file via `extract_blocks` and concatenated); `parse_errors`
/// carries forward any marker-parse failures the caller collected.
///
/// Shape B groups by literal id text. For Shape C (composable ids
/// resolved through the reference engine), use
/// [`verify_changes_composable`].
#[must_use]
pub fn verify_changes(
    blocks: &[IfChangeBlock],
    parse_errors: &[MarkerParseError],
    diff: &ChangedLines,
) -> ChangesReport {
    verify_changes_composable(
        blocks,
        parse_errors,
        diff,
        None::<&dyn Fn(&str) -> Option<String>>,
    )
}

/// Pass 3 with optional composable-id resolution (DESIGN §10.7).
///
/// When `resolver` is `Some`, every block's id is passed through it
/// before being used as the Shape B group key — so
/// `IfChange(JIRA(PROJ-123))` blocks coalesce regardless of which
/// file they appear in, provided the resolver maps the id to a
/// stable canonical form. When `resolver` returns `None` for an id
/// (or when no resolver is supplied), the literal id text is used,
/// keeping Shape B semantics unchanged.
#[must_use]
#[allow(clippy::too_many_lines)] // pass 3 is naturally long; splitting hides flow
pub fn verify_changes_composable<F>(
    blocks: &[IfChangeBlock],
    parse_errors: &[MarkerParseError],
    diff: &ChangedLines,
    resolver: Option<&F>,
) -> ChangesReport
where
    F: Fn(&str) -> Option<String> + ?Sized,
{
    // Resolve each block's id once, up front. The resolved key is the
    // resolver's output (when Some) or the literal id (when None / no
    // resolver). For lookup-by-resolved-id, we share one helper.
    let resolve = |id: &str| -> String {
        resolver
            .and_then(|r| r(id))
            .unwrap_or_else(|| id.to_string())
    };

    // Index by resolved id for Shape B / C peer lookup. We skip
    // blocks without an id.
    let mut by_id: BTreeMap<String, Vec<&IfChangeBlock>> = BTreeMap::new();
    for b in blocks {
        if let Some(ref id) = b.id {
            by_id.entry(resolve(id)).or_default().push(b);
        }
    }

    // Index by (file, label) → block range for `FileLabel` target
    // lookups. The label *is* the IfChange id; the path is normalized
    // by stripping a leading `/` so workspace-rooted targets match
    // workspace-relative block paths.
    let mut by_label: BTreeMap<(String, String), (u32, u32)> = BTreeMap::new();
    for b in blocks {
        if let Some(ref id) = b.id {
            let normalized_path = b.file.trim_start_matches('/').to_string();
            by_label.insert((normalized_path, id.clone()), (b.line_start, b.line_end));
        }
    }

    let mut violations = Vec::new();
    let mut changed_block_count = 0usize;
    let mut violating_block_count = 0usize;
    let mut no_verify_block_count = 0usize;

    for b in blocks {
        let changed = diff.intersects(&b.file, b.line_start, b.line_end);
        if !changed {
            continue;
        }
        changed_block_count += 1;
        if b.no_verify_reason.is_some() {
            no_verify_block_count += 1;
            continue;
        }
        let mut this_block_violations = Vec::new();

        // Shape A — explicit targets.
        for target in &b.targets {
            let hit = match target {
                Target::File { path } => diff.file_touched(path),
                Target::FileLine { path, line } => diff.intersects(path, *line, *line),
                Target::FileLineRange { path, start, end } => diff.intersects(path, *start, *end),
                // Anchor target: the section under `#anchor` must
                // change. v0.2 semantics (simple): the anchor must
                // exist in the target file *and* any line in the file
                // changed. A richer "heading-bounded range" check
                // lands in v0.3 once we track heading line numbers
                // (DESIGN §10.2 + §6.3).
                Target::FileAnchor { path, .. } => diff.file_touched(path),
                // Label target: `path:label-name` resolves to the
                // block opened by `IfChange('label-name')` in the
                // target file. The block's line range is the change
                // requirement.
                Target::FileLabel { path, label } => {
                    let key = (path.trim_start_matches('/').to_string(), label.clone());
                    by_label
                        .get(&key)
                        .is_some_and(|&(start, end)| diff.intersects(path, start, end))
                }
                // Glob target: `/path/*.md{any|all|soft}`. Match the
                // pattern against the diff's changed-file list.
                Target::FileGlob { pattern, flags } => {
                    let stripped = pattern.trim_start_matches('/');
                    let glob_result = globset::Glob::new(stripped);
                    if let Ok(glob) = glob_result {
                        let matcher = glob.compile_matcher();
                        let touched = diff.files();
                        let matched_count = touched.iter().filter(|f| matcher.is_match(f)).count();
                        match flags.mode {
                            // `any`: at least one matched-and-changed.
                            // `all` v0.2 semantics: same as `any` — the
                            // strict "every workspace file matching
                            // the glob must change" form needs a full
                            // workspace enumeration that the verifier
                            // doesn't have at this layer. Tracked as
                            // a follow-up in DESIGN §10.2.
                            super::parse::GlobMode::Any | super::parse::GlobMode::All => {
                                matched_count > 0
                            }
                        }
                    } else {
                        // A malformed glob can't be satisfied — treat
                        // as missing-target. (The marker parser
                        // doesn't validate glob syntax to match the
                        // relaxed file-target parse.)
                        false
                    }
                }
            };
            if !hit {
                // Severity drops to Warning iff the target carries
                // the `{soft}` flag (only meaningful on globs today;
                // other target kinds always emit Error).
                let severity = match target {
                    Target::FileGlob {
                        flags: super::parse::GlobFlags { soft: true, .. },
                        ..
                    } => Severity::Warning,
                    _ => Severity::Error,
                };
                let msg = format!(
                    "block at `{f}:{l}` requires `{tgt}` to also change, but the diff doesn't \
                     touch it",
                    f = b.file,
                    l = b.line_start,
                    tgt = format_target(target),
                );
                this_block_violations.push(Violation {
                    kind: ViolationKind::MissingTarget,
                    severity,
                    file: b.file.clone(),
                    line: b.line_start,
                    message: msg,
                });
            }
        }

        // Shape B / C — peers with the same (resolved) id.
        if let Some(ref id) = b.id {
            let resolved_id = resolve(id);
            if let Some(peers) = by_id.get(&resolved_id) {
                for peer in peers {
                    if std::ptr::eq(*peer, b) {
                        continue;
                    }
                    let peer_changed = diff.intersects(&peer.file, peer.line_start, peer.line_end);
                    if !peer_changed {
                        let msg = format!(
                            "block at `{f}:{l}` shares id `{id}` with `{pf}:{pl}` which the \
                             diff doesn't touch",
                            f = b.file,
                            l = b.line_start,
                            pf = peer.file,
                            pl = peer.line_start,
                        );
                        this_block_violations.push(Violation {
                            kind: ViolationKind::MissingPeer,
                            severity: Severity::Error,
                            file: b.file.clone(),
                            line: b.line_start,
                            message: msg,
                        });
                    }
                }
            }
        }

        if !this_block_violations.is_empty() {
            violating_block_count += 1;
            violations.extend(this_block_violations);
        }
    }

    let parse_errors: Vec<ParseErrorReport> = parse_errors
        .iter()
        .map(|e| ParseErrorReport {
            kind: parse_error_kind(e).to_string(),
            file: parse_error_file(e),
            line: parse_error_line(e),
            message: e.to_string(),
        })
        .collect();
    let violations: Vec<ViolationReport> = violations
        .into_iter()
        .map(|v| ViolationReport {
            kind: match v.kind {
                ViolationKind::MissingTarget => "missing-target".to_string(),
                ViolationKind::MissingPeer => "missing-peer".to_string(),
            },
            severity: v.severity,
            file: v.file,
            line: v.line,
            message: v.message,
        })
        .collect();

    ChangesReport {
        block_count: blocks.len(),
        changed_block_count,
        violating_block_count,
        no_verify_block_count,
        parse_errors,
        violations,
    }
}

fn format_target(t: &Target) -> String {
    match t {
        Target::File { path } => path.clone(),
        Target::FileLine { path, line } => format!("{path}:{line}"),
        Target::FileLineRange { path, start, end } => format!("{path}:{start}-{end}"),
        Target::FileAnchor { path, anchor } => format!("{path}#{anchor}"),
        Target::FileLabel { path, label } => format!("{path}:{label}"),
        Target::FileGlob { pattern, flags } => {
            // Build the brace-suffix as the user wrote it: emit only
            // the non-default tokens. Default is `{any}` with no soft
            // — those produce a bare `pattern` for cleanliness.
            let mut tokens: Vec<&'static str> = Vec::new();
            match flags.mode {
                super::parse::GlobMode::Any => {}
                super::parse::GlobMode::All => tokens.push("all"),
            }
            if flags.soft {
                tokens.push("soft");
            }
            if tokens.is_empty() {
                pattern.clone()
            } else {
                format!("{pattern}{{{}}}", tokens.join(","))
            }
        }
    }
}

fn parse_error_kind(e: &MarkerParseError) -> &'static str {
    match e {
        MarkerParseError::OrphanIfChange { .. } => "orphan-ifchange",
        MarkerParseError::OrphanThenChange { .. } => "orphan-thenchange",
        MarkerParseError::MalformedTarget { .. } => "malformed-target",
    }
}

fn parse_error_file(e: &MarkerParseError) -> String {
    match e {
        MarkerParseError::OrphanIfChange { file, .. }
        | MarkerParseError::OrphanThenChange { file, .. }
        | MarkerParseError::MalformedTarget { file, .. } => file.clone(),
    }
}

fn parse_error_line(e: &MarkerParseError) -> u32 {
    match e {
        MarkerParseError::OrphanIfChange { line, .. }
        | MarkerParseError::OrphanThenChange { line, .. }
        | MarkerParseError::MalformedTarget { line, .. } => *line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(
        file: &str,
        ls: u32,
        le: u32,
        id: Option<&str>,
        targets: Vec<Target>,
    ) -> IfChangeBlock {
        IfChangeBlock {
            file: file.to_string(),
            line_start: ls,
            line_end: le,
            id: id.map(str::to_string),
            targets,
            no_verify_reason: None,
        }
    }

    #[test]
    fn test_unchanged_block_emits_no_violation() {
        let b = block(
            "a.rs",
            1,
            10,
            None,
            vec![Target::File {
                path: "b.md".into(),
            }],
        );
        let cl = ChangedLines::from_pairs(&[("c.rs", &[(1, 5)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 0);
        assert_eq!(r.changed_block_count, 0);
    }

    #[test]
    fn test_changed_shape_a_missing_target_emits_violation() {
        let b = block(
            "a.rs",
            1,
            10,
            None,
            vec![Target::File {
                path: "b.md".into(),
            }],
        );
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(5, 5)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
        assert!(r.violations[0].message.contains("b.md"));
        assert!(!r.passed());
    }

    #[test]
    fn test_changed_shape_a_target_also_changed_passes() {
        let b = block(
            "a.rs",
            1,
            10,
            None,
            vec![Target::File {
                path: "b.md".into(),
            }],
        );
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(5, 5)]), ("b.md", &[(1, 1)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_changed_shape_a_line_range_target_intersection() {
        let b = block(
            "a.rs",
            1,
            10,
            None,
            vec![Target::FileLineRange {
                path: "b.rs".into(),
                start: 100,
                end: 200,
            }],
        );
        // Change in target is line 150 — inside the requested range.
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(5, 5)]), ("b.rs", &[(150, 150)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert!(r.passed());
    }

    #[test]
    fn test_changed_shape_a_line_range_target_miss_outside_range() {
        let b = block(
            "a.rs",
            1,
            10,
            None,
            vec![Target::FileLineRange {
                path: "b.rs".into(),
                start: 100,
                end: 200,
            }],
        );
        // Change in target is at line 250 — outside the range.
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(5, 5)]), ("b.rs", &[(250, 250)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
    }

    #[test]
    fn test_changed_shape_b_peer_not_changed_violates() {
        let b1 = block("a.rs", 1, 5, Some("auth-v3"), vec![]);
        let b2 = block("b.rs", 1, 5, Some("auth-v3"), vec![]);
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b1, b2], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-peer");
        assert!(r.violations[0].message.contains("b.rs"));
    }

    #[test]
    fn test_changed_shape_b_both_peers_changed_passes() {
        let b1 = block("a.rs", 1, 5, Some("auth-v3"), vec![]);
        let b2 = block("b.rs", 10, 15, Some("auth-v3"), vec![]);
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)]), ("b.rs", &[(12, 12)])]);
        let r = verify_changes(&[b1, b2], &[], &cl);
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_anchor_target_unchanged_file_emits_missing_target() {
        let b = block(
            "src/code.rs",
            1,
            5,
            None,
            vec![Target::FileAnchor {
                path: "docs/security.md".into(),
                anchor: "hashing".into(),
            }],
        );
        let cl = ChangedLines::from_pairs(&[("src/code.rs", &[(2, 2)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
        assert!(r.violations[0].message.contains("docs/security.md#hashing"));
    }

    #[test]
    fn test_label_target_resolves_to_named_block_in_target_file() {
        // Block A in src/a.rs targets `/docs/x.md:section-name`.
        // Block B in docs/x.md has IfChange('section-name').
        let block_a = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileLabel {
                path: "/docs/x.md".into(),
                label: "section-name".into(),
            }],
            no_verify_reason: None,
        };
        let block_b = IfChangeBlock {
            file: "docs/x.md".into(),
            line_start: 50,
            line_end: 60,
            id: Some("section-name".into()),
            targets: vec![],
            no_verify_reason: None,
        };
        // Diff: block_a changes; docs/x.md line 55 also changes
        // (inside block_b's range 50..60).
        let cl = ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)]), ("docs/x.md", &[(55, 55)])]);
        let r = verify_changes(&[block_a, block_b], &[], &cl);
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_label_target_unsatisfied_when_diff_outside_named_block_range() {
        let block_a = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileLabel {
                path: "/docs/x.md".into(),
                label: "section".into(),
            }],
            no_verify_reason: None,
        };
        let block_b = IfChangeBlock {
            file: "docs/x.md".into(),
            line_start: 50,
            line_end: 60,
            id: Some("section".into()),
            targets: vec![],
            no_verify_reason: None,
        };
        // Diff touches block_a + a *different* line in docs/x.md
        // (line 5 — outside block_b's [50, 60] range).
        let cl = ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)]), ("docs/x.md", &[(5, 5)])]);
        let r = verify_changes(&[block_a, block_b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
        assert!(r.violations[0].message.contains("/docs/x.md:section"));
    }

    #[test]
    fn test_label_target_unknown_label_treated_as_missing() {
        // Target references a label that no block declares — the
        // lookup returns None and the target is missing.
        let block_a = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileLabel {
                path: "/docs/x.md".into(),
                label: "nonexistent".into(),
            }],
            no_verify_reason: None,
        };
        let cl = ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)])]);
        let r = verify_changes(&[block_a], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
    }

    #[test]
    fn test_composable_resolver_groups_blocks_with_distinct_ids_to_same_resolved_key() {
        // Two blocks in different files with id texts that the
        // resolver maps to the *same* canonical key. They should
        // group under Shape B-style peer-check.
        let b1 = block("a.rs", 1, 5, Some("JIRA(PROJ-1)"), vec![]);
        let b2 = block("b.rs", 10, 15, Some("JIRA(PROJ-1)"), vec![]);
        let resolver = |id: &str| -> Option<String> {
            // Mock: every JIRA(...) maps to a canonical jira-url.
            if id.starts_with("JIRA(") {
                Some(format!("https://j/{id}"))
            } else {
                None
            }
        };
        // Both blocks change → both peers in the group → passes.
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)]), ("b.rs", &[(12, 12)])]);
        let r = verify_changes_composable(&[b1, b2], &[], &cl, Some(&resolver));
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_composable_resolver_one_peer_unchanged_emits_missing_peer() {
        let b1 = block("a.rs", 1, 5, Some("JIRA(PROJ-9)"), vec![]);
        let b2 = block("b.rs", 10, 15, Some("JIRA(PROJ-9)"), vec![]);
        let resolver = |id: &str| -> Option<String> {
            if id.starts_with("JIRA(") {
                Some(format!("https://j/{id}"))
            } else {
                None
            }
        };
        // Only a.rs changes; b.rs unchanged → peer violation.
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)])]);
        let r = verify_changes_composable(&[b1, b2], &[], &cl, Some(&resolver));
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-peer");
    }

    #[test]
    fn test_composable_resolver_none_falls_back_to_literal_id() {
        // Resolver returns None for everything → behaves like Shape B
        // with literal ids. Different literal ids → no grouping.
        let b1 = block("a.rs", 1, 5, Some("alpha"), vec![]);
        let b2 = block("b.rs", 10, 15, Some("beta"), vec![]);
        let resolver = |_: &str| None;
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)])]);
        let r = verify_changes_composable(&[b1, b2], &[], &cl, Some(&resolver));
        // No groups, no peer requirement → passes.
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_composable_no_resolver_argument_matches_legacy_behaviour() {
        // verify_changes is a thin wrapper for the no-resolver case.
        let b1 = block("a.rs", 1, 5, Some("grp"), vec![]);
        let b2 = block("b.rs", 10, 15, Some("grp"), vec![]);
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b1, b2], &[], &cl);
        // Shape B: literal id "grp" matches across files → peer needed.
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-peer");
    }

    #[test]
    fn test_glob_target_matches_any_changed_file() {
        let b = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileGlob {
                pattern: "/docs/*.md".into(),
                flags: super::super::parse::GlobFlags::default(),
            }],
            no_verify_reason: None,
        };
        let cl =
            ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)]), ("docs/security.md", &[(50, 50)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_glob_target_no_matching_file_changed_fails() {
        let b = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileGlob {
                pattern: "/docs/*.md".into(),
                flags: super::super::parse::GlobFlags::default(),
            }],
            no_verify_reason: None,
        };
        // src/a.rs changed; no docs/*.md changed.
        let cl = ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
        // `format_target` omits the redundant `{any}` suffix on the
        // default flag set; the bare pattern is enough.
        assert!(r.violations[0].message.contains("/docs/*.md"));
    }

    #[test]
    fn test_glob_target_doublestar_recursive_match() {
        let b = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileGlob {
                pattern: "/docs/**/*.md".into(),
                flags: super::super::parse::GlobFlags::default(),
            }],
            no_verify_reason: None,
        };
        let cl = ChangedLines::from_pairs(&[
            ("src/a.rs", &[(2, 2)]),
            ("docs/api/v2/index.md", &[(1, 1)]),
        ]);
        let r = verify_changes(&[b], &[], &cl);
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_anchor_target_changed_file_passes() {
        let b = block(
            "src/code.rs",
            1,
            5,
            None,
            vec![Target::FileAnchor {
                path: "docs/security.md".into(),
                anchor: "hashing".into(),
            }],
        );
        let cl = ChangedLines::from_pairs(&[
            ("src/code.rs", &[(2, 2)]),
            ("docs/security.md", &[(50, 50)]),
        ]);
        let r = verify_changes(&[b], &[], &cl);
        assert!(r.passed(), "{r:#?}");
    }

    #[test]
    fn test_no_verify_block_skipped_but_counted() {
        let mut b = block(
            "a.rs",
            1,
            5,
            None,
            vec![Target::File {
                path: "missing.md".into(),
            }],
        );
        b.no_verify_reason = Some("one-shot refactor".into());
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert!(r.passed());
        assert_eq!(r.no_verify_block_count, 1);
        assert_eq!(r.changed_block_count, 1);
        assert_eq!(r.violations.len(), 0);
    }

    #[test]
    fn test_parse_errors_forwarded_into_report_and_fail_passed() {
        let r = verify_changes(
            &[],
            &[MarkerParseError::OrphanIfChange {
                file: "x.rs".into(),
                line: 7,
            }],
            &ChangedLines::default(),
        );
        assert!(!r.passed());
        assert_eq!(r.parse_errors.len(), 1);
        assert_eq!(r.parse_errors[0].kind, "orphan-ifchange");
    }

    // ---------------------------------------------------------------
    // `{soft}` glob flag — DESIGN §10.2 severity modifier. A soft
    // glob target that misses still surfaces a violation, but with
    // Severity::Warning so `passed()` returns true (advisory, not
    // failing).
    // ---------------------------------------------------------------

    #[test]
    fn test_soft_glob_target_miss_is_warning_not_error() {
        let b = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileGlob {
                pattern: "/docs/*.md".into(),
                flags: super::super::parse::GlobFlags {
                    mode: super::super::parse::GlobMode::Any,
                    soft: true,
                },
            }],
            no_verify_reason: None,
        };
        // a.rs changed, but no /docs/*.md file changed → glob misses.
        let cl = ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-target");
        assert_eq!(r.violations[0].severity, Severity::Warning);
        // Soft warnings don't fail the run.
        assert!(r.passed(), "soft glob miss must not fail passed()");
    }

    #[test]
    fn test_non_soft_glob_target_miss_is_error() {
        let b = IfChangeBlock {
            file: "src/a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: None,
            targets: vec![Target::FileGlob {
                pattern: "/docs/*.md".into(),
                flags: super::super::parse::GlobFlags::default(), // any, not soft
            }],
            no_verify_reason: None,
        };
        let cl = ChangedLines::from_pairs(&[("src/a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].severity, Severity::Error);
        assert!(!r.passed(), "non-soft glob miss must fail passed()");
    }

    #[test]
    fn test_peer_mismatch_severity_is_always_error() {
        // Shape B peer-mismatch violations are not modifiable by
        // glob flags (peers aren't globs). Confirm Severity::Error.
        let b1 = IfChangeBlock {
            file: "a.rs".into(),
            line_start: 1,
            line_end: 3,
            id: Some("k".into()),
            targets: vec![],
            no_verify_reason: None,
        };
        let b2 = IfChangeBlock {
            file: "b.rs".into(),
            line_start: 5,
            line_end: 7,
            id: Some("k".into()),
            targets: vec![],
            no_verify_reason: None,
        };
        // a.rs changes; b.rs doesn't.
        let cl = ChangedLines::from_pairs(&[("a.rs", &[(2, 2)])]);
        let r = verify_changes(&[b1, b2], &[], &cl);
        assert_eq!(r.violations.len(), 1);
        assert_eq!(r.violations[0].kind, "missing-peer");
        assert_eq!(r.violations[0].severity, Severity::Error);
    }
}
