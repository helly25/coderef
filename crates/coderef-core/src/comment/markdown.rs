//! Markdown-specific comment-region detection.
//!
//! Markdown's only comment delimiter is the HTML `<!-- -->` block, but
//! detecting it correctly is genuinely harder than treating it as a
//! generic block comment, because `<!--` literals routinely appear:
//!
//!   - inside fenced code blocks (` ```...``` ` or ` ~~~...~~~ `)
//!   - inside inline code spans (`` `<!--` ``)
//!   - inside indented (4-space) code blocks — we ignore these for v0.2
//!     because they're rare in modern markdown and adding the support
//!     means walking every line and tracking the previous blank-line
//!     state. Fenced blocks cover ~99% of real-world cases.
//!
//! Algorithm:
//!   1. Single forward scan that finds every protected region — fenced
//!      code blocks and inline code spans — collecting their byte
//!      ranges. Within a protected region nothing else is interpreted.
//!   2. Within the same scan, when not inside any protected region,
//!      recognise `<!-- ... -->` block comments and add them to the
//!      output. An unterminated `<!--` extends to EOF (matching the
//!      generic detector's posture).
//!
//! Trade-off: this is one forward pass over the buffer with at most a
//! few small allocations. `CommonMark`'s full backtracking rules around
//! delimiter runs aren't reproduced — e.g. ``"`` (double-backtick) is
//! treated as a 2-backtick inline-code span opener; mismatched runs
//! become unterminated regions that extend to EOF. That matches the
//! v0.1 posture (unterminated comments also extend to EOF) and is
//! good enough for the `scope.commentsOnly` use case.

use super::detector::Range;

/// Detect comment regions in a markdown buffer.
pub fn detect_markdown_comment_ranges(content: &str) -> Vec<Range> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        // Fenced code block? Only at the start of a line (after optional
        // whitespace, per CommonMark spec — we accept any whitespace
        // leading on the same line that the fence starts).
        if at_line_start(bytes, i) {
            if let Some((fence_char, count, fence_open_end)) = match_fence_open(bytes, i) {
                // Range covers from the fence's first char through (and
                // including) the closing fence line — or to EOF if
                // unterminated.
                let end = find_fence_close(bytes, fence_open_end, fence_char, count).unwrap_or(len);
                i = end;
                continue;
            }
        }

        // Inline code span? Any run of N backticks (N >= 1) on a line.
        if bytes[i] == b'`' {
            let run = count_backticks(bytes, i);
            let close = find_inline_code_close(bytes, i + run, run).unwrap_or(len);
            i = close;
            continue;
        }

        // HTML block comment? `<!--`
        if starts_with(bytes, i, b"<!--") {
            let start = i;
            let body_start = i + 4;
            let body_end = find_subsequence(bytes, b"-->", body_start).map_or(len, |pos| pos + 3);
            ranges.push(Range {
                start,
                end: body_end,
            });
            i = body_end;
            continue;
        }

        i += utf8_char_len(bytes[i]);
    }
    ranges
}

/// Returns true if the byte index `at` is at the start of a line
/// (preceded only by horizontal whitespace since the previous `\n`).
fn at_line_start(bytes: &[u8], at: usize) -> bool {
    let mut j = at;
    while j > 0 {
        let b = bytes[j - 1];
        if b == b'\n' {
            return true;
        }
        if b == b' ' || b == b'\t' {
            j -= 1;
            continue;
        }
        return false;
    }
    true // at == 0 or only whitespace from BOF
}

/// If `at` (which must be a line start by `at_line_start`) is the
/// beginning of a fence (3+ backticks or 3+ tildes), return
/// `(fence_char, count, end-of-opening-line)`. The opening line is
/// taken to extend to the next `\n` (exclusive) or EOF.
fn match_fence_open(bytes: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    // Skip leading horizontal whitespace.
    let mut j = at;
    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
        j += 1;
    }
    if j >= bytes.len() {
        return None;
    }
    let fence_char = bytes[j];
    if fence_char != b'`' && fence_char != b'~' {
        return None;
    }
    let mut k = j;
    while k < bytes.len() && bytes[k] == fence_char {
        k += 1;
    }
    let count = k - j;
    if count < 3 {
        return None;
    }
    // Skip the rest of the line — the "info string" (language tag etc).
    let mut line_end = k;
    while line_end < bytes.len() && bytes[line_end] != b'\n' {
        line_end += 1;
    }
    if line_end < bytes.len() {
        line_end += 1; // consume the newline so the search starts on a fresh line.
    }
    Some((fence_char, count, line_end))
}

/// Find the closing fence after `from`. The closing fence is a line
/// (after horizontal whitespace) of the same `fence_char` repeated
/// `>= count` times, followed by optional whitespace + newline (or
/// EOF). Returns the byte offset one past the closing line.
fn find_fence_close(bytes: &[u8], from: usize, fence_char: u8, count: usize) -> Option<usize> {
    let mut line_start = from;
    while line_start < bytes.len() {
        let mut j = line_start;
        // Skip horizontal whitespace.
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        let mut k = j;
        while k < bytes.len() && bytes[k] == fence_char {
            k += 1;
        }
        if k - j >= count {
            // Closing fence found. Skip rest of line + newline.
            let mut line_end = k;
            while line_end < bytes.len() && bytes[line_end] != b'\n' {
                line_end += 1;
            }
            if line_end < bytes.len() {
                line_end += 1;
            }
            return Some(line_end);
        }
        // Advance to the next line.
        while line_start < bytes.len() && bytes[line_start] != b'\n' {
            line_start += 1;
        }
        if line_start < bytes.len() {
            line_start += 1;
        }
    }
    None
}

/// Count consecutive backticks starting at `at`.
fn count_backticks(bytes: &[u8], at: usize) -> usize {
    let mut k = at;
    while k < bytes.len() && bytes[k] == b'`' {
        k += 1;
    }
    k - at
}

/// Find a closing run of exactly `count` backticks (preceded and
/// followed by non-backticks, per `CommonMark`). Returns the byte
/// offset one past the closing run.
fn find_inline_code_close(bytes: &[u8], from: usize, count: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] != b'`' {
            j += 1;
            continue;
        }
        let run_start = j;
        while j < bytes.len() && bytes[j] == b'`' {
            j += 1;
        }
        if j - run_start == count {
            return Some(j);
        }
        // Different-length run; not a match. Keep scanning.
    }
    None
}

fn starts_with(bytes: &[u8], at: usize, needle: &[u8]) -> bool {
    bytes.len() - at >= needle.len() && &bytes[at..at + needle.len()] == needle
}

fn find_subsequence(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < from + needle.len() {
        return None;
    }
    (from..=haystack.len().saturating_sub(needle.len()))
        .find(|&i| &haystack[i..i + needle.len()] == needle)
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

    fn slices<'a>(ranges: &[Range], content: &'a str) -> Vec<&'a str> {
        ranges.iter().map(|r| &content[r.start..r.end]).collect()
    }

    #[test]
    fn test_markdown_html_block_comment_in_body_text_is_detected() {
        let content = "# Title\n<!-- hidden -->\nbody";
        let r = detect_markdown_comment_ranges(content);
        assert_eq!(slices(&r, content), vec!["<!-- hidden -->"]);
    }

    #[test]
    fn test_markdown_html_comment_inside_inline_code_is_not_detected() {
        // The regression case from PR #7: a `<!--` literal inside an
        // inline-code span must NOT be treated as a comment opener.
        let content = "Doc says `<!--` is the opener.";
        let r = detect_markdown_comment_ranges(content);
        assert!(r.is_empty(), "got: {:?}", slices(&r, content));
    }

    #[test]
    fn test_markdown_html_comment_inside_fenced_code_block_is_not_detected() {
        let content = "intro\n\n```\n<!-- this is example markup -->\n```\n\nbody";
        let r = detect_markdown_comment_ranges(content);
        assert!(r.is_empty(), "got: {:?}", slices(&r, content));
    }

    #[test]
    fn test_markdown_html_comment_outside_after_fenced_block_is_detected() {
        let content = "```\n<!-- inside -->\n```\n<!-- real -->";
        let r = detect_markdown_comment_ranges(content);
        let s = slices(&r, content);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0], "<!-- real -->");
    }

    #[test]
    fn test_markdown_tilde_fenced_code_block_also_protects() {
        let content = "~~~\n<!-- escaped -->\n~~~\n<!-- real -->";
        let r = detect_markdown_comment_ranges(content);
        let s = slices(&r, content);
        assert_eq!(s, vec!["<!-- real -->"]);
    }

    #[test]
    fn test_markdown_fence_with_info_string_still_protects() {
        let content = "```html\n<!-- example -->\n```\n<!-- real -->";
        let r = detect_markdown_comment_ranges(content);
        let s = slices(&r, content);
        assert_eq!(s, vec!["<!-- real -->"]);
    }

    #[test]
    fn test_markdown_longer_fence_can_be_closed_only_by_at_least_same_length() {
        // 4 backticks opens; 3 backticks does NOT close.
        let content = "````\nbody with ``` inside\n````\n<!-- real -->";
        let r = detect_markdown_comment_ranges(content);
        assert_eq!(slices(&r, content), vec!["<!-- real -->"]);
    }

    #[test]
    fn test_markdown_double_backtick_inline_code_protects_single_backticks_inside() {
        // `` `code with literal backtick` `` — the outer `` opens an
        // inline code span; the inner single backticks are literals.
        let content = "intro ``a `b` c`` <!-- real -->";
        let r = detect_markdown_comment_ranges(content);
        assert_eq!(slices(&r, content), vec!["<!-- real -->"]);
    }

    #[test]
    fn test_markdown_unterminated_block_comment_extends_to_eof() {
        let content = "lead\n<!-- never closes";
        let r = detect_markdown_comment_ranges(content);
        assert_eq!(slices(&r, content), vec!["<!-- never closes"]);
    }

    #[test]
    fn test_markdown_unterminated_fence_protects_to_eof() {
        let content = "intro\n```\n<!-- still inside -->\nno close here";
        let r = detect_markdown_comment_ranges(content);
        assert!(r.is_empty(), "got: {:?}", slices(&r, content));
    }

    #[test]
    fn test_markdown_leading_whitespace_before_fence_is_accepted() {
        let content = "intro\n  ```\n<!-- escaped -->\n  ```\n<!-- real -->";
        let r = detect_markdown_comment_ranges(content);
        assert_eq!(slices(&r, content), vec!["<!-- real -->"]);
    }

    #[test]
    fn test_markdown_multiple_html_comments_in_body() {
        let content = "<!-- one -->\nbody\n<!-- two -->";
        let r = detect_markdown_comment_ranges(content);
        assert_eq!(slices(&r, content), vec!["<!-- one -->", "<!-- two -->"]);
    }

    #[test]
    fn test_markdown_empty_input_yields_no_ranges() {
        let r = detect_markdown_comment_ranges("");
        assert!(r.is_empty());
    }

    // ---------- The DESIGN.md regression case ----------

    #[test]
    fn test_markdown_designdotmd_class_case_does_not_swallow_body_after_backticks() {
        // The pattern that caught the v0.1 bug: a `<!--` literal inside
        // inline code (or otherwise text-like-but-protected) followed
        // much later by a `-->` elsewhere. The v0.1 implementation
        // would have flagged everything between as "in comment". The
        // v0.2 implementation must not.
        let content = "\
The HTML opener `<!--` is shown above.

Later text discussing `-->` in passing.

DOCREF(/docs/foo) should NOT be flagged as in-comment.
";
        let r = detect_markdown_comment_ranges(content);
        assert!(r.is_empty(), "got: {:?}", slices(&r, content));
    }
}
