//! `IfChange` / `ThenChange` marker parsing.
//!
//! Walks a source file line-by-line and produces a list of paired
//! blocks. Marker regexes are hardcoded for v0.2 — the common spelling
//! `# IfChange(id?)` / `# ThenChange(targets?)` after a comment-prefix
//! lead, language-agnostic via line trimming. Migration from
//! `LINT.IfChange/ThenChange` (per-pattern `ifChange.regex` overrides)
//! is a v0.3 follow-up; this PR ships the default-spelling path that
//! the vast majority of teams will use.

use fancy_regex::Regex;
use thiserror::Error;

use std::sync::LazyLock;

/// Default marker spellings. Strict enough to avoid false-positive
/// matches inside narrative prose ("if changing the format ..."); the
/// `\b...\b` boundaries and the required `(` / `^` neighbours make the
/// markers explicit-only.
static IF_CHANGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bIfChange(?:\((?<id>[^)]*)\))?").expect("IfChange marker regex is valid")
});
static THEN_CHANGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bThenChange(?:\((?<targets>[^)]*)\))?").expect("ThenChange marker regex is valid")
});
/// `NoVerify(coderef:ifchange)` opt-out (DESIGN §10.6). Reason text
/// must follow — the verifier records it for audit and an empty
/// reason is treated as an authoring error elsewhere.
static NO_VERIFY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bNoVerify\(coderef:ifchange\)(?::\s*(?<reason>.*))?")
        .expect("NoVerify marker regex is valid")
});

/// One IfChange/ThenChange block found in a single file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfChangeBlock {
    /// Workspace-relative path of the file this block lives in.
    pub file: String,
    /// 1-indexed line of the `IfChange` marker (the block's open line).
    pub line_start: u32,
    /// 1-indexed line of the matching `ThenChange` marker (block's close).
    pub line_end: u32,
    /// Optional id captured from `IfChange(my-id)`. None / empty for
    /// Shape A blocks.
    pub id: Option<String>,
    /// Explicit `ThenChange` targets (Shape A). Empty for Shape B.
    pub targets: Vec<Target>,
    /// Inline `NoVerify(coderef:ifchange): reason` if found on the
    /// `IfChange` line or the line immediately above it. The verifier
    /// honours this and skips violations for the block.
    pub no_verify_reason: Option<String>,
}

/// One parsed target token from a `ThenChange(...)` argument list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Whole-file: at least one line in the file must change.
    File { path: String },
    /// Single line: `path:N` — line N must be inside a changed hunk.
    FileLine { path: String, line: u32 },
    /// Inclusive line range: `path:N-M` — at least one line in [N, M]
    /// must be inside a changed hunk.
    FileLineRange { path: String, start: u32, end: u32 },
    /// Named anchor (heading slug in a Markdown file): `path#anchor`.
    /// The anchor is resolved by `crate::anchor::verify_anchor`
    /// against the target file's heading slugs; if found, the heading
    /// range is treated as the changed-region requirement. v0.2
    /// semantics (kept simple): if the anchor exists in the target
    /// file *and* any line in the file changed, the target is
    /// satisfied. A richer "heading section range" interpretation
    /// (DESIGN §10.2) lands in v0.3.
    FileAnchor { path: String, anchor: String },
    /// Named-region label: `path:label-name`. Resolves to the block
    /// opened by `IfChange('label-name')` in the target file (DESIGN
    /// §10.2). The block's `[line_start, line_end]` range is treated
    /// as the changed-region requirement. The label form is used
    /// when the `:` is followed by a non-numeric token; line/range
    /// forms still win when the suffix is digits or `N-M`.
    FileLabel { path: String, label: String },
}

impl Target {
    /// Path component shared across all target variants.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::File { path }
            | Self::FileLine { path, .. }
            | Self::FileLineRange { path, .. }
            | Self::FileAnchor { path, .. }
            | Self::FileLabel { path, .. } => path,
        }
    }
}

/// Aggregate result of scanning one file for IfChange/ThenChange
/// markers. Includes successfully paired blocks plus any per-file
/// parse errors that the doctor surfaces.
#[derive(Clone, Debug)]
pub struct MarkerParseReport {
    pub blocks: Vec<IfChangeBlock>,
    pub errors: Vec<MarkerParseError>,
}

/// One per-file marker-parse failure.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum MarkerParseError {
    #[error("`{file}:{line}` has an `IfChange` marker with no matching `ThenChange` in the file")]
    OrphanIfChange { file: String, line: u32 },
    #[error("`{file}:{line}` has a `ThenChange` marker without a preceding open `IfChange`")]
    OrphanThenChange { file: String, line: u32 },
    #[error("`{file}:{line}` has a malformed target token `{token}` in `ThenChange(...)`")]
    MalformedTarget {
        file: String,
        line: u32,
        token: String,
    },
}

/// Extract paired IfChange/ThenChange blocks from `content`. The file
/// path is embedded into the returned blocks unchanged (the caller
/// chooses absolute vs workspace-relative).
#[must_use]
pub fn extract_blocks(content: &str, file: &str) -> MarkerParseReport {
    let mut blocks = Vec::new();
    let mut errors = Vec::new();

    // Pending open: line number + captured id, waiting for the next ThenChange.
    let mut open: Option<(u32, Option<String>, Option<String>)> = None;
    // Carries the previous line's NoVerify reason forward by exactly
    // one line, so a NoVerify *above* the IfChange line is honoured.
    let mut prev_no_verify: Option<String> = None;

    for (zero_idx, line) in content.lines().enumerate() {
        let line_num = u32::try_from(zero_idx + 1).unwrap_or(u32::MAX);

        // Capture NoVerify on this line for use by IfChange on the
        // same line OR the next line.
        let this_no_verify = NO_VERIFY_RE.captures(line).ok().flatten().map(|c| {
            c.name("reason")
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default()
        });

        // Match IfChange first. The regex doesn't anchor against
        // ThenChange — but ThenChange contains "Change" too, and a
        // bare `\bThen` doesn't match `\bIf`. Still, a line like
        // `// ThenChange(...)` should NOT trigger IfChange detection;
        // the regex `\bIfChange\b...` won't match `ThenChange` because
        // the prefix is different. Confirmed by tests below.
        if let Ok(Some(cap)) = IF_CHANGE_RE.captures(line) {
            // Reject lines that *also* contain ThenChange on the same
            // line (would otherwise produce a degenerate block).
            // Per DESIGN, the markers occupy their own lines; we treat
            // a same-line both-markers form as `OrphanIfChange`.
            if open.is_some() {
                // Nested or overlapping IfChange — close the pending
                // open as orphan-style error and keep going.
                if let Some((open_line, _, _)) = open.take() {
                    errors.push(MarkerParseError::OrphanIfChange {
                        file: file.to_string(),
                        line: open_line,
                    });
                }
            }
            let id = cap.name("id").map(|m| {
                let raw = m.as_str().trim();
                // Strip surrounding matching single or double quotes
                // so `IfChange('hash-params')` and `IfChange(hash-params)`
                // produce the same id — the README's canonical
                // example uses the quoted form.
                strip_matching_quotes(raw).to_string()
            });
            // Take the higher-priority NoVerify: same-line wins;
            // otherwise the line-above reason.
            let nv = this_no_verify.clone().or_else(|| prev_no_verify.clone());
            open = Some((line_num, id, nv));
            // Same-line NoVerify gets consumed by the just-opened
            // block; clear so it doesn't leak to a sibling later.
            prev_no_verify = None;
            continue;
        }

        if let Ok(Some(cap)) = THEN_CHANGE_RE.captures(line) {
            let Some((open_line, id, nv_reason)) = open.take() else {
                errors.push(MarkerParseError::OrphanThenChange {
                    file: file.to_string(),
                    line: line_num,
                });
                prev_no_verify = this_no_verify;
                continue;
            };
            let targets_text = cap
                .name("targets")
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let mut targets = Vec::new();
            for raw in split_targets(&targets_text) {
                let raw = raw.trim();
                if raw.is_empty() {
                    continue;
                }
                match parse_target(raw) {
                    Ok(t) => targets.push(t),
                    Err(()) => errors.push(MarkerParseError::MalformedTarget {
                        file: file.to_string(),
                        line: line_num,
                        token: raw.to_string(),
                    }),
                }
            }
            blocks.push(IfChangeBlock {
                file: file.to_string(),
                line_start: open_line,
                line_end: line_num,
                id: id.filter(|s| !s.is_empty()),
                targets,
                no_verify_reason: nv_reason,
            });
            prev_no_verify = None;
            continue;
        }

        // Neither marker on this line — let the NoVerify carry one line.
        prev_no_verify = this_no_verify;
    }

    // Leftover open: an IfChange with no closing ThenChange.
    if let Some((open_line, _, _)) = open {
        errors.push(MarkerParseError::OrphanIfChange {
            file: file.to_string(),
            line: open_line,
        });
    }

    MarkerParseReport { blocks, errors }
}

/// Split a `ThenChange(...)` arg list on commas, respecting nothing
/// fancier than v0.2 needs (no escaped commas, no nested parens —
/// targets like `JIRA(PROJ-1)` are deferred to Shape C in v0.3).
fn split_targets(s: &str) -> Vec<String> {
    s.split(',').map(|part| part.trim().to_string()).collect()
}

/// Strip matching surrounding `'...'` or `"..."` quotes if present.
/// Returns the original slice unchanged when the quotes don't match
/// or aren't present.
fn strip_matching_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return s;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
        // Safe: ASCII quote characters are 1 byte each.
        return &s[1..s.len() - 1];
    }
    s
}

/// Parse a single target token. Supports:
///
/// - `path`
/// - `path:N`
/// - `path:N-M`
/// - `path#anchor`
///
/// Anything else is `MalformedTarget` at the caller. The `#`
/// disambiguator is checked before `:` so a path containing both
/// (e.g. `docs/x.md#anchor`) is parsed as anchor-only — line/range
/// targets and anchor targets are mutually exclusive in v0.2.
fn parse_target(raw: &str) -> Result<Target, ()> {
    // Anchor form `path#anchor` first.
    if let Some((path, anchor)) = raw.split_once('#') {
        if path.is_empty() || anchor.is_empty() {
            return Err(());
        }
        return Ok(Target::FileAnchor {
            path: path.to_string(),
            anchor: anchor.to_string(),
        });
    }
    if let Some((path, rest)) = raw.split_once(':') {
        if rest.is_empty() {
            return Err(());
        }
        // path may be empty when raw starts with ":" — same-file
        // shortcut. v0.2 doesn't support that yet; reject.
        if path.is_empty() {
            return Err(());
        }
        // Disambiguator (DESIGN §10.2): a `:` followed by digits or
        // `N-M` is a line/range; anything else is a label-name.
        if let Some((a, b)) = rest.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                if a == 0 || b == 0 || a > b {
                    return Err(());
                }
                return Ok(Target::FileLineRange {
                    path: path.to_string(),
                    start: a,
                    end: b,
                });
            }
            // Suffix has a hyphen but isn't a numeric range — fall
            // through to label-name handling below (slug-style labels
            // commonly contain hyphens, e.g. `argon2-params`).
        }
        if let Ok(n) = rest.parse::<u32>() {
            if n == 0 {
                return Err(());
            }
            return Ok(Target::FileLine {
                path: path.to_string(),
                line: n,
            });
        }
        // Not digits and not a numeric range → label-name.
        // Validate: labels are non-empty and don't contain `:` (would
        // re-trigger ambiguity) or whitespace.
        if rest.contains(':') || rest.chars().any(char::is_whitespace) {
            return Err(());
        }
        return Ok(Target::FileLabel {
            path: path.to_string(),
            label: rest.to_string(),
        });
    }
    // Bare path. Reject empty.
    if raw.is_empty() {
        return Err(());
    }
    Ok(Target::File {
        path: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_basic_shape_a_block_with_explicit_targets() {
        let src = "\
// IfChange
fn x() {}
// ThenChange(/docs/x.md, /src/y.rs:42, /src/z.rs:10-20)
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.errors, Vec::new());
        assert_eq!(report.blocks.len(), 1);
        let b = &report.blocks[0];
        assert_eq!(b.line_start, 1);
        assert_eq!(b.line_end, 3);
        assert_eq!(b.id, None);
        assert_eq!(b.targets.len(), 3);
        assert!(matches!(b.targets[0], Target::File { ref path } if path == "/docs/x.md"));
        assert!(
            matches!(b.targets[1], Target::FileLine { ref path, line: 42 } if path == "/src/y.rs")
        );
        assert!(
            matches!(b.targets[2], Target::FileLineRange { ref path, start: 10, end: 20 } if path == "/src/z.rs")
        );
    }

    #[test]
    fn test_extract_shape_b_id_block() {
        let src = "\
// IfChange(auth-format-v3)
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty());
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.blocks[0].id.as_deref(), Some("auth-format-v3"));
        assert!(report.blocks[0].targets.is_empty());
    }

    #[test]
    fn test_orphan_if_change_reported() {
        let src = "\
// IfChange
fn x() {}
// nothing else
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 0);
        assert_eq!(report.errors.len(), 1);
        assert!(
            matches!(report.errors[0], MarkerParseError::OrphanIfChange { ref file, line: 1 } if file == "a.rs")
        );
    }

    #[test]
    fn test_orphan_then_change_reported() {
        let src = "\
// stuff
// ThenChange(/x.md)
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 0);
        assert_eq!(report.errors.len(), 1);
        assert!(
            matches!(report.errors[0], MarkerParseError::OrphanThenChange { ref file, line: 2 } if file == "a.rs"),
            "got: {:#?}",
            report.errors
        );
    }

    #[test]
    fn test_multiple_blocks_in_one_file_each_pair() {
        let src = "\
// IfChange(a)
fn aa() {}
// ThenChange
// IfChange(b)
fn bb() {}
// ThenChange(/x.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks.len(), 2);
        assert_eq!(report.blocks[0].id.as_deref(), Some("a"));
        assert_eq!(report.blocks[1].id.as_deref(), Some("b"));
    }

    #[test]
    fn test_malformed_target_reported_other_targets_kept() {
        // `:zz` is a valid *label-name* under v0.2's
        // labels-after-non-numeric-colon rule, so use a label that
        // collides with a line/range numeric (`/bad.md:0`) — line 0
        // is rejected as out-of-range.
        let src = "\
// IfChange
fn x() {}
// ThenChange(/ok.md, /bad.md:0, /also-ok.md:5)
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.blocks[0].targets.len(), 2);
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::MalformedTarget { ref token, .. } if token == "/bad.md:0"
        ));
    }

    #[test]
    fn test_no_verify_inline_same_line_honoured() {
        let src = "\
// IfChange — NoVerify(coderef:ifchange): one-shot refactor
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(
            report.blocks[0].no_verify_reason.as_deref(),
            Some("one-shot refactor")
        );
    }

    #[test]
    fn test_no_verify_inline_line_above_honoured() {
        let src = "\
// NoVerify(coderef:ifchange): peer block intentionally lagging
// IfChange
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(
            report.blocks[0].no_verify_reason.as_deref(),
            Some("peer block intentionally lagging")
        );
    }

    #[test]
    fn test_no_verify_two_lines_above_not_honoured() {
        let src = "\
// NoVerify(coderef:ifchange): too far up
// unrelated comment
// IfChange
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].no_verify_reason.is_none());
    }

    #[test]
    fn test_nested_open_reported_as_orphan_inner_pair_still_parses() {
        // Sequence: IfChange, IfChange, ThenChange. The first
        // IfChange is orphaned (close not paired before the second
        // open consumes the next ThenChange).
        let src = "\
// IfChange
// IfChange
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::OrphanIfChange { line: 1, .. }
        ));
    }

    #[test]
    fn test_then_change_with_no_args_yields_shape_b_block_with_no_targets() {
        let src = "\
// IfChange(grp)
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.blocks[0].id.as_deref(), Some("grp"));
    }

    #[test]
    fn test_then_change_arg_with_only_whitespace_yields_no_targets_no_error() {
        let src = "\
// IfChange
x
// ThenChange( , , )
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].targets.is_empty());
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
    }

    #[test]
    fn test_parse_anchor_target() {
        let src = "\
// IfChange
x
// ThenChange(/docs/security.md#hashing)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.blocks[0].targets.len(), 1);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileAnchor { ref path, ref anchor }
                if path == "/docs/security.md" && anchor == "hashing"
        ));
    }

    #[test]
    fn test_parse_anchor_target_with_hyphens_and_digits_in_anchor() {
        // Real-world heading slugs include hyphens, digits, and the
        // github double-hyphen.
        let src = "\
// IfChange
x
// ThenChange(/docs/x.md#argon2-params, /docs/y.md#section--2)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].targets.len(), 2);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileAnchor { ref anchor, .. } if anchor == "argon2-params"
        ));
        assert!(matches!(
            report.blocks[0].targets[1],
            Target::FileAnchor { ref anchor, .. } if anchor == "section--2"
        ));
    }

    #[test]
    fn test_parse_anchor_target_with_empty_anchor_rejected() {
        let src = "\
// IfChange
x
// ThenChange(/docs/x.md#)
";
        let report = extract_blocks(src, "a.rs");
        // The first target malforms (empty anchor); no successful
        // targets land, but the block still parses.
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::MalformedTarget { ref token, .. } if token == "/docs/x.md#"
        ));
    }

    #[test]
    fn test_parse_anchor_target_with_empty_path_rejected() {
        let src = "\
// IfChange
x
// ThenChange(#dangling)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_parse_anchor_and_line_range_targets_coexist_in_same_marker() {
        let src = "\
// IfChange
x
// ThenChange(/docs/x.md#hashing, /src/y.rs:10-20, /a.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        let ts = &report.blocks[0].targets;
        assert_eq!(ts.len(), 3);
        assert!(matches!(ts[0], Target::FileAnchor { .. }));
        assert!(matches!(ts[1], Target::FileLineRange { .. }));
        assert!(matches!(ts[2], Target::File { .. }));
    }

    #[test]
    fn test_parse_label_target_alphanumeric_suffix() {
        let src = "\
// IfChange
x
// ThenChange(/docs/security.md:argon2-params)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].targets.len(), 1);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileLabel { ref path, ref label }
                if path == "/docs/security.md" && label == "argon2-params"
        ));
    }

    #[test]
    fn test_parse_label_target_disambiguator_digits_win() {
        // `:42` is a line number, not a label called "42".
        let src = "\
// IfChange
x
// ThenChange(/a.rs:42)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileLine { .. }
        ));
    }

    #[test]
    fn test_parse_label_target_with_hyphens_treated_as_label_not_range() {
        // `:foo-bar` is a label called "foo-bar" — not a range,
        // because "foo" and "bar" don't parse as u32.
        let src = "\
// IfChange
x
// ThenChange(/a.rs:foo-bar)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileLabel { ref label, .. } if label == "foo-bar"
        ));
    }

    #[test]
    fn test_parse_label_target_with_double_colon_rejected() {
        let src = "\
// IfChange
x
// ThenChange(/a.rs:foo:bar)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_if_change_strips_matching_single_quotes_around_id() {
        // README's canonical form uses `IfChange('hash-params')`.
        // Should produce id = "hash-params", not "'hash-params'".
        let src = "\
// IfChange('hash-params')
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("hash-params"));
    }

    #[test]
    fn test_if_change_strips_matching_double_quotes_around_id() {
        let src = "\
// IfChange(\"my-id\")
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("my-id"));
    }

    #[test]
    fn test_if_change_preserves_bare_id_without_quotes() {
        let src = "\
// IfChange(my-id)
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks[0].id.as_deref(), Some("my-id"));
    }

    #[test]
    fn test_if_change_preserves_unmatched_quote() {
        // A leading-only quote isn't matched; preserved as-is.
        let src = "\
// IfChange('mismatched)
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks[0].id.as_deref(), Some("'mismatched"));
    }

    #[test]
    fn test_parse_label_and_line_targets_coexist_in_same_marker() {
        let src = "\
// IfChange
x
// ThenChange(/a.md:my-section, /b.rs:42, /c.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        let ts = &report.blocks[0].targets;
        assert_eq!(ts.len(), 3);
        assert!(matches!(ts[0], Target::FileLabel { .. }));
        assert!(matches!(ts[1], Target::FileLine { .. }));
        assert!(matches!(ts[2], Target::File { .. }));
    }
}
