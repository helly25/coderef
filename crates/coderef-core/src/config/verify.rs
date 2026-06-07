//! Verify-toggle config. See `DESIGN.md` §5.3, §13.3.
//!
//! Accepted as a boolean shorthand (`"verify": true`) or a structured
//! `VerifyToggle` object. v0.1 honours `enabled`; richer fields exist
//! for forward compatibility.

#[cfg(feature = "schemars")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Per-pattern verify configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(untagged)]
pub enum VerifyToggle {
    /// `"verify": true` / `"verify": false`.
    Bool(bool),
    /// Full structured toggle.
    Full(VerifyOptions),
    /// Empty object preserves "unspecified" semantics.
    #[default]
    None,
}

/// Structured verify configuration (the non-boolean shape).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(JsonSchema))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VerifyOptions {
    /// Whether verification runs at all. `None` means inherit defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Force a specific network profile for this verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,

    /// For multi-target patterns: whether this target must succeed for the
    /// reference to be considered verified (v0.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// HTTP method override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// HTTP statuses considered success.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accept_status: Vec<u16>,

    /// Per-target timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    /// Anchor verification mode (v0.2; §6.3.1, §13.3.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

impl VerifyToggle {
    /// Boolean view: `true` if verification is explicitly enabled,
    /// `false` if explicitly disabled, `None` if unspecified.
    #[must_use]
    pub fn enabled(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Full(opts) => opts.enabled,
            Self::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_toggle_bool_true_parses() {
        let v: VerifyToggle = serde_json::from_str("true").unwrap();
        assert_eq!(v.enabled(), Some(true));
    }

    #[test]
    fn test_verify_toggle_bool_false_parses() {
        let v: VerifyToggle = serde_json::from_str("false").unwrap();
        assert_eq!(v.enabled(), Some(false));
    }

    #[test]
    fn test_verify_toggle_full_object_with_enabled() {
        let v: VerifyToggle =
            serde_json::from_str(r#"{ "enabled": true, "profile": "office" }"#).unwrap();
        assert_eq!(v.enabled(), Some(true));
    }

    #[test]
    fn test_verify_toggle_empty_object_yields_none_enabled() {
        let v: VerifyToggle = serde_json::from_str("{}").unwrap();
        assert_eq!(v.enabled(), None);
    }

    #[test]
    fn test_verify_toggle_unknown_field_in_full_form_rejected() {
        let err =
            serde_json::from_str::<VerifyToggle>(r#"{ "enabled": true, "bogus": 1 }"#).unwrap_err();
        // VerifyToggle is untagged, so the error message hints at "data did not match any variant"
        assert!(err.to_string().contains("did not match"));
    }
}
