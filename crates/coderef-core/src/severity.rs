//! Diagnostic severity, used uniformly across config, doctor, and reports.
//! See `DESIGN.md` §9.1 for the canonical set.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity for a doctor check or pattern outcome.
///
/// Order is significant: a `max` over a set of severities is meaningful —
/// `Off < Hint < Info < Warning < Error`.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    /// The check is disabled; produces no diagnostic.
    Off,
    /// Informational; cosmetic / advisory.
    Hint,
    /// Worth noting but not a defect.
    #[default]
    Info,
    /// Likely a defect; surfaces in reports.
    Warning,
    /// Definite defect; fails the run.
    Error,
}

impl Severity {
    /// True iff the severity should fail an exit-code 1 sweep.
    #[must_use]
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Error)
    }

    /// True iff the severity is suppressed in default reports.
    #[must_use]
    pub fn is_suppressed(self) -> bool {
        matches!(self, Self::Off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering_off_is_lowest_and_error_is_highest() {
        assert!(Severity::Off < Severity::Hint);
        assert!(Severity::Hint < Severity::Info);
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn test_severity_default_is_info() {
        assert_eq!(Severity::default(), Severity::Info);
    }

    #[test]
    fn test_severity_is_failure_only_for_error() {
        assert!(Severity::Error.is_failure());
        assert!(!Severity::Warning.is_failure());
        assert!(!Severity::Info.is_failure());
        assert!(!Severity::Hint.is_failure());
        assert!(!Severity::Off.is_failure());
    }

    #[test]
    fn test_severity_is_suppressed_only_for_off() {
        assert!(Severity::Off.is_suppressed());
        assert!(!Severity::Hint.is_suppressed());
        assert!(!Severity::Info.is_suppressed());
        assert!(!Severity::Warning.is_suppressed());
        assert!(!Severity::Error.is_suppressed());
    }

    #[test]
    fn test_severity_serializes_as_kebab_case() {
        assert_eq!(serde_json::to_string(&Severity::Off).unwrap(), "\"off\"");
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"warning\""
        );
    }

    #[test]
    fn test_severity_deserializes_from_kebab_case() {
        let s: Severity = serde_json::from_str("\"error\"").unwrap();
        assert_eq!(s, Severity::Error);
    }
}
