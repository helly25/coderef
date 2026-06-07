//! Byte-range scanner that identifies comment regions in a source
//! buffer for a given `Language`.
//!
//! Algorithm: a single forward pass with three states (`Code`,
//! `String`, `BlockComment`), recognising line comments at the top
//! level only.

use super::languages::Language;

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

/// Scan `content` for comment regions under `lang`'s syntax. Returns a
/// list of disjoint, sorted ranges.
#[must_use]
pub fn detect_comment_ranges(content: &str, lang: &Language) -> Vec<Range> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
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
            ranges.push(Range { start, end });
            i = end;
            continue;
        }

        // 2) Line comment?
        if let Some((_lc, lc_len)) = match_at(bytes, i, lang.line_comments) {
            let start = i;
            let end = find_byte_or_end(bytes, b'\n', i + lc_len);
            ranges.push(Range { start, end });
            i = end;
            continue;
        }

        // 3) String literal? Skip past it without recording.
        if let Some((delim, delim_len)) = match_at(bytes, i, lang.string_delimiters) {
            let close_search_start = i + delim_len;
            let close = find_string_close(bytes, delim.as_bytes(), close_search_start);
            i = close;
            continue;
        }

        // 4) Plain code byte; advance.
        i += utf8_char_len(bytes[i]);
    }
    ranges
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

    fn ranges_for(ext: &str, content: &str) -> Vec<Range> {
        detect_comment_ranges(content, language_for_extension(ext).unwrap())
    }

    fn slices_of<'a>(ranges: &[Range], content: &'a str) -> Vec<&'a str> {
        ranges.iter().map(|r| &content[r.start..r.end]).collect()
    }

    // ---------- Rust / C-family ----------

    #[test]
    fn test_detect_rust_line_comment_to_end_of_line() {
        let content = "let x = 1; // tail comment\nlet y = 2;";
        let r = ranges_for("rs", content);
        assert_eq!(r.len(), 1);
        assert_eq!(slices_of(&r, content), vec!["// tail comment"]);
    }

    #[test]
    fn test_detect_rust_line_comment_at_eof_has_no_trailing_newline() {
        let content = "let x = 1; // no trailing newline";
        let r = ranges_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["// no trailing newline"]);
    }

    #[test]
    fn test_detect_rust_block_comment_single_line() {
        let content = "fn x() /* block */ {}";
        let r = ranges_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["/* block */"]);
    }

    #[test]
    fn test_detect_rust_block_comment_multi_line() {
        let content = "/* line 1\nline 2\nline 3 */ fn x() {}";
        let r = ranges_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["/* line 1\nline 2\nline 3 */"]);
    }

    #[test]
    fn test_detect_rust_string_literal_not_a_comment() {
        let content = "let s = \"// not a comment\";";
        let r = ranges_for("rs", content);
        assert!(r.is_empty());
    }

    #[test]
    fn test_detect_rust_block_inside_string_not_a_comment() {
        let content = "let s = \"/* not a comment */\";";
        let r = ranges_for("rs", content);
        assert!(r.is_empty());
    }

    #[test]
    fn test_detect_rust_unterminated_block_extends_to_eof() {
        let content = "code; /* unterminated";
        let r = ranges_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["/* unterminated"]);
    }

    #[test]
    fn test_detect_rust_backslash_escape_inside_string() {
        // \" is an escaped quote; the string isn't terminated until the
        // unescaped ". The // inside is still inside a string, not a
        // comment.
        let content = "let s = \"a\\\"b // c\"; let t = 1; // real comment";
        let r = ranges_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["// real comment"]);
    }

    // ---------- Python ----------

    #[test]
    fn test_detect_python_hash_line_comment() {
        let content = "x = 1  # tail\ny = 2";
        let r = ranges_for("py", content);
        assert_eq!(slices_of(&r, content), vec!["# tail"]);
    }

    #[test]
    fn test_detect_python_hash_inside_string_not_a_comment() {
        let content = "s = \"# not a comment\"\n# real comment";
        let r = ranges_for("py", content);
        assert_eq!(slices_of(&r, content), vec!["# real comment"]);
    }

    #[test]
    fn test_detect_python_triple_quoted_string_swallows_hashes_inside() {
        let content = "x = \"\"\"docstring\n# not a comment\n\"\"\"\n# real comment";
        let r = ranges_for("py", content);
        assert_eq!(slices_of(&r, content), vec!["# real comment"]);
    }

    // ---------- Lua ----------

    #[test]
    fn test_detect_lua_dash_line_comment() {
        let content = "local x = 1 -- tail\n";
        let r = ranges_for("lua", content);
        assert_eq!(slices_of(&r, content), vec!["-- tail"]);
    }

    #[test]
    fn test_detect_lua_block_comment() {
        let content = "x = 1 --[[ block\nspans ]] y = 2";
        let r = ranges_for("lua", content);
        assert_eq!(slices_of(&r, content), vec!["--[[ block\nspans ]]"]);
    }

    // ---------- Markdown ----------

    #[test]
    fn test_detect_markdown_yields_no_comment_ranges_in_v0_1() {
        // v0.1 markdown has no comment delimiters (see comment/languages.rs
        // MARKDOWN). The detector must not flag <!-- --> ranges; otherwise
        // a literal <!-- inside a fenced code block in DESIGN.md / any
        // README opens a spurious comment range that swallows the rest
        // of the doc until the next -->.
        let content = "# Title\n<!-- hidden -->\nbody";
        let r = ranges_for("md", content);
        assert!(r.is_empty(), "got: {:?}", slices_of(&r, content));
    }

    #[test]
    fn test_detect_markdown_with_backticked_html_comment_does_not_open_range() {
        // The regression case: literal <!-- inside a code-fence literal,
        // followed much later by another --> elsewhere. v0.1's empty
        // markdown comment-set means neither of these is treated as a
        // comment opener / closer, which is the safe v0.1 default.
        let content = "Doc says `<!--` is the opener.\n\nLater: `-->` closes.";
        let r = ranges_for("md", content);
        assert!(r.is_empty(), "got: {:?}", slices_of(&r, content));
    }

    // ---------- Range utilities ----------

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
    fn test_detect_handles_multibyte_utf8_in_string_literal() {
        let content = "let s = \"Müller\"; // tail";
        let r = ranges_for("rs", content);
        assert_eq!(slices_of(&r, content), vec!["// tail"]);
    }

    #[test]
    fn test_detect_multiple_comments_in_one_file() {
        let content = "a // c1\nb /* c2 */ c\nd // c3";
        let r = ranges_for("rs", content);
        let slices = slices_of(&r, content);
        assert_eq!(slices, vec!["// c1", "/* c2 */", "// c3"]);
    }
}
