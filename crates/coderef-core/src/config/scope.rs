//! Scoping configuration. See `DESIGN.md` §5.4.
//!
//! v0.1 honoured `include`, `exclude`, `commentsOnly`. v0.2 adds
//! `commitMessage` semantics (§5.4.3, wired into `coderef commit-msg`).
//! `prefix` (§5.4.2) remains a forward-compat field accepted by the
//! schema but not yet exercised.

#[cfg(feature = "schemars")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a pattern is applied. See `DESIGN.md` §5.4.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScopeConfig {
    /// Gitignore-style globs to include. Empty = include everything.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,

    /// Gitignore-style globs to exclude. Empty = no extra exclusions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// If true, only match inside detected comment regions. See §5.4.1.
    #[serde(default, skip_serializing_if = "is_default", rename = "commentsOnly")]
    pub comments_only: bool,

    /// Full prefix policy (v0.2; §5.4.2). Accepted for forward compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<serde_json::Value>,

    /// Commit-message scope (§5.4.3). `true` / `false` / `"required"`.
    /// `None` = use the kind-based default (`true` for url/local;
    /// `false` for ifchange/block/command).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "commitMessage"
    )]
    pub commit_message: Option<CommitMessageScope>,
}

/// Per-pattern commit-message scope. See `DESIGN.md` §5.4.3.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(untagged)]
pub enum CommitMessageScope {
    /// `true` — pattern scans commit messages. `false` — pattern does
    /// NOT scan commit messages.
    Bool(bool),
    /// `"required"` — every commit message must contain at least one
    /// match of this pattern. Missing matches are reported as a
    /// `commitMessageMissing` diagnostic on the pattern.
    Tag(CommitMessageTag),
}

/// String tag for `CommitMessageScope::Tag`. Single-variant enum so
/// serde can distinguish it from `Bool` in the untagged form.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CommitMessageTag {
    /// `"required"` — pattern must match every commit message.
    Required,
}

/// Effective commit-message scope after applying kind-based defaults.
///
/// `commit_msg::effective_scope` resolves a Pattern → one of these;
/// the type lives here (alongside the config struct) so doctor and
/// other WASM-safe consumers can use it without dragging in the
/// `commit_msg` verifier (which is host-only).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveCommitMessageScope {
    /// Pattern scans commit messages.
    Scan,
    /// Pattern does NOT scan commit messages.
    Skip,
    /// Pattern scans AND must produce at least one match.
    Required,
}

/// Resolve `scope.commitMessage` to its effective value.
///
/// Per DESIGN §5.4.3, defaults are kind-based when undeclared:
/// `url` / `local` → Scan; everything else → Skip.
#[must_use]
pub fn resolve_commit_message_scope(pat: &super::pattern::Pattern) -> EffectiveCommitMessageScope {
    let declared = pat.scope.as_ref().and_then(|s| s.commit_message);
    match declared {
        Some(CommitMessageScope::Bool(true)) => EffectiveCommitMessageScope::Scan,
        Some(CommitMessageScope::Bool(false)) => EffectiveCommitMessageScope::Skip,
        Some(CommitMessageScope::Tag(CommitMessageTag::Required)) => {
            EffectiveCommitMessageScope::Required
        }
        None => match pat.kind {
            super::pattern::PatternKind::Url | super::pattern::PatternKind::Local => {
                EffectiveCommitMessageScope::Scan
            }
            // ifchange, block, command don't translate to single-message
            // scans — DESIGN §5.4.3 defaults table.
            _ => EffectiveCommitMessageScope::Skip,
        },
    }
}

fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    *t == T::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_empty_object_yields_default() {
        let s: ScopeConfig = serde_json::from_str("{}").unwrap();
        assert!(s.include.is_empty());
        assert!(s.exclude.is_empty());
        assert!(!s.comments_only);
    }

    #[test]
    fn test_scope_comments_only_uses_camel_case_key() {
        let s: ScopeConfig = serde_json::from_str(r#"{ "commentsOnly": true }"#).unwrap();
        assert!(s.comments_only);
    }

    #[test]
    fn test_scope_unknown_field_rejected() {
        let err = serde_json::from_str::<ScopeConfig>(r#"{ "bogus": 1 }"#).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn test_scope_v0_2_fields_accepted_for_forward_compat() {
        let s: ScopeConfig = serde_json::from_str(
            r#"{
                "prefix": { "require": "comment" },
                "commitMessage": "required"
            }"#,
        )
        .unwrap();
        assert!(s.prefix.is_some());
        assert!(s.commit_message.is_some());
    }

    #[test]
    fn test_scope_commit_message_parses_bool_true() {
        let s: ScopeConfig = serde_json::from_str(r#"{ "commitMessage": true }"#).unwrap();
        assert_eq!(s.commit_message, Some(CommitMessageScope::Bool(true)));
    }

    #[test]
    fn test_scope_commit_message_parses_bool_false() {
        let s: ScopeConfig = serde_json::from_str(r#"{ "commitMessage": false }"#).unwrap();
        assert_eq!(s.commit_message, Some(CommitMessageScope::Bool(false)));
    }

    #[test]
    fn test_scope_commit_message_parses_required_tag() {
        let s: ScopeConfig = serde_json::from_str(r#"{ "commitMessage": "required" }"#).unwrap();
        assert_eq!(
            s.commit_message,
            Some(CommitMessageScope::Tag(CommitMessageTag::Required))
        );
    }

    #[test]
    fn test_scope_commit_message_rejects_unknown_string() {
        let err =
            serde_json::from_str::<ScopeConfig>(r#"{ "commitMessage": "bogus" }"#).unwrap_err();
        // The exact wording depends on serde; the contract is that
        // it does not silently coerce to Bool(true) or anything else.
        assert!(
            err.to_string().contains("data did not match")
                || err.to_string().contains("unknown variant")
                || err.to_string().contains("commitMessage"),
            "got: {err}"
        );
    }
}
