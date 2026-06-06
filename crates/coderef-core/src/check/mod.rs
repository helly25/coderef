//! End-to-end pipeline: scan a workspace, verify every reference, and
//! return a structured summary suitable for either a human report or
//! the conformance harness's JSON diff (DESIGN.md §17.4).
//!
//! Serial verification only in v0.1; parallelism (rayon) is a known
//! follow-up. Verifies are inherently I/O-bound, so the simpler
//! implementation also keeps the test surface small until the v0.2
//! cache + per-profile config land.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

use crate::config::Config;
use crate::reference::Reference;
use crate::scan::{scan_workspace, WorkspaceScanError};
use crate::verify::{verify_reference, VerifyError, VerifyOptions, VerifyOutcome};

/// Outcome of checking the full workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckReport {
    /// Number of references scanned.
    pub total: usize,
    /// Number that verified successfully.
    pub ok: usize,
    /// Number that broke (any non-Ok / non-Skipped outcome).
    pub broken: usize,
    /// Number that were skipped (kind not yet implemented, verify
    /// disabled, …).
    pub skipped: usize,
    /// Per-reference results, in scan order.
    pub results: Vec<CheckResult>,
}

impl CheckReport {
    /// `true` iff every reference verified successfully or was skipped.
    /// What `coderef check` exits zero on.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.broken == 0
    }
}

/// One entry in `CheckReport::results`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub reference: Reference,
    pub outcome: VerifyOutcome,
}

/// Run the full pipeline against `root` with `config` + `opts`.
pub fn check_workspace(
    root: impl AsRef<Path>,
    config: &Config,
    opts: &VerifyOptions,
) -> Result<CheckReport, CheckError> {
    let refs = scan_workspace(root.as_ref(), config).map_err(CheckError::Scan)?;
    check_references(refs, opts)
}

/// Verify an explicit list of references (test-friendly seam).
pub fn check_references(
    refs: Vec<Reference>,
    opts: &VerifyOptions,
) -> Result<CheckReport, CheckError> {
    let mut results = Vec::with_capacity(refs.len());
    let mut ok = 0;
    let mut broken = 0;
    let mut skipped = 0;
    for reference in refs {
        let outcome = verify_reference(&reference, opts).map_err(CheckError::Verify)?;
        match &outcome {
            VerifyOutcome::Ok => ok += 1,
            VerifyOutcome::Skipped { .. } => skipped += 1,
            _ => broken += 1,
        }
        results.push(CheckResult { reference, outcome });
    }
    let total = results.len();
    Ok(CheckReport {
        total,
        ok,
        broken,
        skipped,
        results,
    })
}

/// Failures from `check_workspace`.
#[derive(Debug, Error)]
pub enum CheckError {
    #[error(transparent)]
    Scan(WorkspaceScanError),

    #[error(transparent)]
    Verify(VerifyError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PatternKind;
    use indexmap::IndexMap;

    fn r(id: &str, kind: PatternKind, target: &str) -> Reference {
        Reference {
            pattern_id: id.into(),
            pattern_kind: kind,
            file: "x.rs".into(),
            line: 1,
            column: 1,
            byte_start: 0,
            byte_end: 1,
            matched_text: "X".into(),
            captures: IndexMap::new(),
            target: target.into(),
            title: None,
            in_comment: false,
        }
    }

    #[test]
    fn test_check_report_passed_true_when_no_broken() {
        let report = CheckReport {
            total: 2,
            ok: 1,
            broken: 0,
            skipped: 1,
            results: vec![],
        };
        assert!(report.passed());
    }

    #[test]
    fn test_check_report_passed_false_when_any_broken() {
        let report = CheckReport {
            total: 2,
            ok: 1,
            broken: 1,
            skipped: 0,
            results: vec![],
        };
        assert!(!report.passed());
    }

    #[test]
    fn test_check_references_counts_ifchange_kind_as_skipped() {
        let refs = vec![r("ic", PatternKind::IfChange, "anything")];
        let opts = VerifyOptions::default();
        let report = check_references(refs, &opts).unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.ok, 0);
        assert_eq!(report.broken, 0);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn test_check_references_local_missing_path_counted_as_broken() {
        let tmp = std::env::temp_dir().join(format!(
            "coderef-check-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let opts = VerifyOptions {
            workspace_root: tmp.clone(),
            ..VerifyOptions::default()
        };
        let refs = vec![r("docref", PatternKind::Local, "missing.md")];
        let report = check_references(refs, &opts).unwrap();
        assert_eq!(report.broken, 1);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_check_references_local_existing_path_counted_as_ok() {
        let tmp = std::env::temp_dir().join(format!(
            "coderef-check-ok-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("present.md"), "x").unwrap();
        let opts = VerifyOptions {
            workspace_root: tmp.clone(),
            ..VerifyOptions::default()
        };
        let refs = vec![r("docref", PatternKind::Local, "present.md")];
        let report = check_references(refs, &opts).unwrap();
        assert_eq!(report.ok, 1);
        assert_eq!(report.broken, 0);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
