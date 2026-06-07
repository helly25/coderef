//! Doctor end-to-end test: workspace-scope (includes `pattern.unused`).

use coderef_core::config::Config;
use coderef_core::doctor::run_doctor_with_workspace;
use std::fs;
use std::path::PathBuf;

fn tmpdir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "coderef-doctor-{label}-{pid}-{nanos}",
        pid = std::process::id(),
        nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn doctor_flags_pattern_unused_in_an_empty_workspace_as_info_not_warning() {
    use coderef_core::severity::Severity;
    let root = tmpdir("unused");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "todo": {
            "regex": "TODO\\(@(?<user>\\w+)\\)",
            "target": "x/${user}"
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.check == "pattern.unused" && d.pattern_id.as_deref() == Some("todo"))
        .expect("pattern.unused expected");
    // v0.2 change: pattern.unused is Info, not Warning, so shared/
    // template configs that declare patterns for repos that don't
    // use every one don't get noisy warnings by default. The hint
    // explains the escalation path.
    assert_eq!(diag.severity, Severity::Info, "diag: {diag:#?}");
    assert!(
        diag.message.contains("scanned"),
        "message lacks scan context: {}",
        diag.message
    );
    assert!(
        report.passed(),
        "Info-severity unused shouldn't fail passed()"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_pattern_with_matches_is_not_flagged_unused() {
    let root = tmpdir("used");
    fs::write(root.join("a.rs"), "// TODO(@alice)").unwrap();
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "todo": {
            "regex": "TODO\\(@(?<user>\\w+)\\)",
            "target": "x/${user}"
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let found = report
        .diagnostics
        .iter()
        .any(|d| d.check == "pattern.unused");
    assert!(
        !found,
        "unexpected unused diagnostic in: {:#?}",
        report.diagnostics
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_static_errors_surface_alongside_scan_dependent_warnings() {
    let root = tmpdir("mixed");
    // No source file → 'todo' will be unused. And 'bad' has an invalid
    // regex (static error). Both must be in the report.
    let cfg = Config::from_jsonc_str(
        r#"{
            "patterns": {
                "todo": {
                    "regex": "TODO\\(@(?<user>\\w+)\\)",
                    "target": "x/${user}"
                },
                "bad": { "regex": "(?<u>X", "target": "x" }
            }
        }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.check == "pattern.regexInvalid" && d.pattern_id.as_deref() == Some("bad")));
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.check == "pattern.unused" && d.pattern_id.as_deref() == Some("todo")));
    assert!(!report.passed()); // because of the regexInvalid Error.
    fs::remove_dir_all(&root).unwrap();
}
