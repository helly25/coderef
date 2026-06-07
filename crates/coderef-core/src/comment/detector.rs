//! Byte-range scanner that classifies non-body regions of a source
//! buffer for a given `Language`.
//!
//! Algorithm: a single forward pass with three states (`Code`,
//! `String`, `BlockComment`), recognising line comments at the top
//! level only. Each detected region is tagged with a `RegionKind`
//! (`BlockComment` / `LineComment` / `StringLiteral`) — markdown adds
//! `CodeSnippet` through its dedicated parser.

use super::languages::Language;
use super::region::{ClassifiedRange, RegionKind};

/// `[start, end)` byte range inside a buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    /// True iff the byte offset `pos` is contained in `[start, end)`.
    #[must_use]
    pub fn contains(&self, pos: usize) -> bool {
        pos >= self.start && pos < self.end
    }
}

/// Predicate over an ordered list of disjoint ranges.
#[must_use]
pub fn is_in_any_range(ranges: &[Range], pos: usize) -> bool {
    // Linear scan suffices at the call-site cadence (one check per
    // pattern match, ranges typically dozens per file). If profiling
    // shows it hot, switch to a binary search on the start offset.
    ranges.iter().any(|r| r.contains(pos))
}

/// Scan `content` for classified non-body regions under `lang`'s
/// syntax. Returns a list of disjoint, sorted ranges, each tagged
/// with a `RegionKind`.
///
/// Markdown is special-cased to the dedicated `super::markdown` parser
/// because correctly identifying `<!-- -->` comments + fenced /
/// inline code in markdown requires backtick / tilde awareness that
/// the generic state machine can't represent.
#[must_use]
pub fn detect_regions(content: &str, lang: &Language) -> Vec<ClassifiedRange> {
    if lang.name == "markdown" {
        return super::markdown::detect_markdown_regions(content);
    }
    let bytes = content.as_bytes();
    let mut regions: Vec<ClassifiedRange> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // 1) Block comment? Matched FIRST so a longer block-comment
        // opener (e.g. Lua `--[[`) wins over a shorter line-comment
        // opener (`--`) at the same position.
        if let Some((close, open_len)) = match_block_at(bytes, i, lang.block_comments) {
            let start = i;
            let body_start = i + open_len;
            let body_end = find_subsequence(bytes, close.as_bytes(), body_start)
                .map_or(bytes.len(), |pos| pos);
            let end = if body_end == bytes.len() {
                bytes.len()
            } else {
                body_end + close.len()
            };
            regions.push(ClassifiedRange {
                range: Range { start, end },
                kind: RegionKind::BlockComment,
            });
            i = end;
            continue;
        }

        // 2) Line comment?
        if let Some((_lc, lc_len)) = match_at(bytes, i, lang.line_comments) {
            let start = i;
            let end = find_byte_or_end(bytes, b'\n', i + lc_len);
            regions.push(ClassifiedRange {
                range: Range { start, end },
                kind: RegionKind::LineComment,
            });
            i = end;
            continue;
        }

        // 3) String literal? v0.2: classify, don't silently consume.
        // A reference inside a string (especially in test fixtures) is
        // still semantically a reference — see DESIGN.md §5.4.1 v0.2
        // notes on classified regions.
        if let Some((delim, delim_len)) = match_at(bytes, i, lang.string_delimiters) {
            let start = i;
            let close_search_start = i + delim_len;
            let end = find_string_close(bytes, delim.as_bytes(), close_search_start);
            regions.push(ClassifiedRange {
                range: Range { start, end },
                kind: RegionKind::StringLiteral,
            });
            i = end;
            continue;
        }

        // 4) Plain code byte; advance.
        i += utf8_char_len(bytes[i]);
    }
    regions
}

/// Returns `Some((matched_token, len))` if any of `tokens` is a prefix
/// of `bytes[at..]`. Picks the longest match (so `"""` beats `"`).
fn match_at<'a>(bytes: &[u8], at: usize, tokens: &'a [&'static str]) -> Option<(&'a str, usize)> {
    let mut best: Option<(&str, usize)> = None;
    for t in tokens {
        let tb = t.as_bytes();
        if bytes.len() - at >= tb.len() && &bytes[at..at + tb.len()] == tb {
            match best {
                Some((_, blen)) if blen >= tb.len() => {}
                _ => best = Some((t, tb.len())),
            }
        }
    }
    best
}

/// Like `match_at` but for block-comment open delimiters; returns the
/// matching close delimiter alongside the open length.
fn match_block_at(
    bytes: &[u8],
    at: usize,
    pairs: &'static [(&'static str, &'static str)],
) -> Option<(&'static str, usize)> {
    let mut best: Option<(&'static str, usize)> = None;
    for (open, close) in pairs {
        let ob = open.as_bytes();
        if bytes.len() - at >= ob.len() && &bytes[at..at + ob.len()] == ob {
            match best {
                Some((_, blen)) if blen >= ob.len() => {}
                _ => best = Some((close, ob.len())),
            }
        }
    }
    best
}

fn find_byte_or_end(bytes: &[u8], target: u8, from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|b| *b == target)
        .map_or(bytes.len(), |p| from + p)
}

fn find_subsequence(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < from + needle.len() {
        return None;
    }
    (from..=haystack.len().saturating_sub(needle.len()))
        .find(|&i| &haystack[i..i + needle.len()] == needle)
}

/// Find the closing delimiter for a string literal opened at `from`,
/// respecting backslash escapes. Returns one past the closing delimiter,
/// or the end of input if unterminated.
fn find_string_close(bytes: &[u8], delim: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            // Escape sequence; skip the next byte.
            i += 2;
            continue;
        }
        if bytes.len() - i >= delim.len() && &bytes[i..i + delim.len()] == delim {
            return i + delim.len();
        }
        i += utf8_char_len(bytes[i]);
    }
    bytes.len()
}

const fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comment::languages::language_for_extension;

    fn regions_for(ext: &str, content: &str) -> Vec<ClassifiedRange> {
        detect_regions(content, language_for_extension(ext).unwrap())
    }

    fn slices_of<'a>(regions: &[ClassifiedRange], content: &'a str) -> Vec<&'a str> {
        regions
            .iter()
            .map(|r| &content[r.range.start..r.range.end])
            .collect()
    }

    fn kinds_of(regions: &[ClassifiedRange]) -> Vec<RegionKind> {
        regions.iter().map(|r| r.kind).collect()
    }

    // ---------- Rust / C-family ----------

    #[test]
    fn test_detect_rust_line_comment_classified_as_line_comment() {
        let content = "let x = 1; // tail comment\nlet y = 2;";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["// tail comment"]);
        assert_eq!(kinds_of(&r), vec![RegionKind::LineComment]);
    }

    #[test]
    fn test_detect_rust_line_comment_at_eof_has_no_trailing_newline() {
        let content = "let x = 1; // no trailing newline";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["// no trailing newline"]);
    }

    #[test]
    fn test_detect_rust_block_comment_classified_as_block_comment() {
        let content = "fn x() /* block */ {}";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["/* block */"]);
        assert_eq!(kinds_of(&r), vec![RegionKind::BlockComment]);
    }

    #[test]
    fn test_detect_rust_block_comment_multi_line() {
        let content = "/* line 1\nline 2\nline 3 */ fn x() {}";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["/* line 1\nline 2\nline 3 */"]);
    }

    #[test]
    fn test_detect_rust_string_literal_classified_as_string_literal() {
        // v0.2 change: strings emit a StringLiteral region (in v0.1
        // they were silently consumed). The `//` literal inside the
        // string still doesn't open a LineComment, but the string
        // itself is now a region.
        let content = "let s = \"// not a comment\";";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["\"// not a comment\""]);
        assert_eq!(kinds_of(&r), vec![RegionKind::StringLiteral]);
    }

    #[test]
    fn test_detect_rust_block_inside_string_classified_as_string_only() {
        let content = "let s = \"/* not a comment */\";";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["\"/* not a comment */\""]);
        assert_eq!(kinds_of(&r), vec![RegionKind::StringLiteral]);
    }

    #[test]
    fn test_detect_rust_unterminated_block_extends_to_eof() {
        let content = "code; /* unterminated";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["/* unterminated"]);
    }

    #[test]
    fn test_detect_rust_string_then_real_comment_classifies_each_correctly() {
        let content = "let s = \"a\\\"b // c\"; let t = 1; // real comment";
        let r = regions_for("rs", content);
        assert_eq!(
            kinds_of(&r),
            vec![RegionKind::StringLiteral, RegionKind::LineComment]
        );
        assert_eq!(slices_of(&r, content)[1], "// real comment");
    }

    // ---------- Python ----------

    #[test]
    fn test_detect_python_hash_line_comment() {
        let content = "x = 1  # tail\ny = 2";
        let r = regions_for("py", content);
        assert_eq!(slices_of(&r, content), vec!["# tail"]);
        assert_eq!(kinds_of(&r), vec![RegionKind::LineComment]);
    }

    #[test]
    fn test_detect_python_hash_inside_string_classifies_string_separately() {
        let content = "s = \"# not a comment\"\n# real comment";
        let r = regions_for("py", content);
        assert_eq!(
            kinds_of(&r),
            vec![RegionKind::StringLiteral, RegionKind::LineComment]
        );
    }

    #[test]
    fn test_detect_python_triple_quoted_string_is_a_single_string_literal() {
        let content = "x = \"\"\"docstring\n# not a comment\n\"\"\"\n# real comment";
        let r = regions_for("py", content);
        // One StringLiteral (the triple-quoted block) followed by one
        // LineComment (`# real comment`).
        assert_eq!(
            kinds_of(&r),
            vec![RegionKind::StringLiteral, RegionKind::LineComment]
        );
        assert!(slices_of(&r, content)[0].starts_with("\"\"\""));
    }

    // ---------- Lua ----------

    #[test]
    fn test_detect_lua_dash_line_comment() {
        let content = "local x = 1 -- tail\n";
        let r = regions_for("lua", content);
        assert_eq!(slices_of(&r, content), vec!["-- tail"]);
        assert_eq!(kinds_of(&r), vec![RegionKind::LineComment]);
    }

    #[test]
    fn test_detect_lua_block_comment() {
        let content = "x = 1 --[[ block\nspans ]] y = 2";
        let r = regions_for("lua", content);
        assert_eq!(slices_of(&r, content), vec!["--[[ block\nspans ]]"]);
        assert_eq!(kinds_of(&r), vec![RegionKind::BlockComment]);
    }

    // ---------- Markdown ----------
    //
    // The full markdown parser lives in super::markdown. The two
    // tests here just verify the detector's dispatch to that parser
    // is wired up — comprehensive coverage of fenced blocks + inline
    // code + comment cases is in markdown.rs's own tests.

    #[test]
    fn test_detect_markdown_dispatches_to_fenced_block_aware_parser() {
        let content = "# Title\n<!-- hidden -->\nbody";
        let r = regions_for("md", content);
        assert_eq!(slices_of(&r, content), vec!["<!-- hidden -->"]);
        assert_eq!(kinds_of(&r), vec![RegionKind::BlockComment]);
    }

    #[test]
    fn test_detect_markdown_inline_code_classified_as_code_snippet() {
        // v0.2 change: backtick-wrapped content emits a CodeSnippet
        // region (in v0.1 it was protected and silently dropped).
        // References inside it survive `commentsOnly: true` via the
        // new is_comment_like predicate.
        let content = "See `JIRA(PROJ-1)` for the canonical example.";
        let r = regions_for("md", content);
        assert_eq!(kinds_of(&r), vec![RegionKind::CodeSnippet]);
        assert!(slices_of(&r, content)[0].contains("JIRA"));
    }

    // ---------- Range utilities (unchanged) ----------

    #[test]
    fn test_range_contains_inclusive_start_exclusive_end() {
        let r = Range { start: 10, end: 20 };
        assert!(r.contains(10));
        assert!(r.contains(19));
        assert!(!r.contains(20));
        assert!(!r.contains(9));
    }

    #[test]
    fn test_is_in_any_range_true_when_position_in_one_range() {
        let ranges = vec![Range { start: 0, end: 5 }, Range { start: 10, end: 20 }];
        assert!(is_in_any_range(&ranges, 12));
    }

    #[test]
    fn test_is_in_any_range_false_when_position_in_no_range() {
        let ranges = vec![Range { start: 0, end: 5 }, Range { start: 10, end: 20 }];
        assert!(!is_in_any_range(&ranges, 7));
    }

    #[test]
    fn test_is_in_any_range_false_for_empty_range_list() {
        assert!(!is_in_any_range(&[], 0));
    }

    // ---------- UTF-8 ----------

    #[test]
    fn test_detect_handles_multibyte_utf8_string_alongside_line_comment() {
        let content = "let s = \"Müller\"; // tail";
        let r = regions_for("rs", content);
        assert_eq!(
            kinds_of(&r),
            vec![RegionKind::StringLiteral, RegionKind::LineComment]
        );
        assert_eq!(slices_of(&r, content)[1], "// tail");
    }

    #[test]
    fn test_detect_multiple_comments_classified_correctly() {
        let content = "a // c1\nb /* c2 */ c\nd // c3";
        let r = regions_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["// c1", "/* c2 */", "// c3"]);
        assert_eq!(
            kinds_of(&r),
            vec![
                RegionKind::LineComment,
                RegionKind::BlockComment,
                RegionKind::LineComment
            ]
        );
    }
}
