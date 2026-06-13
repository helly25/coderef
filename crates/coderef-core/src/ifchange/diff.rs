//! Unified-diff parsing → per-file changed-line interval sets.
//!
//! `coderef changes` runs `git diff -U0` and feeds the output through
//! `parse_unified_diff`, which produces `ChangedLines` — a map from
//! file path to the set of line numbers (in the *new* file) that the
//! diff touches. The verifier then asks `ChangedLines::touches(file,
//! line_or_range)` to know whether a block / target was hit.
//!
//! We use `-U0` so each hunk header (`@@ -A,B +C,D @@`) names the
//! exact line span of the change. Renames are tracked via the
//! `rename to <path>` extended header; for v0.2 we treat the renamed
//! path as the canonical file (so peer blocks in the new path
//! resolve correctly).

use std::collections::BTreeMap;

/// Per-file set of changed line numbers (1-indexed, new-file numbering).
#[derive(Clone, Debug, Default)]
pub struct ChangedLines {
    inner: BTreeMap<String, Vec<(u32, u32)>>,
}

impl ChangedLines {
    /// Returns `true` if the file has *any* changed line. The
    /// lookup is leading-`/`-insensitive — `coderef`'s
    /// workspace-rooted convention writes paths as `/docs/x.md`,
    /// while `git diff` emits them without the leading slash. Both
    /// forms compare equal here.
    #[must_use]
    pub fn file_touched(&self, file: &str) -> bool {
        let key = file.trim_start_matches('/');
        self.inner.get(key).is_some_and(|ivs| !ivs.is_empty())
    }

    /// Returns `true` if any of `lines` falls inside a changed
    /// interval for `file`. Inclusive on both ends.
    #[must_use]
    pub fn intersects(&self, file: &str, start: u32, end: u32) -> bool {
        let key = file.trim_start_matches('/');
        let Some(ivs) = self.inner.get(key) else {
            return false;
        };
        ivs.iter().any(|&(a, b)| !(b < start || a > end))
    }

    /// Sorted list of files that have any changed lines.
    #[must_use]
    pub fn files(&self) -> Vec<&str> {
        self.inner
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Test-friendly: feed in pre-built data.
    #[cfg(test)]
    pub fn from_pairs(pairs: &[(&str, &[(u32, u32)])]) -> Self {
        let mut m = BTreeMap::new();
        for (f, ivs) in pairs {
            m.insert((*f).to_string(), ivs.to_vec());
        }
        Self { inner: m }
    }
}

/// Parse `git diff -U0` output.
///
/// Skips binary diffs. Honours rename headers (the new path becomes
/// the canonical file). Headers like `--- a/foo` / `+++ b/foo` give
/// the path; the `@@ -A,B +C,D @@` hunk header gives the
/// changed-line range in the new file.
#[must_use]
pub fn parse_unified_diff(diff: &str) -> ChangedLines {
    let mut out: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for raw in diff.lines() {
        let line = raw;

        // File header. `git diff` emits both `--- a/foo` and `+++ b/foo`;
        // the `+++` line names the new file (which is what we want for
        // line-number references after the change).
        if let Some(rest) = line.strip_prefix("+++ ") {
            current = strip_diff_path_prefix(rest);
            // Initialise an empty entry so the file shows in `files()`
            // even if it has only deletions (whose `+` count is 0).
            if let Some(ref c) = current {
                out.entry(c.clone()).or_default();
            }
            continue;
        }
        if line.starts_with("--- ") {
            continue;
        }

        // Rename header: `rename to <path>` overrides the file path
        // for subsequent hunks until the next `+++`.
        if let Some(path) = line.strip_prefix("rename to ") {
            current = Some(path.to_string());
            out.entry(path.to_string()).or_default();
            continue;
        }

        // Hunk header: `@@ -OLDSTART,OLDLEN +NEWSTART,NEWLEN @@`.
        if let Some(rest) = line.strip_prefix("@@") {
            let Some(hunk) = parse_hunk_header(rest) else {
                continue;
            };
            if let Some(ref f) = current {
                let (start, len) = hunk;
                if len == 0 {
                    // Pure deletion in the new file. `git diff -U0`
                    // emits `+NEWSTART,0` to mean "deletion at
                    // position NEWSTART, no new lines". We record
                    // the position as a zero-length probe — but
                    // intersects() requires start ≤ end, so use the
                    // single line `start..=start` as the marker.
                    // Callers asking "did file F change at line X"
                    // will say yes only if X == start.
                    out.entry(f.clone())
                        .or_default()
                        .push((start.max(1), start.max(1)));
                } else {
                    out.entry(f.clone())
                        .or_default()
                        .push((start, start + len - 1));
                }
            }
        }
    }

    ChangedLines { inner: out }
}

/// Strip the `a/` or `b/` prefix that git uses on diff paths. `git
/// diff --no-prefix` skips it; the default doesn't.
fn strip_diff_path_prefix(s: &str) -> Option<String> {
    let s = s.trim();
    if s == "/dev/null" {
        return None;
    }
    Some(
        s.strip_prefix("a/")
            .or_else(|| s.strip_prefix("b/"))
            .unwrap_or(s)
            .to_string(),
    )
}

/// Parse the *suffix* of a unified-diff hunk header — everything
/// after the leading `@@`. Format: ` -OLDSTART[,OLDLEN] +NEWSTART[,NEWLEN] @@`.
/// Returns (`new_start`, `new_len`). `new_len` defaults to 1 when omitted
/// (matches GNU diff conventions).
fn parse_hunk_header(rest: &str) -> Option<(u32, u32)> {
    // Find the new-side `+` segment. Walk char-by-char to keep it
    // dependency-free.
    let plus = rest.find('+')?;
    let after_plus = &rest[plus + 1..];
    // The new-side spec runs to the next space or `@@`.
    let spec_end = after_plus
        .find(' ')
        .or_else(|| after_plus.find('@'))
        .unwrap_or(after_plus.len());
    let spec = &after_plus[..spec_end];
    let (start_s, len_s) = match spec.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (spec, None),
    };
    let start: u32 = start_s.parse().ok()?;
    let len: u32 = match len_s {
        Some(s) => s.parse().ok()?,
        None => 1,
    };
    Some((start, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unified_diff_single_file_single_hunk_records_range() {
        let diff = "\
diff --git a/src/foo.rs b/src/foo.rs
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -10,0 +11,3 @@
+let x = 1;
+let y = 2;
+let z = 3;
";
        let cl = parse_unified_diff(diff);
        assert!(cl.file_touched("src/foo.rs"));
        assert!(cl.intersects("src/foo.rs", 11, 11));
        assert!(cl.intersects("src/foo.rs", 13, 13));
        assert!(!cl.intersects("src/foo.rs", 14, 100));
    }

    #[test]
    fn test_parse_unified_diff_multiple_hunks() {
        let diff = "\
+++ b/src/a.rs
@@ -1,1 +1,1 @@
@@ -100,0 +105,5 @@
";
        let cl = parse_unified_diff(diff);
        assert!(cl.intersects("src/a.rs", 1, 1));
        assert!(cl.intersects("src/a.rs", 105, 109));
        assert!(!cl.intersects("src/a.rs", 50, 60));
    }

    #[test]
    fn test_parse_unified_diff_no_count_defaults_to_one_line() {
        let diff = "\
+++ b/a.rs
@@ -7 +7 @@
-bar
+baz
";
        let cl = parse_unified_diff(diff);
        assert!(cl.intersects("a.rs", 7, 7));
        assert!(!cl.intersects("a.rs", 6, 6));
    }

    #[test]
    fn test_parse_unified_diff_pure_deletion_records_position() {
        let diff = "\
+++ b/a.rs
@@ -10,3 +9,0 @@
-line a
-line b
-line c
";
        let cl = parse_unified_diff(diff);
        // The "+0" length means deletion. We treat that as a
        // probe-point at line 9 so a check "is line 9 changed?"
        // succeeds — that's the closest interpretation of "the
        // change happened at this position".
        assert!(cl.intersects("a.rs", 9, 9));
        assert!(!cl.intersects("a.rs", 10, 10));
    }

    #[test]
    fn test_parse_unified_diff_dev_null_old_side_treated_as_create() {
        let diff = "\
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,2 @@
+line 1
+line 2
";
        let cl = parse_unified_diff(diff);
        assert!(cl.file_touched("src/new.rs"));
        assert!(cl.intersects("src/new.rs", 1, 2));
    }

    #[test]
    fn test_parse_unified_diff_dev_null_new_side_skipped() {
        // File deletion: the new path is /dev/null, so no entry.
        let diff = "\
--- a/src/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-line 1
-line 2
";
        let cl = parse_unified_diff(diff);
        assert!(!cl.file_touched("src/gone.rs"));
    }

    #[test]
    fn test_parse_unified_diff_rename_uses_new_path() {
        let diff = "\
diff --git a/old.rs b/new.rs
rename from old.rs
rename to new.rs
--- a/old.rs
+++ b/new.rs
@@ -3,1 +3,1 @@
-x
+y
";
        let cl = parse_unified_diff(diff);
        assert!(cl.intersects("new.rs", 3, 3));
    }

    #[test]
    fn test_parse_unified_diff_strips_a_b_path_prefix() {
        let diff = "\
+++ b/path/with/prefix.rs
@@ -1,1 +1,1 @@
";
        let cl = parse_unified_diff(diff);
        // The `a/` prefix gone; `path/with/prefix.rs` is the key.
        assert!(cl.intersects("path/with/prefix.rs", 1, 1));
    }

    #[test]
    fn test_changed_lines_files_lists_touched_only() {
        let cl = ChangedLines::from_pairs(&[("touched.rs", &[(1, 5)]), ("empty.rs", &[])]);
        assert_eq!(cl.files(), vec!["touched.rs"]);
    }
}
