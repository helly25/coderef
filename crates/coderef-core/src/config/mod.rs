//! Configuration types and JSONC loader.
//!
//! `Config` is the top-level type matching the `$schema`-tagged
//! `.coderef.jsonc` files described in `DESIGN.md` §7. JSONC parsing (`//`
//! line comments, `/* */` block comments, trailing commas) is delegated to
//! the `jsonc-parser` crate.
//!
//! v0.1 foundation slice covers the subset of fields the v0.1 MVP needs:
//! `patterns` (url + local kinds, single-target), `variables`, `ignore`,
//! `workspaceRoot`, basic `scope.commentsOnly`. Fields tagged for v0.2+
//! are present in the types as `Option<…>` so configs that include them
//! parse without error; they are simply ignored by the v0.1 engine.

mod pattern;
mod resolve;
mod scope;
mod verify;

pub use self::pattern::{ActionConfig, ActionsConfig, Pattern, PatternKind, TargetSpec};
pub use self::resolve::{AnchorMode, CaseSensitivity, LocalResolveConfig};
pub use self::scope::ScopeConfig;
pub use self::verify::VerifyToggle;

use indexmap::IndexMap;
#[cfg(feature = "schemars")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

use crate::severity::Severity;

/// Top-level coderef configuration. See `DESIGN.md` §7.2 / §7.3.
///
/// `IndexMap` is used in place of `HashMap` so that pattern declaration
/// order is preserved — relevant for tie-breaking when two patterns share
/// the same `priority` (DESIGN.md §5.5, §9.2).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// JSON Schema URL for editor autocomplete. Ignored by the engine.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// User-defined variables (`${config:variables.x}`). See `DESIGN.md` §8.3.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variables: IndexMap<String, serde_json::Value>,

    /// Gitignore-style globs to exclude from scanning, applied repo-wide.
    /// See `DESIGN.md` §7.2.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,

    /// Map of pattern id → pattern definition. See `DESIGN.md` §5, §10.
    #[serde(default)]
    pub patterns: IndexMap<String, Pattern>,

    /// Override the auto-detected workspace root. Supports variables.
    #[serde(
        rename = "workspaceRoot",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub workspace_root: Option<String>,

    /// Workspace-level severity overrides for doctor checks.
    ///
    /// Resolution order is: per-pattern `Pattern.severity[check_id]`,
    /// then this map, then the check's hardcoded default. Use it to
    /// suppress a check across every pattern in the repo (`{
    /// "pattern.captureUnused": "off" }`), or escalate a check
    /// globally (`{ "pattern.unused": "error" }`) without sprinkling
    /// overrides on every pattern.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub severity: IndexMap<String, Severity>,
}

impl Config {
    /// Parse a config from a JSONC string.
    pub fn from_jsonc_str(src: &str) -> Result<Self, ConfigError> {
        let value: serde_json::Value = jsonc_parser::parse_to_serde_value(src, &Default::default())
            .map_err(|e| ConfigError::ParseJsonc(e.to_string()))?
            .ok_or(ConfigError::EmptyConfig)?;
        Self::from_value(value)
    }

    /// Parse a config from an already-deserialized `serde_json::Value`.
    pub fn from_value(value: serde_json::Value) -> Result<Self, ConfigError> {
        serde_json::from_value(value).map_err(|e| ConfigError::Deserialize(e.to_string()))
    }

    /// Convenience: read a JSONC file from disk and parse it.
    ///
    /// This is a host-side helper. The function reads a file from the
    /// local filesystem and is therefore unavailable on `wasm32` targets
    /// where `std::fs` is sandboxed. WASM hosts hand a buffer to
    /// [`Self::from_jsonc_str`] instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadFile(path.display().to_string(), e.to_string()))?;
        Self::from_jsonc_str(&src)
    }
}

/// Failures from config loading.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// I/O error reading the config file.
    #[error("failed to read config from `{0}`: {1}")]
    ReadFile(String, String),

    /// The file was empty or only whitespace/comments.
    #[error("config is empty (no JSON value)")]
    EmptyConfig,

    /// JSONC parsing failed (syntax error, unmatched bracket, etc.).
    #[error("failed to parse JSONC: {0}")]
    ParseJsonc(String),

    /// JSON value did not match the `Config` schema.
    #[error("config does not match expected shape: {0}")]
    Deserialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_empty_object_parses_with_defaults() {
        let cfg = Config::from_jsonc_str("{}").unwrap();
        assert!(cfg.patterns.is_empty());
        assert!(cfg.variables.is_empty());
        assert!(cfg.ignore.is_empty());
        assert!(cfg.workspace_root.is_none());
        assert!(cfg.schema.is_none());
    }

    #[test]
    fn test_config_strips_line_comments_and_trailing_commas() {
        let src = r#"
        {
            // This is a line comment.
            "ignore": [
                "node_modules/",   // another comment
            ],   // trailing comma above
        }
        "#;
        let cfg = Config::from_jsonc_str(src).unwrap();
        assert_eq!(cfg.ignore, vec!["node_modules/"]);
    }

    #[test]
    fn test_config_strips_block_comments() {
        let src = r#"
        {
            /* block comment
               spanning lines */
            "ignore": ["a"]
        }
        "#;
        let cfg = Config::from_jsonc_str(src).unwrap();
        assert_eq!(cfg.ignore, vec!["a"]);
    }

    #[test]
    fn test_config_empty_input_returns_empty_config_error() {
        let err = Config::from_jsonc_str("").unwrap_err();
        assert!(matches!(err, ConfigError::EmptyConfig));
    }

    #[test]
    fn test_config_malformed_jsonc_returns_parse_error() {
        let err = Config::from_jsonc_str("{ \"ignore\": [").unwrap_err();
        assert!(matches!(err, ConfigError::ParseJsonc(_)));
    }

    #[test]
    fn test_config_unknown_top_level_field_returns_deserialize_error() {
        let src = r#"{ "unknownField": 1 }"#;
        let err = Config::from_jsonc_str(src).unwrap_err();
        assert!(matches!(err, ConfigError::Deserialize(_)));
    }

    #[test]
    fn test_config_preserves_pattern_declaration_order() {
        let src = r#"
        {
            "patterns": {
                "bbb": { "regex": "BBB" },
                "aaa": { "regex": "AAA" },
                "ccc": { "regex": "CCC" }
            }
        }
        "#;
        let cfg = Config::from_jsonc_str(src).unwrap();
        let ids: Vec<&str> = cfg.patterns.keys().map(String::as_str).collect();
        assert_eq!(ids, vec!["bbb", "aaa", "ccc"]);
    }

    #[test]
    fn test_config_schema_field_is_preserved() {
        let src = r#"{ "$schema": "https://example.com/schema.json" }"#;
        let cfg = Config::from_jsonc_str(src).unwrap();
        assert_eq!(
            cfg.schema.as_deref(),
            Some("https://example.com/schema.json")
        );
    }
}
