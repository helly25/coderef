//! Static + scan-dependent checks against a `Config`.
//!
//! Doctor surfaces configuration problems that would otherwise show up
//! only at scan / verify time, or that wouldn't show up at all and
//! would just silently misbehave. See `DESIGN.md` §9 for the full
//! v0.1+ check catalogue.
//!
//! v0.1 checks (this PR):
//!
//! - `pattern.targetMissing`        — neither `target` nor `targets[]` set
//! - `pattern.targetsBothFieldsSet` — both `target` and `targets[]` set
//! - `pattern.regexInvalid`         — `fancy-regex` couldn't compile the regex
//! - `pattern.captureUnknown`       — `${capture:X}` in target/title isn't a regex capture
//! - `pattern.captureUnused`        — regex captures `X` but nothing references it
//! - `pattern.variableConfigUnknown` — `${config:X}` references a missing variable
//! - `pattern.variableNamespaceFuture` — `${git:X}` / `${blame:X}` (v0.2 / v0.3)
//! - `pattern.unused`               — scan-dependent: pattern matched zero files
//! - `variable.invalidSyntax`       — template parser failed (also surfaces in
//!   scan/verify; doctor catches it ahead of time)
//!
//! Severity defaults follow `DESIGN.md` §9.1: configuration mistakes
//! that break the pattern are `Error`; lints (`unused`, future
//! namespace, unused capture) are `Warning`. Per-pattern severity
//! overrides via `pattern.severity[checkName]` are a v0.2 feature.

mod checks;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::severity::Severity;

/// One issue flagged by doctor.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable id of the check that produced this diagnostic, e.g.
    /// `"pattern.captureUnknown"`. Used as the key for per-pattern
    /// severity overrides (v0.2).
    pub check: String,
    /// Default severity (or the per-pattern override once §5.4.3
    /// wiring lands).
    pub severity: Severity,
    /// Pattern id this diagnostic refers to, if applicable.
    pub pattern_id: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Optional follow-up hint with a suggested fix.
    pub hint: Option<String>,
}

/// Aggregate doctor output.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorReport {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub hints: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl DoctorReport {
    /// `true` iff no diagnostic has `Severity::Error`.
    /// What `coderef doctor` exits zero on.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.errors == 0
    }

    fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Self {
        let total = diagnostics.len();
        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;
        let mut hints = 0;
        for d in &diagnostics {
            match d.severity {
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
                Severity::Info => infos += 1,
                Severity::Hint => hints += 1,
                Severity::Off => {} // shouldn't be emitted but ignore if so.
            }
        }
        Self {
            total,
            errors,
            warnings,
            infos,
            hints,
            diagnostics,
        }
    }
}

/// Run all static checks against `config`. Does not touch the file
/// system; the result is the same regardless of workspace contents.
#[must_use]
pub fn run_doctor(config: &Config) -> DoctorReport {
    let mut diagnostics = Vec::new();
    for (id, pattern) in &config.patterns {
        self::checks::check_pattern(id, pattern, config, &mut diagnostics);
    }
    diagnostics.sort_by(|a, b| {
        // Errors first, then by check id, then by pattern id.
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.check.cmp(&b.check))
            .then_with(|| a.pattern_id.cmp(&b.pattern_id))
    });
    DoctorReport::from_diagnostics(diagnostics)
}

/// Run static checks AND scan-dependent checks against a real
/// workspace. Adds `pattern.unused` for patterns with zero matches.
///
/// Doctor is resilient to invalid patterns: if any pattern's regex
/// doesn't compile, it's already been flagged by the static checks;
/// the scan-dependent pass simply skips that pattern instead of
/// aborting. So a broken `bad` pattern alongside an unused-in-this-
/// workspace `todo` produces *both* diagnostics in one report.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_doctor_with_workspace(
    root: impl AsRef<std::path::Path>,
    config: &Config,
) -> Result<DoctorReport, DoctorError> {
    let mut report = run_doctor(config);

    // Build a config containing only patterns whose regex compiles,
    // so a single bad pattern doesn't sink the whole scan.
    let mut scannable = config.clone();
    scannable.patterns.retain(|_id, p| {
        fancy_regex::Regex::new(&p.regex).is_ok() && (p.target.is_some() ^ !p.targets.is_empty())
    });

    let refs = crate::scan::scan_workspace(root.as_ref(), &scannable)
        .map_err(|e| DoctorError::Scan(e.to_string()))?;

    let total_refs = refs.len();
    let files_with_matches: std::collections::HashSet<&str> =
        refs.iter().map(|r| r.file.as_str()).collect();

    let mut counts_by_pattern: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for r in &refs {
        *counts_by_pattern.entry(r.pattern_id.as_str()).or_insert(0) += 1;
    }

    let mut additions: Vec<Diagnostic> = Vec::new();
    for (id, pattern) in &scannable.patterns {
        if !counts_by_pattern.contains_key(id.as_str()) {
            // Default severity is `Info` (shared / template configs
            // declare patterns that don't apply to every repo, and
            // that's not a defect). Per-pattern `severity` override
            // lets a strict user escalate to Warning / Error or
            // disable entirely with Off.
            let sev = self::checks::resolve_severity(pattern, "pattern.unused", Severity::Info);
            if sev == Severity::Off {
                continue;
            }
            // Multi-line message/hint: lines after the first are rendered
            // with a renderer-added indent (no leading whitespace in the
            // source). Doctor's text formatter handles alignment.
            let description_clause = pattern
                .description
                .as_deref()
                .map(|d| format!("\npattern description: {d}"))
                .unwrap_or_default();
            let message = format!(
                "pattern `{id}` matched no references in this workspace.\n\
                 workspace scan: {total_refs} reference(s) across {file_count} \
                 file(s) for the other patterns.{description_clause}",
                file_count = files_with_matches.len(),
            );
            additions.push(Diagnostic {
                check: "pattern.unused".into(),
                severity: sev,
                pattern_id: Some(id.clone()),
                message,
                hint: Some(
                    "if this is a shared / template config, leave it.\n\
                     otherwise:\n\
                       - remove the pattern,\n\
                       - tighten `scope.include` to a subtree where you expect matches, or\n\
                       - escalate the severity in your local config (the `severity` field on \
                     the pattern) to make the check fail."
                        .into(),
                ),
            });
        }
    }

    if !additions.is_empty() {
        let mut combined = std::mem::take(&mut report.diagnostics);
        combined.extend(additions);
        combined.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.check.cmp(&b.check))
                .then_with(|| a.pattern_id.cmp(&b.pattern_id))
        });
        report = DoctorReport::from_diagnostics(combined);
    }

    Ok(report)
}

/// Failures from `run_doctor_with_workspace`.
#[derive(Debug, thiserror::Error)]
pub enum DoctorError {
    #[error("doctor: scan failed: {0}")]
    Scan(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_jsonc(src: &str) -> Config {
        Config::from_jsonc_str(src).unwrap()
    }

    #[test]
    fn test_doctor_passes_on_a_clean_config() {
        let cfg = cfg_jsonc(
            r#"{
                "variables": { "base": "https://x" },
                "patterns": {
                    "todo": {
                        "regex": "TODO\\(@(?<user>\\w+)\\)",
                        "target": "${config:base}/${user}"
                    }
                }
            }"#,
        );
        let report = run_doctor(&cfg);
        assert!(report.passed(), "diagnostics: {:#?}", report.diagnostics);
        assert_eq!(report.errors, 0);
    }

    #[test]
    fn test_doctor_flags_pattern_missing_target() {
        let cfg = cfg_jsonc(r#"{ "patterns": { "x": { "regex": "X" } } }"#);
        let report = run_doctor(&cfg);
        assert_eq!(report.errors, 1);
        assert_eq!(report.diagnostics[0].check, "pattern.targetMissing");
        assert_eq!(report.diagnostics[0].pattern_id.as_deref(), Some("x"));
    }

    #[test]
    fn test_doctor_flags_pattern_targets_both_fields_set() {
        let cfg = cfg_jsonc(
            r#"{ "patterns": { "x": {
                "regex": "X",
                "target": "a",
                "targets": [{ "url": "b" }]
            } } }"#,
        );
        let report = run_doctor(&cfg);
        let found = report
            .diagnostics
            .iter()
            .any(|d| d.check == "pattern.targetsBothFieldsSet" && d.severity == Severity::Error);
        assert!(found, "got: {:#?}", report.diagnostics);
    }

    #[test]
    fn test_doctor_flags_invalid_regex_as_error() {
        let cfg = cfg_jsonc(
            r#"{ "patterns": { "x": {
                "regex": "TODO\\(@(?<user>\\w+\\)",
                "target": "x/${user}"
            } } }"#,
        );
        let report = run_doctor(&cfg);
        let found = report
            .diagnostics
            .iter()
            .any(|d| d.check == "pattern.regexInvalid" && d.severity == Severity::Error);
        assert!(found, "got: {:#?}", report.diagnostics);
    }

    #[test]
    fn test_doctor_flags_capture_unknown_in_target() {
        let cfg = cfg_jsonc(
            r#"{ "patterns": { "x": {
                "regex": "TODO\\(@(?<user>\\w+)\\)",
                "target": "x/${capture:notACapture}"
            } } }"#,
        );
        let report = run_doctor(&cfg);
        let found = report
            .diagnostics
            .iter()
            .any(|d| d.check == "pattern.captureUnknown");
        assert!(found, "got: {:#?}", report.diagnostics);
    }

    #[test]
    fn test_doctor_flags_unused_capture_as_warning() {
        let cfg = cfg_jsonc(
            r#"{ "patterns": { "x": {
                "regex": "TODO\\(@(?<user>\\w+)\\) (?<extra>\\S+)",
                "target": "x/${user}"
            } } }"#,
        );
        let report = run_doctor(&cfg);
        let extra = report
            .diagnostics
            .iter()
            .find(|d| d.check == "pattern.captureUnused")
            .expect("expected pattern.captureUnused");
        assert_eq!(extra.severity, Severity::Warning);
        assert!(extra.message.contains("extra"), "got: {}", extra.message);
    }

    #[test]
    fn test_doctor_flags_config_variable_unknown() {
        let cfg = cfg_jsonc(
            r#"{
                "variables": { "definedHere": "yes" },
                "patterns": { "x": {
                    "regex": "X",
                    "target": "${config:notDefinedHere}"
                } }
            }"#,
        );
        let report = run_doctor(&cfg);
        let found = report
            .diagnostics
            .iter()
            .any(|d| d.check == "pattern.variableConfigUnknown");
        assert!(found, "got: {:#?}", report.diagnostics);
    }

    #[test]
    fn test_doctor_flags_git_namespace_as_future() {
        let cfg = cfg_jsonc(
            r#"{ "patterns": { "x": {
                "regex": "X",
                "target": "x/${git:branch}"
            } } }"#,
        );
        let report = run_doctor(&cfg);
        let found = report
            .diagnostics
            .iter()
            .any(|d| d.check == "pattern.variableNamespaceFuture");
        assert!(found, "got: {:#?}", report.diagnostics);
    }

    #[test]
    fn test_doctor_flags_template_with_invalid_syntax() {
        let cfg = cfg_jsonc(
            r#"{ "patterns": { "x": {
                "regex": "X",
                "target": "x/${unclosed"
            } } }"#,
        );
        let report = run_doctor(&cfg);
        let found = report
            .diagnostics
            .iter()
            .any(|d| d.check == "variable.invalidSyntax");
        assert!(found, "got: {:#?}", report.diagnostics);
    }

    #[test]
    fn test_doctor_orders_errors_before_warnings() {
        let cfg = cfg_jsonc(
            r#"{
                "patterns": {
                    "noTarget": { "regex": "X" },
                    "unusedCap": {
                        "regex": "(?<unused>X)",
                        "target": "x"
                    }
                }
            }"#,
        );
        let report = run_doctor(&cfg);
        assert!(report.diagnostics.len() >= 2);
        // First diagnostic must be the Error.
        assert_eq!(report.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn test_doctor_passed_true_when_no_errors() {
        let report = DoctorReport {
            total: 1,
            errors: 0,
            warnings: 1,
            infos: 0,
            hints: 0,
            diagnostics: vec![],
        };
        assert!(report.passed());
    }

    #[test]
    fn test_doctor_passed_false_when_any_error() {
        let report = DoctorReport {
            total: 1,
            errors: 1,
            warnings: 0,
            infos: 0,
            hints: 0,
            diagnostics: vec![],
        };
        assert!(!report.passed());
    }
}
