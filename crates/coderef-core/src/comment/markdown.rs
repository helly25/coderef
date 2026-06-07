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
use super::region::{ClassifiedRange, RegionKind};

/// Detect classified regions in a markdown buffer.
///
/// v0.2 emits four kinds:
///   - `BlockComment` for `<!-- ... -->` outside protected regions
///   - `CodeSnippet`  for fenced ` ```...``` ` / `~~~...~~~` blocks
///   - `CodeSnippet`  for inline backtick spans (`` `code` ``)
///   - `CodeSnippet`  for indented (4-space / tab) code blocks
///
/// All four are comment-like under `commentsOnly: true` per
/// `RegionKind::is_comment_like`. A reference inside a markdown code
/// block (or HTML comment) is therefore picked up by patterns scoped
/// to comments — see `DESIGN.md` §5.4.1 v0.2 notes.
pub fn detect_markdown_regions(content: &str) -> Vec<ClassifiedRange> {
    let bytes = content.as_bytes();
    let mut regions: Vec<ClassifiedRange> = Vec::new();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        // Fenced code block? Only at the start of a line (after optional
        // whitespace, per CommonMark spec).
        if at_line_start(bytes, i) {
            if let Some((fence_char, count, fence_open_end)) = match_fence_open(bytes, i) {
                let start = i;
                let end = find_fence_close(bytes, fence_open_end, fence_char, count).unwrap_or(len);
                regions.push(ClassifiedRange {
                    range: Range { start, end },
                    kind: RegionKind::CodeSnippet,
                });
                i = end;
                continue;
            }
        }

        // Inline code span? Any run of N backticks (N >= 1).
        if bytes[i] == b'`' {
            let start = i;
            let run = count_backticks(bytes, i);
            let end = find_inline_code_close(bytes, i + run, run).unwrap_or(len);
            regions.push(ClassifiedRange {
                range: Range { start, end },
                kind: RegionKind::CodeSnippet,
            });
            i = end;
            continue;
        }

        // HTML block comment? `<!--`
        if starts_with(bytes, i, b"<!--") {
            let start = i;
            let body_start = i + 4;
            let body_end = find_subsequence(bytes, b"-->", body_start).map_or(len, |pos| pos + 3);
            regions.push(ClassifiedRange {
                range: Range {
                    start,
                    end: body_end,
                },
                kind: RegionKind::BlockComment,
            });
            i = body_end;
            continue;
        }

        i += utf8_char_len(bytes[i]);
    }

    // Second pass: indented code blocks. CommonMark rule — a block
    // of one or more lines indented 4+ spaces (or a tab) that follows
    // a blank line or the start of the document. Stops at the next
    // non-blank non-indented line.
    //
    // Done in a second pass over the buffer that's already had
    // fenced + inline + HTML-comment regions found, so we don't
    // double-classify a line that's already inside one of those.
    for r in detect_indented_code_regions(bytes, &regions) {
        regions.push(r);
    }
    regions.sort_by_key(|r| r.range.start);
    regions
}

/// Walk `bytes` line by line; emit `CodeSnippet` ranges for indented
/// (4+ space or tab-indented) code blocks that don't overlap any
/// already-detected region.
fn detect_indented_code_regions(
    bytes: &[u8],
    existing: &[ClassifiedRange],
) -> Vec<ClassifiedRange> {
    let mut out = Vec::new();
    let mut prev_was_blank = true; // start-of-doc counts as blank
    let mut line_start = 0;
    let len = bytes.len();
    while line_start < len {
        let line_end = find_line_end(bytes, line_start);
        let already_in_region = existing.iter().any(|r| r.range.contains(line_start));
        if already_in_region {
            prev_was_blank = false;
            line_start = line_end;
            continue;
        }
        let blank = is_blank_line(bytes, line_start, line_end);
        if blank {
            prev_was_blank = true;
            line_start = line_end;
            continue;
        }
        if prev_was_blank && is_indented_code_start(bytes, line_start) {
            let block_start = line_start;
            // Extend through subsequent indented OR blank lines, but
            // trim trailing blank lines back (they're not part of
            // the block per CommonMark).
            let mut block_end = line_end;
            let mut last_non_blank_end = line_end;
            let mut scan = line_end;
            while scan < len {
                let nle = find_line_end(bytes, scan);
                let nb = is_blank_line(bytes, scan, nle);
                let in_existing = existing.iter().any(|r| r.range.contains(scan));
                if in_existing {
                    break;
                }
                if nb {
                    scan = nle;
                    block_end = scan;
                    continue;
                }
                if is_indented_code_start(bytes, scan) {
                    scan = nle;
                    block_end = scan;
                    last_non_blank_end = scan;
                    continue;
                }
                break;
            }
            // Trim trailing blanks from the block_end.
            let _ = block_end;
            out.push(ClassifiedRange {
                range: Range {
                    start: block_start,
                    end: last_non_blank_end,
                },
                kind: RegionKind::CodeSnippet,
            });
            line_start = last_non_blank_end;
            prev_was_blank = false;
            continue;
        }
        prev_was_blank = false;
        line_start = line_end;
    }
    out
}

fn find_line_end(bytes: &[u8], from: usize) -> usize {
    let nl = bytes[from..].iter().position(|b| *b == b'\n');
    match nl {
        Some(off) => from + off + 1, // consume newline as part of line
        None => bytes.len(),
    }
}

fn is_blank_line(bytes: &[u8], start: usize, end: usize) -> bool {
    bytes[start..end]
        .iter()
        .all(|b| matches!(*b, b' ' | b'\t' | b'\r' | b'\n'))
}

/// True iff the line at `from` starts with at least 4 spaces or one
/// tab (the `CommonMark` indentation that opens an indented code block).
fn is_indented_code_start(bytes: &[u8], from: usize) -> bool {
    if from >= bytes.len() {
        return false;
    }
    if bytes[from] == b'\t' {
        return true;
    }
    let take = bytes.len().min(from + 4);
    bytes[from..take].iter().all(|b| *b == b' ') && take - from == 4
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

    fn slices<'a>(regions: &[ClassifiedRange], content: &'a str) -> Vec<&'a str> {
        regions
            .iter()
            .map(|r| &content[r.range.start..r.range.end])
            .collect()
    }

    fn kinds(regions: &[ClassifiedRange]) -> Vec<RegionKind> {
        regions.iter().map(|r| r.kind).collect()
    }

    #[test]
    fn test_markdown_html_block_comment_classified_as_block_comment() {
        let content = "# Title\n<!-- hidden -->\nbody";
        let r = detect_markdown_regions(content);
        assert_eq!(slices(&r, content), vec!["<!-- hidden -->"]);
        assert_eq!(kinds(&r), vec![RegionKind::BlockComment]);
    }

    #[test]
    fn test_markdown_inline_code_classified_as_code_snippet() {
        let content = "Doc says `<!--` is the opener.";
        let r = detect_markdown_regions(content);
        // v0.2: the inline `<!--` literal is now a CodeSnippet region,
        // not a swallowed-and-dropped fragment.
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
        assert!(slices(&r, content)[0].contains("<!--"));
    }

    #[test]
    fn test_markdown_fenced_block_classified_as_code_snippet() {
        let content = "intro\n\n```\n<!-- example -->\n```\n\nbody";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
        assert!(slices(&r, content)[0].contains("<!-- example -->"));
    }

    #[test]
    fn test_markdown_fenced_block_then_real_html_comment_classified_separately() {
        let content = "```\n<!-- inside -->\n```\n<!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
        assert_eq!(slices(&r, content)[1], "<!-- real -->");
    }

    #[test]
    fn test_markdown_tilde_fence_classified_as_code_snippet() {
        let content = "~~~\n<!-- escaped -->\n~~~\n<!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
    }

    #[test]
    fn test_markdown_fence_with_info_string_still_classified_as_code_snippet() {
        let content = "```html\n<!-- example -->\n```\n<!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
    }

    #[test]
    fn test_markdown_longer_fence_can_be_closed_only_by_at_least_same_length() {
        let content = "````\nbody with ``` inside\n````\n<!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
        assert_eq!(slices(&r, content)[1], "<!-- real -->");
    }

    #[test]
    fn test_markdown_double_backtick_inline_code_classified_as_code_snippet() {
        let content = "intro ``a `b` c`` <!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
        assert_eq!(slices(&r, content)[1], "<!-- real -->");
    }

    #[test]
    fn test_markdown_unterminated_block_comment_extends_to_eof() {
        let content = "lead\n<!-- never closes";
        let r = detect_markdown_regions(content);
        assert_eq!(slices(&r, content), vec!["<!-- never closes"]);
        assert_eq!(kinds(&r), vec![RegionKind::BlockComment]);
    }

    #[test]
    fn test_markdown_unterminated_fence_classified_as_code_snippet_to_eof() {
        let content = "intro\n```\n<!-- still inside -->\nno close here";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
    }

    #[test]
    fn test_markdown_leading_whitespace_before_fence_is_accepted() {
        let content = "intro\n  ```\n<!-- escaped -->\n  ```\n<!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
    }

    #[test]
    fn test_markdown_multiple_html_comments_in_body() {
        let content = "<!-- one -->\nbody\n<!-- two -->";
        let r = detect_markdown_regions(content);
        assert_eq!(slices(&r, content), vec!["<!-- one -->", "<!-- two -->"]);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::BlockComment, RegionKind::BlockComment]
        );
    }

    #[test]
    fn test_markdown_empty_input_yields_no_regions() {
        let r = detect_markdown_regions("");
        assert!(r.is_empty());
    }

    // ---------- v0.2: code snippets carry references through ----------

    #[test]
    fn test_markdown_inline_code_with_reference_emits_one_code_snippet() {
        // The change of intent from PR #9 to this one: the inline code
        // containing JIRA(PROJ-1) is now a CodeSnippet region. Under
        // `commentsOnly: true` it's comment-like and the JIRA matches
        // inside survive the scope filter.
        let content = "See `JIRA(PROJ-1)` for context.";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
        let s = slices(&r, content)[0];
        assert!(s.contains("JIRA(PROJ-1)"), "got: {s}");
    }

    // ---------- Indented (4-space / tab) code blocks ----------

    #[test]
    fn test_markdown_indented_code_block_after_blank_line_classified_as_code_snippet() {
        // Standard CommonMark: blank line, then 4-space-indented line opens
        // an indented code block. Treat it as a CodeSnippet.
        let content = "intro line\n\n    let x = TODO(@alice);\n\nbody";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
        assert!(
            slices(&r, content)[0].contains("TODO(@alice)"),
            "got: {:?}",
            slices(&r, content),
        );
    }

    #[test]
    fn test_markdown_indented_code_block_at_doc_start_is_recognised() {
        // Document-start counts as 'after blank line' for the opener check.
        let content = "    let x = 1;\n    let y = 2;\nbody";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
    }

    #[test]
    fn test_markdown_indented_line_in_middle_of_paragraph_is_not_a_code_block() {
        // CommonMark: a 4-space-indented line is NOT a code block if the
        // previous line is non-blank (it's a paragraph continuation).
        let content = "paragraph line 1\n    paragraph line 2 (visually indented)\n";
        let r = detect_markdown_regions(content);
        assert!(r.is_empty(), "got: {:?}", slices(&r, content));
    }

    #[test]
    fn test_markdown_indented_code_block_extends_through_consecutive_indented_lines() {
        let content = "intro\n\n    line A\n    line B\n    line C\n\nbody";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
        let s = slices(&r, content)[0];
        assert!(s.contains("line A"));
        assert!(s.contains("line B"));
        assert!(s.contains("line C"));
    }

    #[test]
    fn test_markdown_tab_indented_line_after_blank_is_a_code_block() {
        // A single tab is equivalent to 4 spaces for indented-block purposes.
        let content = "intro\n\n\tlet x = TODO(@bob);\n\nbody";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
        assert!(slices(&r, content)[0].contains("TODO(@bob)"));
    }

    #[test]
    fn test_markdown_indented_block_followed_by_real_html_comment_emits_both() {
        let content = "intro\n\n    let x = 1;\n\n<!-- real -->";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::BlockComment]
        );
    }

    #[test]
    fn test_markdown_indented_block_doesnt_re_emit_inside_fenced_block() {
        // A fenced block whose body happens to be 4-space-indented should
        // emit ONCE (as the fenced block, not also as an indented one).
        let content = "intro\n\n```\n    indented inside fence\n```\n";
        let r = detect_markdown_regions(content);
        assert_eq!(kinds(&r), vec![RegionKind::CodeSnippet]);
    }

    #[test]
    fn test_markdown_designdotmd_class_case_emits_each_region_correctly() {
        // PR #9's "doesn't swallow body" test, updated for v0.2: the
        // inline `<!--` and `-->` are each their own CodeSnippet
        // regions. The DOCREF in body text isn't in any region.
        let content = "\
The HTML opener `<!--` is shown above.

Later text discussing `-->` in passing.

DOCREF(/docs/foo) is body text.
";
        let r = detect_markdown_regions(content);
        assert_eq!(
            kinds(&r),
            vec![RegionKind::CodeSnippet, RegionKind::CodeSnippet]
        );
        // The DOCREF byte offset is OUTSIDE every emitted region.
        let docref_offset = content.find("DOCREF").unwrap();
        assert!(
            !r.iter().any(|cr| cr.range.contains(docref_offset)),
            "DOCREF should be in body text, not in any region"
        );
    }
}
