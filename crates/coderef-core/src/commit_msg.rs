//! Commit-message linting (DESIGN.md §16.1.1).
//!
//! `coderef commit-msg <file>` reads the file, strips git's comment
//! lines (`^#`), runs the pattern scanner over the remaining text,
//! filters to patterns where `scope.commitMessage` resolves to `true`
//! or `"required"`, and verifies each match exactly like
//! `coderef check` does on source files.
//!
//! `"required"` patterns enforce "every commit message must contain
//! at least one match" — a `RequiredMissing` entry is emitted if none
//! of their matches survived the scan.

use serde::{Deserialize, Serialize};

use crate::check::CheckResult;
use crate::config::{Config, Pattern};
use crate::pattern::CompiledPattern;
use crate::scan::{scan_file, ScanError, ScanOptions};
use crate::variables::Context;
use crate::verify::{verify_reference, VerifyError, VerifyOptions, VerifyOutcome};

/// Filename label used in `Reference.file` for commit-message scans.
/// Same fixed label is used regardless of where the message came from
/// — the file path passed in is for input, not for diagnostics.
pub const COMMIT_MSG_FILE_LABEL: &str = "<commit-message>";

/// Outcome of linting a commit message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitMsgReport {
    /// One entry per matched reference, with its verification result.
    pub results: Vec<CheckResult>,
    /// One entry per `"required"` pattern that didn't match.
    pub required_missing: Vec<RequiredMissing>,
    /// Aggregate counts.
    pub total: usize,
    pub ok: usize,
    pub broken: usize,
    pub skipped: usize,
}

impl CommitMsgReport {
    /// `true` iff no required pattern was missing AND no reference
    /// broke. What `coderef commit-msg` exits zero on.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.broken == 0 && self.required_missing.is_empty()
    }
}

/// One required-pattern miss.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequiredMissing {
    pub pattern_id: String,
    /// Optional human-readable hint; populated from the pattern's
    /// `description` when set.
    pub description: Option<String>,
}

// Effective-scope resolution lives in `config::scope` so the (WASM-
// safe) doctor module can use it without dragging in this verifier.
// Re-export the symbols here so existing call sites
// (`crate::commit_msg::effective_scope`, etc.) keep compiling.
pub use crate::config::resolve_commit_message_scope as effective_scope;
pub use crate::config::EffectiveCommitMessageScope as EffectiveScope;

/// Strip git's comment lines (`^#`) from a commit-message string,
/// returning the lint-relevant text. Lines that *start* with `#` are
/// dropped; lines containing `#` mid-text are preserved.
#[must_use]
pub fn strip_commit_comments(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for line in raw.split_inclusive('\n') {
        // Note: `split_inclusive` preserves the terminating newline
        // on each segment, so we don't need to re-add one.
        let body = line.trim_start_matches([' ', '\t']);
        if body.starts_with('#') {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Run the commit-message linter over `message`.
///
/// `config` provides patterns + variables; `verify_opts` is reused
/// from `coderef check` (timeout, `accept_status`, workspace root
/// for any `kind: "local"` references that might appear in the
/// message).
pub fn check_commit_message(
    message: &str,
    config: &Config,
    verify_opts: &VerifyOptions,
) -> Result<CommitMsgReport, CommitMsgError> {
    // 1. Strip git comments.
    let body = strip_commit_comments(message);

    // 2. Build the filtered + compiled pattern set.
    let mut compiled: Vec<(CompiledPattern, Pattern)> = Vec::new();
    let mut required_ids: Vec<(String, Option<String>)> = Vec::new();
    for (id, raw) in &config.patterns {
        match effective_scope(raw) {
            EffectiveScope::Skip => continue,
            EffectiveScope::Required => {
                required_ids.push((id.clone(), raw.description.clone()));
            }
            EffectiveScope::Scan => {}
        }
        // Skip patterns whose regex or target shape is invalid; doctor
        // would have already flagged those statically. Letting them
        // through would abort the whole lint.
        let Ok(c) = CompiledPattern::compile(id.clone(), raw) else {
            continue;
        };
        compiled.push((c, raw.clone()));
    }

    // 3. Scan the message body. No language is passed so the scanner's
    //    `commentsOnly` filter would drop matches — but commit
    //    messages aren't source code; a future iteration may add a
    //    pseudo-language. For now, patterns that set
    //    `scope.commentsOnly: true` will miss in commit messages.
    //    Document this in the help text.
    let ctx = Context::new().with_strict(false);
    let opts = ScanOptions {
        patterns: &compiled,
        language: None,
        base_context: &ctx,
        file: COMMIT_MSG_FILE_LABEL,
    };
    let refs = scan_file(&body, &opts).map_err(CommitMsgError::Scan)?;

    // 4. Snapshot matched pattern ids *before* consuming refs, so we
    //    can compare against `required_ids` after verification.
    let matched_pattern_ids: std::collections::HashSet<String> =
        refs.iter().map(|r| r.pattern_id.clone()).collect();

    // 5. Verify each matched reference.
    let mut results = Vec::with_capacity(refs.len());
    let mut ok = 0;
    let mut broken = 0;
    let mut skipped = 0;
    for reference in refs {
        let outcome = verify_reference(&reference, verify_opts).map_err(CommitMsgError::Verify)?;
        match &outcome {
            VerifyOutcome::Ok => ok += 1,
            VerifyOutcome::Skipped { .. } => skipped += 1,
            _ => broken += 1,
        }
        results.push(CheckResult { reference, outcome });
    }

    // 6. Required-but-missing: every required pattern that didn't
    //    produce at least one match.
    let mut required_missing = Vec::new();
    for (id, description) in required_ids {
        if !matched_pattern_ids.contains(&id) {
            required_missing.push(RequiredMissing {
                pattern_id: id,
                description,
            });
        }
    }

    let total = results.len();
    Ok(CommitMsgReport {
        results,
        required_missing,
        total,
        ok,
        broken,
        skipped,
    })
}

/// Failures from `check_commit_message`.
#[derive(Debug, thiserror::Error)]
pub enum CommitMsgError {
    #[error(transparent)]
    Scan(ScanError),
    #[error(transparent)]
    Verify(VerifyError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg(src: &str) -> Config {
        Config::from_jsonc_str(src).unwrap()
    }

    #[test]
    fn test_strip_commit_comments_drops_leading_hash_lines() {
        let raw = "feat: x\n\n# Please enter the commit message\n# starting with #\nBody\n";
        let body = strip_commit_comments(raw);
        assert_eq!(body, "feat: x\n\nBody\n");
    }

    #[test]
    fn test_strip_commit_comments_preserves_inline_hash() {
        let raw = "fix: address comment #142\n";
        let body = strip_commit_comments(raw);
        assert_eq!(body, "fix: address comment #142\n");
    }

    #[test]
    fn test_strip_commit_comments_drops_indented_hash_lines() {
        // git's `commentChar` lines are typically column 0 but some
        // configs add a leading space. Be lenient.
        let raw = "feat: x\n  # indented\nBody\n";
        let body = strip_commit_comments(raw);
        assert_eq!(body, "feat: x\nBody\n");
    }

    #[test]
    fn test_effective_scope_url_default_is_scan() {
        let c = cfg(r#"{ "patterns": { "u": {
            "regex":  "TODO\\(@(?<x>\\w+)\\)",
            "target": "https://x/${x}"
        } } }"#);
        let p = c.patterns.get("u").unwrap();
        assert_eq!(effective_scope(p), EffectiveScope::Scan);
    }

    #[test]
    fn test_effective_scope_local_default_is_scan() {
        let c = cfg(r#"{ "patterns": { "d": {
            "kind":   "local",
            "regex":  "DOC\\((?<p>[^)]+)\\)",
            "target": "${p}"
        } } }"#);
        let p = c.patterns.get("d").unwrap();
        assert_eq!(effective_scope(p), EffectiveScope::Scan);
    }

    #[test]
    fn test_effective_scope_block_default_is_skip() {
        let c = cfg(r#"{ "patterns": { "b": {
            "kind":  "block",
            "regex": "NOCOMMIT"
        } } }"#);
        let p = c.patterns.get("b").unwrap();
        assert_eq!(effective_scope(p), EffectiveScope::Skip);
    }

    #[test]
    fn test_effective_scope_explicit_required_takes_precedence() {
        let c = cfg(r#"{ "patterns": { "j": {
            "regex":  "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
            "target": "https://j/${t}",
            "scope":  { "commitMessage": "required" }
        } } }"#);
        let p = c.patterns.get("j").unwrap();
        assert_eq!(effective_scope(p), EffectiveScope::Required);
    }

    #[test]
    fn test_effective_scope_explicit_false_overrides_kind_default() {
        let c = cfg(r#"{ "patterns": { "u": {
            "regex":  "TODO\\(@(?<x>\\w+)\\)",
            "target": "https://x/${x}",
            "scope":  { "commitMessage": false }
        } } }"#);
        let p = c.patterns.get("u").unwrap();
        assert_eq!(effective_scope(p), EffectiveScope::Skip);
    }

    #[test]
    fn test_check_commit_message_required_pattern_missing_fails() {
        let c = cfg(r#"{ "patterns": { "j": {
            "regex":  "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
            "target": "https://j.example.test/${t}",
            "scope":  { "commitMessage": "required" }
        } } }"#);
        let opts = VerifyOptions::default();
        let report = check_commit_message("feat(auth): swap KDF\n", &c, &opts).unwrap();
        assert_eq!(report.total, 0);
        assert_eq!(report.required_missing.len(), 1);
        assert_eq!(report.required_missing[0].pattern_id, "j");
        assert!(!report.passed());
    }

    #[test]
    fn test_check_commit_message_required_pattern_present_passes_(/* … network */) {
        // The required-presence check is what we're testing; we don't
        // want HTTP verification to flake the test, so use a pattern
        // whose kind: "block" outcome we treat as broken if matched.
        // Actually `block` defaults to Skip in commit-msg scope, so
        // we need to *override* it to scan. Then any block match would
        // count as broken — exactly what we want to avoid here.
        //
        // Instead: use a `kind: "local"` required pattern whose
        // target is a directory that always exists at the verifier's
        // workspace root. We use the tempdir-style approach for that.
        let dir = std::env::temp_dir().join(format!(
            "coderef-cm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "x").unwrap();
        let c = cfg(r#"{ "patterns": { "d": {
            "kind":   "local",
            "regex":  "DOCREF\\((?<p>[^)]+)\\)",
            "target": "${p}",
            "scope":  { "commitMessage": "required" }
        } } }"#);
        let opts = VerifyOptions {
            workspace_root: dir.clone(),
            ..VerifyOptions::default()
        };
        let report = check_commit_message("feat: x — see DOCREF(/a.md)\n", &c, &opts).unwrap();
        assert!(report.required_missing.is_empty());
        assert_eq!(report.total, 1);
        assert!(report.passed(), "{:#?}", report.results);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_check_commit_message_ifchange_default_skip_does_not_lint() {
        // `kind: "ifchange"` defaults to scope.commitMessage = false.
        // Even if the message contains an IfChange marker, it should
        // be skipped entirely (no scan, no required-missing report).
        // Note: ifchange compile errors at this stage on v0.1
        // (pattern.kindNotYetImplemented); the lint must not abort.
        let c = cfg(r#"{ "patterns": { "ic": {
            "kind":  "ifchange",
            "regex": "IfChange\\(([^)]+)\\)"
        } } }"#);
        let opts = VerifyOptions::default();
        let report = check_commit_message("feat: x\n\nIfChange(some-id)\n", &c, &opts).unwrap();
        assert_eq!(report.total, 0);
        assert!(report.required_missing.is_empty());
        assert!(report.passed());
    }

    #[test]
    fn test_check_commit_message_strips_git_comments_before_scan() {
        // The TODO is inside a `# ...` comment line and must be
        // ignored. (The body otherwise contains no triggering
        // pattern.) We verify by setting the pattern as required
        // and confirming it's missing.
        let c = cfg(r#"{ "patterns": { "u": {
            "regex":  "TODO\\(@(?<x>\\w+)\\)",
            "target": "https://x.example.test/${x}",
            "scope":  { "commitMessage": "required" }
        } } }"#);
        let opts = VerifyOptions::default();
        let raw = "feat: x\n# TODO(@alice) — comment, ignored\nbody\n";
        let report = check_commit_message(raw, &c, &opts).unwrap();
        assert_eq!(report.required_missing.len(), 1);
        assert_eq!(report.total, 0);
        assert!(!report.passed());
    }
}
