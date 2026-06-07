//! Pattern definition types. See `DESIGN.md` §5.

use indexmap::IndexMap;
#[cfg(feature = "schemars")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::resolve::LocalResolveConfig;
use crate::config::scope::ScopeConfig;
use crate::config::verify::VerifyToggle;
use crate::severity::Severity;

/// Reference kinds. See `DESIGN.md` §5.2.
///
/// v0.1 implements `Url` and `Local`. `IfChange` is accepted by the schema
/// so v0.2 configs parse, but the v0.1 engine rejects it during scan.
/// `Command` is reserved for the post-v0.4 backlog.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PatternKind {
    /// Target is a URL string.
    #[default]
    Url,
    /// Target is a workspace-relative path resolved via §6.
    Local,
    /// Coupled-change marker pair (v0.2; see §10).
    IfChange,
    /// Custom command (post-v0.4 backlog).
    Command,
}

/// One reference pattern. See `DESIGN.md` §5.1, §5.3, §7.3.
///
/// Many fields are tagged for later versions; they are accepted by the
/// schema and stored on `Pattern` but not exercised by the v0.1 engine.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// Resolver kind. Defaults to `Url`. See `DESIGN.md` §5.2.
    #[serde(default)]
    pub kind: PatternKind,

    /// `fancy-regex`-compatible regex with named captures. See §5.1.
    pub regex: String,

    /// Free-form description of what this pattern is for and when it
    /// applies. Surfaces in `coderef patterns`, hover tooltips, and
    /// doctor diagnostics that name the pattern. Optional but strongly
    /// recommended for shared / template configs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Regex flags applied on top of the always-on `g` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<String>,

    /// Single-target shorthand (v0.1). Mutually exclusive with `targets`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    /// Multi-target list (v0.3+). See `DESIGN.md` §5.3.1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetSpec>,

    /// Hover / link title template. Supports variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Priority for tie-breaking overlapping matches. Default 0.
    /// See `DESIGN.md` §5.5, §9.2.
    #[serde(default)]
    pub priority: i32,

    /// Semantic category (v0.2; DESIGN.md §5.7). Free-form string;
    /// validated against the built-in + user-defined category set at
    /// engine load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    /// Scoping rules: where this pattern is applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeConfig>,

    /// Per-action overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<ActionsConfig>,

    /// Shorthand verify toggle. Either a boolean or a full `VerifyToggle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<VerifyToggle>,

    /// Local-path resolver config (only meaningful for `kind: "local"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve: Option<LocalResolveConfig>,

    /// Per-check severity overrides. Keys are check names; values are
    /// `Severity`. See `DESIGN.md` §5.4.3, §9.1.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub severity: IndexMap<String, Severity>,
}

/// One target in a multi-target pattern. See `DESIGN.md` §5.3.1 (v0.3).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    /// Display label for the hover / "Open with…" picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// URL template; supports variables.
    pub url: String,
    /// Priority; higher wins for primary. Default 0.
    #[serde(default)]
    pub priority: i32,
    /// Per-target verify override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<VerifyToggle>,
}

/// Per-pattern action overrides (open / preview / verify).
/// See `DESIGN.md` §5.3.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ActionsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<ActionConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<ActionConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<ActionConfig>,
}

/// One action. The full set of fields is accepted by the schema so v0.2+
/// configs parse; the v0.1 engine consumes only `kind` and `url`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ActionConfig {
    /// Action kind (e.g. `url`, `http`, `file`, `static`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// URL template (for `http` previews / verifies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Custom headers.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub headers: IndexMap<String, String>,
    /// HTTP method override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Accepted HTTP statuses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accept_status: Vec<u16>,
    /// Markdown template for HTTP-preview rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<String>,
    /// Per-action timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_minimal_only_regex_required() {
        let p: Pattern = serde_json::from_str(r#"{ "regex": "TODO" }"#).unwrap();
        assert_eq!(p.regex, "TODO");
        assert_eq!(p.kind, PatternKind::Url); // default
        assert_eq!(p.priority, 0);
        assert!(p.target.is_none());
    }

    #[test]
    fn test_pattern_kind_defaults_to_url() {
        let p: Pattern = serde_json::from_str(r#"{ "regex": "X" }"#).unwrap();
        assert_eq!(p.kind, PatternKind::Url);
    }

    #[test]
    fn test_pattern_kind_local_parses() {
        let p: Pattern = serde_json::from_str(r#"{ "regex": "X", "kind": "local" }"#).unwrap();
        assert_eq!(p.kind, PatternKind::Local);
    }

    #[test]
    fn test_pattern_kind_ifchange_parses_for_forward_compat() {
        let p: Pattern = serde_json::from_str(r#"{ "regex": "X", "kind": "ifchange" }"#).unwrap();
        assert_eq!(p.kind, PatternKind::IfChange);
    }

    #[test]
    fn test_pattern_unknown_top_level_field_rejected() {
        let err = serde_json::from_str::<Pattern>(r#"{ "regex": "X", "unknown": 1 }"#).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn test_pattern_target_and_targets_both_accepted_by_schema() {
        // v0.1 engine rejects this at compile time (DESIGN §9.1
        // targets.bothFieldsSet); the schema accepts both so doctor
        // produces a friendly error rather than a parse failure.
        let p: Pattern = serde_json::from_str(
            r#"{
                "regex": "X",
                "target": "https://a/",
                "targets": [{ "url": "https://b/" }]
            }"#,
        )
        .unwrap();
        assert!(p.target.is_some());
        assert_eq!(p.targets.len(), 1);
    }

    #[test]
    fn test_pattern_severity_map_parses_with_kebab_case_values() {
        let p: Pattern = serde_json::from_str(
            r#"{ "regex": "X", "severity": { "broken": "warning", "commitMessageMissing": "off" } }"#,
        )
        .unwrap();
        assert_eq!(p.severity.get("broken").copied(), Some(Severity::Warning));
        assert_eq!(
            p.severity.get("commitMessageMissing").copied(),
            Some(Severity::Off)
        );
    }
}
