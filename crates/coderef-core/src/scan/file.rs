//! Per-file scanner. Pure-function over `&str` + `&[CompiledPattern]`.

use indexmap::IndexMap;
use thiserror::Error;

use crate::comment::{detect_comment_ranges, is_in_any_range, Language, Range};
use crate::config::Pattern;
use crate::pattern::{CompiledPattern, PatternError};
use crate::reference::Reference;
use crate::variables::{Context, VariableError};

/// Inputs to a single-file scan.
pub struct ScanOptions<'a> {
    /// Patterns compiled by `CompiledPattern::compile`. Borrowed; the
    /// caller manages their lifetime.
    pub patterns: &'a [(CompiledPattern, Pattern)],

    /// Language descriptor for comment-region detection. `None` means
    /// no comment regions; matches against `scope.commentsOnly: true`
    /// patterns will be filtered out (i.e. the pattern matches nothing
    /// in unknown languages, which is the safe default).
    pub language: Option<&'a Language>,

    /// Variable resolution context. Captures will be inserted per-match
    /// by the scanner; the caller is responsible for seeding builtins,
    /// config variables, file context, env, etc.
    pub base_context: &'a Context<'a>,

    /// File path label embedded in each returned `Reference`. The
    /// scanner does no path manipulation; the caller chooses absolute
    /// or workspace-relative.
    pub file: &'a str,
}

/// Run all patterns against `content`. Returns the references sorted by
/// `(byte_start, pattern_id)` for determinism.
///
/// Error semantics: a per-pattern regex error during iteration aborts
/// the scan. Variable-resolution errors for a *single* match are
/// surfaced as `ScanError::ResolveFailure` so the caller can map them
/// to a diagnostic without losing the other references in the file.
pub fn scan_file(content: &str, opts: &ScanOptions) -> Result<Vec<Reference>, ScanError> {
    let comment_ranges: Vec<Range> = opts
        .language
        .map(|l| detect_comment_ranges(content, l))
        .unwrap_or_default();

    let line_offsets = compute_line_offsets(content);

    let mut refs = Vec::new();
    for (compiled, raw) in opts.patterns {
        let comments_only = raw.scope.as_ref().is_some_and(|s| s.comments_only);

        let mut start = 0;
        while let Some(captures) =
            compiled
                .regex
                .captures_from_pos(content, start)
                .map_err(|e| ScanError::RegexExecution {
                    pattern_id: compiled.id.clone(),
                    message: e.to_string(),
                })?
        {
            let m = captures.get(0).expect("capture group 0 always present");
            let match_start = m.start();
            let match_end = m.end();

            // Advance `start` for next iteration. Empty-match safety:
            // step at least one byte forward.
            start = if match_end > match_start {
                match_end
            } else {
                match_end + 1
            };

            // Filter by commentsOnly scope.
            if comments_only && !is_in_any_range(&comment_ranges, match_start) {
                continue;
            }

            // Extract named captures.
            let mut caps_map = IndexMap::new();
            for name in compiled.regex.capture_names().flatten() {
                if let Some(c) = captures.name(name) {
                    caps_map.insert(name.to_string(), c.as_str().to_string());
                }
            }

            // Build per-match context (base + captures).
            let mut ctx = opts.base_context.clone();
            for (k, v) in &caps_map {
                ctx = ctx.with_capture(k.clone(), v.clone());
            }

            let target =
                compiled
                    .resolve_target(&ctx)
                    .map_err(|source| ScanError::ResolveFailure {
                        pattern_id: compiled.id.clone(),
                        file: opts.file.to_string(),
                        byte_start: match_start,
                        source,
                    })?;
            let title =
                compiled
                    .resolve_title(&ctx)
                    .map_err(|source| ScanError::ResolveFailure {
                        pattern_id: compiled.id.clone(),
                        file: opts.file.to_string(),
                        byte_start: match_start,
                        source,
                    })?;

            let (line, column) = byte_to_line_col(&line_offsets, match_start, content);

            refs.push(Reference {
                pattern_id: compiled.id.clone(),
                pattern_kind: compiled.kind,
                file: opts.file.to_string(),
                line,
                column,
                byte_start: match_start,
                byte_end: match_end,
                matched_text: m.as_str().to_string(),
                captures: caps_map,
                target,
                title,
                in_comment: is_in_any_range(&comment_ranges, match_start),
            });
        }
    }

    refs.sort_by(|a, b| {
        a.byte_start
            .cmp(&b.byte_start)
            .then_with(|| a.pattern_id.cmp(&b.pattern_id))
    });
    Ok(refs)
}

/// Failures from `scan_file`.
#[derive(Debug, Error)]
pub enum ScanError {
    /// Pattern compilation failure (caller didn't compile beforehand).
    #[error(transparent)]
    Pattern(#[from] PatternError),

    /// `fancy-regex` failed mid-iteration. Rare in practice but the
    /// engine can return errors for catastrophic backtracking.
    #[error("pattern `{pattern_id}` failed during regex execution: {message}")]
    RegexExecution { pattern_id: String, message: String },

    /// Variable resolution failed for a single match.
    #[error(
        "pattern `{pattern_id}` at {file}:byte-{byte_start} failed to resolve target: {source}"
    )]
    ResolveFailure {
        pattern_id: String,
        file: String,
        byte_start: usize,
        #[source]
        source: VariableError,
    },
}

fn compute_line_offsets(content: &str) -> Vec<usize> {
    let mut offsets = vec![0_usize];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn byte_to_line_col(line_offsets: &[usize], byte_pos: usize, content: &str) -> (u32, u32) {
    let line_idx = match line_offsets.binary_search(&byte_pos) {
        Ok(i) => i,
        Err(i) => i - 1,
    };
    let line_start = line_offsets[line_idx];
    let col_bytes = &content.as_bytes()[line_start..byte_pos];
    let col = match std::str::from_utf8(col_bytes) {
        Ok(s) => s.chars().count(),
        Err(_) => byte_pos - line_start,
    };
    (
        u32::try_from(line_idx + 1).unwrap_or(u32::MAX),
        u32::try_from(col + 1).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comment::language_for_extension;
    use crate::config::Pattern;
    use crate::pattern::CompiledPattern;

    fn p(id: &str, regex: &str, target: &str, comments_only: bool) -> (CompiledPattern, Pattern) {
        let raw = Pattern {
            regex: regex.into(),
            target: Some(target.into()),
            scope: if comments_only {
                Some(crate::config::ScopeConfig {
                    comments_only: true,
                    ..Default::default()
                })
            } else {
                None
            },
            ..Default::default()
        };
        let compiled = CompiledPattern::compile(id, &raw).unwrap();
        (compiled, raw)
    }

    fn scan(content: &str, ext: &str, patterns: &[(CompiledPattern, Pattern)]) -> Vec<Reference> {
        let ctx = Context::new();
        let opts = ScanOptions {
            patterns,
            language: language_for_extension(ext),
            base_context: &ctx,
            file: "test.rs",
        };
        scan_file(content, &opts).unwrap()
    }

    #[test]
    fn test_scan_finds_single_match_in_plain_text() {
        let pats = vec![p(
            "todo",
            r"TODO\(@(?<user>\w+)\)",
            "https://x/${user}",
            false,
        )];
        let refs = scan("# TODO(@marcus) — fix it", "rs", &pats);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].pattern_id, "todo");
        assert_eq!(refs[0].matched_text, "TODO(@marcus)");
        assert_eq!(refs[0].target, "https://x/marcus");
        assert_eq!(refs[0].line, 1);
        assert_eq!(
            refs[0].captures.get("user").map(String::as_str),
            Some("marcus")
        );
    }

    #[test]
    fn test_scan_finds_multiple_matches_for_same_pattern() {
        let pats = vec![p("todo", r"TODO\(@(?<user>\w+)\)", "x/${user}", false)];
        let content = "TODO(@a) and TODO(@b)";
        let refs = scan(content, "rs", &pats);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].captures["user"], "a");
        assert_eq!(refs[1].captures["user"], "b");
    }

    #[test]
    fn test_scan_orders_matches_by_byte_start() {
        let pats = vec![p("a", r"AAA", "x/a", false), p("b", r"BBB", "x/b", false)];
        let content = "x BBB y AAA z";
        let refs = scan(content, "rs", &pats);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].pattern_id, "b");
        assert_eq!(refs[1].pattern_id, "a");
    }

    #[test]
    fn test_scan_orders_simultaneous_matches_by_pattern_id() {
        let pats = vec![p("zzz", r"X", "x/z", false), p("aaa", r"X", "x/a", false)];
        let refs = scan("X", "rs", &pats);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].pattern_id, "aaa");
        assert_eq!(refs[1].pattern_id, "zzz");
    }

    #[test]
    fn test_scan_comments_only_filters_out_code_matches() {
        let pats = vec![p(
            "todo",
            r"TODO\(@(?<user>\w+)\)",
            "x/${user}",
            true, // commentsOnly
        )];
        let content = "let x = TODO(@code); // TODO(@comment)";
        let refs = scan(content, "rs", &pats);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].captures["user"], "comment");
    }

    #[test]
    fn test_scan_comments_only_in_unknown_language_filters_all() {
        let pats = vec![p("todo", r"TODO\(@(?<user>\w+)\)", "x/${user}", true)];
        let content = "TODO(@x)";
        let ctx = Context::new();
        let opts = ScanOptions {
            patterns: &pats,
            language: None,
            base_context: &ctx,
            file: "x.unknown",
        };
        let refs = scan_file(content, &opts).unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn test_scan_reports_line_and_column_correctly_after_newlines() {
        let pats = vec![p("x", r"X", "u/x", false)];
        let content = "ab\ncd\n  X here";
        let refs = scan(content, "rs", &pats);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].line, 3);
        assert_eq!(refs[0].column, 3);
    }

    #[test]
    fn test_scan_sets_in_comment_flag_per_match() {
        let pats = vec![p("x", r"X", "u/x", false)];
        let content = "X // X";
        let refs = scan(content, "rs", &pats);
        assert_eq!(refs.len(), 2);
        assert!(!refs[0].in_comment);
        assert!(refs[1].in_comment);
    }

    #[test]
    fn test_scan_handles_multibyte_utf8_column_count() {
        let pats = vec![p("x", r"X", "u/x", false)];
        let content = "Müller X"; // 'ü' is two bytes, one char.
        let refs = scan(content, "rs", &pats);
        // Column is 1-indexed; 'M','ü','l','l','e','r',' ' → X at col 8.
        assert_eq!(refs[0].column, 8);
    }

    #[test]
    fn test_scan_returns_empty_when_no_patterns_match() {
        let pats = vec![p("nope", r"NEVER", "u/n", false)];
        let refs = scan("nothing here", "rs", &pats);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_scan_resolve_failure_surfaces_pattern_id_and_byte_offset() {
        // Target references an unresolved capture that isn't in the
        // regex's named captures; strict-mode resolution errors.
        let raw = Pattern {
            regex: "X".into(),
            target: Some("u/${notACapture}".into()),
            ..Default::default()
        };
        let compiled = CompiledPattern::compile("bad", &raw).unwrap();
        let pats = vec![(compiled, raw)];
        let ctx = Context::new();
        let opts = ScanOptions {
            patterns: &pats,
            language: language_for_extension("rs"),
            base_context: &ctx,
            file: "test.rs",
        };
        let err = scan_file("X", &opts).unwrap_err();
        match err {
            ScanError::ResolveFailure {
                pattern_id,
                byte_start,
                ..
            } => {
                assert_eq!(pattern_id, "bad");
                assert_eq!(byte_start, 0);
            }
            other => panic!("expected ResolveFailure, got {other:?}"),
        }
    }
}
