//! Pass 3 — verify co-change.
//!
//! Given the parsed `IfChangeBlock`s + a `ChangedLines` overlay,
//! emit violations for every changed block whose required peers /
//! targets are not also touched.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::diff::ChangedLines;
use super::parse::{IfChangeBlock, MarkerParseError, Target};

/// One violation surfaced by the coupled-change verifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub kind: ViolationKind,
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
    /// `true` iff no violations and no parse errors. What
    /// `coderef changes` exits zero on.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty() && self.parse_errors.is_empty()
    }
}

/// Serialisable mirror of `Violation` (kind is stringified for JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViolationReport {
    pub kind: String,
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
#[must_use]
#[allow(clippy::too_many_lines)] // pass 3 is naturally long; splitting hides flow
pub fn verify_changes(
    blocks: &[IfChangeBlock],
    parse_errors: &[MarkerParseError],
    diff: &ChangedLines,
) -> ChangesReport {
    // Index by id for Shape B peer lookup. We skip blocks without an id.
    let mut by_id: BTreeMap<&str, Vec<&IfChangeBlock>> = BTreeMap::new();
    for b in blocks {
        if let Some(ref id) = b.id {
            by_id.entry(id.as_str()).or_default().push(b);
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
            };
            if !hit {
                let msg = format!(
                    "block at `{f}:{l}` requires `{tgt}` to also change, but the diff doesn't \
                     touch it",
                    f = b.file,
                    l = b.line_start,
                    tgt = format_target(target),
                );
                this_block_violations.push(Violation {
                    kind: ViolationKind::MissingTarget,
                    file: b.file.clone(),
                    line: b.line_start,
                    message: msg,
                });
            }
        }

        // Shape B — peers with the same id.
        if let Some(ref id) = b.id {
            if let Some(peers) = by_id.get(id.as_str()) {
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
}
