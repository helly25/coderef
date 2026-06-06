//! Scoping configuration. See `DESIGN.md` §5.4.
//!
//! v0.1 honours `include`, `exclude`, `commentsOnly`. Full `prefix`
//! policy (§5.4.2) and `commitMessage` (§5.4.3) land in v0.2 — they are
//! present in the type for forward compatibility but ignored by the v0.1
//! engine.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a pattern is applied. See `DESIGN.md` §5.4.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
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

    /// Commit-message scope (v0.2; §5.4.3). Accepted for forward compat.
    /// Either a boolean or the string `"required"`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "commitMessage"
    )]
    pub commit_message: Option<serde_json::Value>,
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
}
