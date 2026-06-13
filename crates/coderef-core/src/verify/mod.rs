//! Reference verification.
//!
//! Given a `Reference`, decides whether its target resolves
//! successfully under the rules for its `pattern_kind`:
//!
//! - `Url` — HTTP HEAD (falling back to GET on 405) within configured
//!   timeout + backoff; success when the status is in `accept_status`
//!   (default 2xx). TLS validation always on (DESIGN.md §18).
//! - `Local` — workspace-relative path exists on the filesystem.
//! - other kinds — `VerifyOutcome::Skipped` with a reason.
//!
//! Caching, parallelism, response-body filtering and anchor verification
//! are deferred to later versions (DESIGN.md §13.4 / §13.3.1).

mod http;
mod local;

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

use crate::config::PatternKind;
use crate::reference::Reference;

/// Result of verifying a single reference.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum VerifyOutcome {
    /// Target resolved successfully.
    Ok,
    /// HTTP returned a status not in `accept_status`.
    BrokenStatus { status: u16 },
    /// Network failure (DNS, connect, TLS, timeout, etc.).
    BrokenNetwork { reason: String },
    /// Local-path target did not exist.
    NotFound { path: String },
    /// A `kind: "block"` pattern matched — the presence of the marker
    /// is itself the failure (e.g. `DO NOT COMMIT`, `NOCOMMIT`,
    /// `DONOTMERGE`). `matched_text` is the literal token that matched.
    BlockMarker { matched_text: String },
    /// Local-path target resolved, but its `#anchor` suffix didn't
    /// match any heading slug in the target file (DESIGN.md §6.3).
    /// `suggestion` carries the Levenshtein-1 hit when one exists.
    AnchorNotFound {
        path: String,
        anchor: String,
        suggestion: Option<String>,
    },
    /// Kind not yet implemented or verification turned off.
    Skipped { reason: String },
}

impl VerifyOutcome {
    /// `true` for `Ok`; `false` for any broken/not-found.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    /// `true` for `Skipped`.
    #[must_use]
    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    /// `true` for anything that should fail an exit-code-1 sweep.
    #[must_use]
    pub fn is_failure(&self) -> bool {
        !self.is_ok() && !self.is_skipped()
    }
}

/// Per-verify configuration. Coarse-grained for v0.1; per-pattern
/// overrides land in v0.2 once the integration with `VerifyToggle` /
/// `actions.verify` is fleshed out.
#[derive(Clone, Debug)]
pub struct VerifyOptions {
    /// Per-request connect + read timeout.
    pub timeout: Duration,
    /// HTTP statuses accepted as success. Empty list ⇒ default to
    /// `200..=299`.
    pub accept_status: Vec<u16>,
    /// Backoff parameters (apply to HTTP only).
    pub backoff: BackoffOptions,
    /// Workspace root, used to resolve relative local paths.
    pub workspace_root: std::path::PathBuf,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            accept_status: vec![],
            backoff: BackoffOptions::default(),
            workspace_root: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        }
    }
}

/// Exponential-backoff parameters.
#[derive(Clone, Debug)]
pub struct BackoffOptions {
    /// Maximum number of attempts (including the first). `1` ⇒ no retry.
    pub max_attempts: u32,
    /// Base delay between attempts.
    pub base: Duration,
    /// Multiplier applied to the previous delay on each retry.
    pub multiplier: f64,
    /// Cap on the per-attempt delay.
    pub max_delay: Duration,
}

impl Default for BackoffOptions {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base: Duration::from_millis(250),
            multiplier: 2.0,
            max_delay: Duration::from_secs(5),
        }
    }
}

impl BackoffOptions {
    /// Delay before the `attempt`-th retry (0-indexed; attempt 0 is the
    /// first retry, i.e. after the initial failure).
    #[must_use]
    pub fn delay_for(&self, attempt: u32) -> Duration {
        // u128 → f64 + f64 → u64 casts are intentional: timing
        // arithmetic doesn't need integer precision and rounding to
        // milliseconds is the goal.
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            let base_ms = self.base.as_millis() as f64;
            let scaled = base_ms * self.multiplier.powi(attempt as i32);
            let bounded = scaled.min(self.max_delay.as_millis() as f64);
            Duration::from_millis(bounded as u64)
        }
    }
}

/// Verify a single reference. Dispatches by `pattern_kind`.
///
/// Returns a `VerifyOutcome` on success of the verification *process*
/// (regardless of whether the target itself is reachable). `Err` is
/// reserved for misconfiguration or programmer error.
pub fn verify_reference(r: &Reference, opts: &VerifyOptions) -> Result<VerifyOutcome, VerifyError> {
    match r.pattern_kind {
        PatternKind::Url => Ok(self::http::verify_http(&r.target, opts)),
        PatternKind::Local => Ok(self::local::verify_local(&r.target, &opts.workspace_root)),
        PatternKind::Block => Ok(VerifyOutcome::BlockMarker {
            matched_text: r.matched_text.clone(),
        }),
        PatternKind::IfChange => Ok(VerifyOutcome::Skipped {
            reason: "kind `ifchange` is verified by `coderef changes` (v0.2)".into(),
        }),
        PatternKind::Command => Ok(VerifyOutcome::Skipped {
            reason: "kind `command` is a post-v0.4 feature".into(),
        }),
    }
}

/// Failures from the verification subsystem.
#[derive(Debug, Error)]
pub enum VerifyError {
    /// Network sub-system reported a failure that isn't covered by
    /// `BrokenNetwork` (e.g. agent build failed). Currently unused;
    /// reserved for future expansion.
    #[error("verifier setup failure: {0}")]
    Setup(String),
}

/// Resolve `target` under `workspace_root` per DESIGN.md §6.1's
/// default `workspace` anchor mode: a leading `/` is treated as
/// workspace-rooted (not filesystem-absolute). v0.2 will offer
/// `file` and `rootedOrFile` modes via per-pattern `resolve.anchor_mode`.
#[allow(dead_code)] // used by local::verify_local; doc-export for clarity.
pub(crate) fn join_under_workspace(workspace_root: &Path, target: &str) -> std::path::PathBuf {
    let trimmed = target.strip_prefix('/').unwrap_or(target);
    workspace_root.join(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_outcome_is_ok_only_for_ok() {
        assert!(VerifyOutcome::Ok.is_ok());
        assert!(!VerifyOutcome::BrokenStatus { status: 404 }.is_ok());
        assert!(!VerifyOutcome::NotFound { path: "x".into() }.is_ok());
        assert!(!VerifyOutcome::Skipped { reason: "x".into() }.is_ok());
    }

    #[test]
    fn test_verify_outcome_is_failure_excludes_ok_and_skipped() {
        assert!(!VerifyOutcome::Ok.is_failure());
        assert!(!VerifyOutcome::Skipped { reason: "x".into() }.is_failure());
        assert!(VerifyOutcome::BrokenStatus { status: 404 }.is_failure());
        assert!(VerifyOutcome::BrokenNetwork { reason: "x".into() }.is_failure());
        assert!(VerifyOutcome::NotFound { path: "x".into() }.is_failure());
    }

    #[test]
    fn test_backoff_delay_grows_with_attempt() {
        let b = BackoffOptions {
            max_attempts: 5,
            base: Duration::from_millis(100),
            multiplier: 2.0,
            max_delay: Duration::from_secs(60),
        };
        assert_eq!(b.delay_for(0), Duration::from_millis(100));
        assert_eq!(b.delay_for(1), Duration::from_millis(200));
        assert_eq!(b.delay_for(2), Duration::from_millis(400));
        assert_eq!(b.delay_for(3), Duration::from_millis(800));
    }

    #[test]
    fn test_backoff_delay_capped_at_max() {
        let b = BackoffOptions {
            max_attempts: 5,
            base: Duration::from_millis(100),
            multiplier: 10.0,
            max_delay: Duration::from_millis(500),
        };
        assert_eq!(b.delay_for(0), Duration::from_millis(100));
        assert_eq!(b.delay_for(1), Duration::from_millis(500)); // cap
        assert_eq!(b.delay_for(10), Duration::from_millis(500)); // cap
    }

    #[test]
    fn test_verify_outcome_serializes_as_kebab_tagged() {
        let s = serde_json::to_string(&VerifyOutcome::Ok).unwrap();
        assert_eq!(s, r#"{"kind":"ok"}"#);

        let s = serde_json::to_string(&VerifyOutcome::BrokenStatus { status: 404 }).unwrap();
        assert!(s.contains(r#""kind":"broken-status""#));
        assert!(s.contains(r#""status":404"#));
    }

    #[test]
    fn test_join_under_workspace_relative_path_joins() {
        let base = Path::new("/repo");
        assert_eq!(
            join_under_workspace(base, "docs/x.md"),
            Path::new("/repo/docs/x.md")
        );
    }

    #[test]
    fn test_join_under_workspace_leading_slash_treated_as_workspace_root() {
        let base = Path::new("/repo");
        // Per DESIGN §6.1 leading-slash semantics: `/path` anchors at
        // workspace root. We strip the leading `/` before joining.
        assert_eq!(
            join_under_workspace(base, "/docs/x.md"),
            Path::new("/repo/docs/x.md")
        );
    }

    #[test]
    fn test_verify_skipped_for_ifchange_kind() {
        let r = Reference {
            pattern_id: "ic".into(),
            pattern_kind: PatternKind::IfChange,
            file: "x.rs".into(),
            line: 1,
            column: 1,
            byte_start: 0,
            byte_end: 1,
            matched_text: "X".into(),
            captures: indexmap::IndexMap::new(),
            target: "anything".into(),
            title: None,
            in_comment: false,
        };
        let result = verify_reference(&r, &VerifyOptions::default()).unwrap();
        assert!(matches!(result, VerifyOutcome::Skipped { .. }));
    }

    #[test]
    fn test_verify_block_kind_returns_block_marker_with_matched_text() {
        let r = Reference {
            pattern_id: "block-default".into(),
            pattern_kind: PatternKind::Block,
            file: "x.rs".into(),
            line: 3,
            column: 1,
            byte_start: 0,
            byte_end: 12,
            matched_text: "DO NOT MERGE".into(),
            captures: indexmap::IndexMap::new(),
            target: "DO NOT MERGE".into(),
            title: None,
            in_comment: true,
        };
        let result = verify_reference(&r, &VerifyOptions::default()).unwrap();
        match result {
            VerifyOutcome::BlockMarker { matched_text } => {
                assert_eq!(matched_text, "DO NOT MERGE");
            }
            other => panic!("expected BlockMarker, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_outcome_block_marker_is_failure() {
        let o = VerifyOutcome::BlockMarker {
            matched_text: "NOCOMMIT".into(),
        };
        assert!(o.is_failure());
        assert!(!o.is_ok());
        assert!(!o.is_skipped());
    }

    #[test]
    fn test_verify_skipped_for_command_kind() {
        let r = Reference {
            pattern_id: "cmd".into(),
            pattern_kind: PatternKind::Command,
            file: "x.rs".into(),
            line: 1,
            column: 1,
            byte_start: 0,
            byte_end: 1,
            matched_text: "X".into(),
            captures: indexmap::IndexMap::new(),
            target: "anything".into(),
            title: None,
            in_comment: false,
        };
        let result = verify_reference(&r, &VerifyOptions::default()).unwrap();
        assert!(matches!(result, VerifyOutcome::Skipped { .. }));
    }
}
