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
        diag.message.contains("workspace scan"),
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
fn doctor_pattern_severity_override_escalates_unused_to_error() {
    use coderef_core::severity::Severity;
    let root = tmpdir("escalate");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "todo": {
            "regex": "TODO\\(@(?<user>\\w+)\\)",
            "target": "x/${user}",
            "severity": { "pattern.unused": "error" }
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.check == "pattern.unused")
        .expect("expected pattern.unused diagnostic");
    assert_eq!(diag.severity, Severity::Error, "diag: {diag:#?}");
    // With an Error-severity diagnostic, the report must fail.
    assert!(
        !report.passed(),
        "Error-escalated pattern.unused should fail passed()"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_pattern_severity_override_off_suppresses_diagnostic() {
    let root = tmpdir("suppress");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "todo": {
            "regex": "TODO\\(@(?<user>\\w+)\\)",
            "target": "x/${user}",
            "severity": { "pattern.unused": "off" }
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
        "severity: off must suppress emission entirely; got: {:#?}",
        report.diagnostics
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_pattern_severity_override_demotes_regex_invalid_from_error_to_warning() {
    use coderef_core::severity::Severity;
    let root = tmpdir("demote");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "x": {
            "regex": "(?<u>X",
            "target": "x",
            "severity": { "pattern.regexInvalid": "warning" }
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.check == "pattern.regexInvalid")
        .expect("expected pattern.regexInvalid diagnostic");
    assert_eq!(diag.severity, Severity::Warning, "diag: {diag:#?}");
    // Warning-only → exit 0.
    assert!(
        report.passed(),
        "Warning-only report should pass — got: {:#?}",
        report.diagnostics
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_workspace_severity_override_demotes_all_patterns_unused_to_off() {
    let root = tmpdir("ws-off");
    let cfg = Config::from_jsonc_str(
        r#"{
            "severity": { "pattern.unused": "off" },
            "patterns": {
                "todo": {
                    "regex": "TODO\\(@(?<user>\\w+)\\)",
                    "target": "x/${user}"
                },
                "jira": {
                    "regex": "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
                    "target": "j/${t}"
                }
            }
        }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let found = report
        .diagnostics
        .iter()
        .any(|d| d.check == "pattern.unused");
    assert!(
        !found,
        "workspace severity off must suppress pattern.unused for every pattern; got: {:#?}",
        report.diagnostics
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_workspace_severity_override_is_overridden_by_per_pattern() {
    use coderef_core::severity::Severity;
    let root = tmpdir("layered");
    let cfg = Config::from_jsonc_str(
        r#"{
            "severity": { "pattern.unused": "off" },
            "patterns": {
                "strict-todo": {
                    "regex": "TODO\\(@(?<user>\\w+)\\)",
                    "target": "x/${user}",
                    "severity": { "pattern.unused": "error" }
                },
                "lax-jira": {
                    "regex": "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
                    "target": "j/${t}"
                }
            }
        }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    // strict-todo's per-pattern Error override wins over the workspace
    // Off; lax-jira gets the workspace Off.
    let strict = report
        .diagnostics
        .iter()
        .find(|d| d.check == "pattern.unused" && d.pattern_id.as_deref() == Some("strict-todo"));
    let lax = report
        .diagnostics
        .iter()
        .find(|d| d.check == "pattern.unused" && d.pattern_id.as_deref() == Some("lax-jira"));
    assert_eq!(
        strict.map(|d| d.severity),
        Some(Severity::Error),
        "per-pattern Error should win over workspace Off",
    );
    assert!(
        lax.is_none(),
        "no per-pattern override → workspace Off applies; got: {:#?}",
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

fn write(root: &std::path::Path, rel: &str, content: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, content).unwrap();
}

#[test]
fn doctor_category_mismatch_fires_when_matches_consistently_use_at_sigil() {
    // Pattern declares category: "urls" but every match starts with
    // `@` (looks like people). Should produce category.mismatch.
    let root = tmpdir("cat-mismatch");
    write(
        &root,
        "src/a.rs",
        "// TODO(@alice)\n// TODO(@bob)\n// TODO(@carol)\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "todo": {
                "regex":    "TODO\\(@(?<user>\\w+)\\)",
                "target":   "https://x/${user}",
                "category": "urls"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let found = report
        .diagnostics
        .iter()
        .any(|d| d.check == "category.mismatch" && d.pattern_id.as_deref() == Some("todo"));
    assert!(found, "got: {:#?}", report.diagnostics);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_category_mismatch_silent_when_category_matches_sigil() {
    let root = tmpdir("cat-match");
    write(
        &root,
        "src/a.rs",
        "// TODO(@alice)\n// TODO(@bob)\n// TODO(@carol)\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "todo": {
                "regex":    "TODO\\(@(?<user>\\w+)\\)",
                "target":   "https://x/${user}",
                "category": "people"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "category.mismatch"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_category_mismatch_silent_when_below_min_samples() {
    // Only 2 matches → below the 3-sample minimum.
    let root = tmpdir("cat-fewsamples");
    write(&root, "src/a.rs", "// TODO(@alice)\n// TODO(@bob)\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "todo": {
                "regex":    "TODO\\(@(?<user>\\w+)\\)",
                "target":   "https://x/${user}",
                "category": "urls"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "category.mismatch"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_anchor_skipped_ext_fires_on_non_markdown_target_with_anchor() {
    let root = tmpdir("skip-ext");
    write(&root, "docs/notes.txt", "any\n");
    write(&root, "src/a.rs", "// DOCREF(/docs/notes.txt#section)\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "docref": {
                "kind":   "local",
                "regex":  "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
                "target": "${path}#${anchor}"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let found = report
        .diagnostics
        .iter()
        .any(|d| d.check == "anchor.skippedExt" && d.pattern_id.as_deref() == Some("docref"));
    assert!(found, "got: {:#?}", report.diagnostics);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_anchor_skipped_ext_silent_for_markdown_target_with_anchor() {
    let root = tmpdir("skip-md");
    write(&root, "docs/notes.md", "# Section\n");
    write(&root, "src/a.rs", "// DOCREF(/docs/notes.md#section)\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "docref": {
                "kind":   "local",
                "regex":  "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
                "target": "${path}#${anchor}"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "anchor.skippedExt"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_anchor_style_mismatch_fires_when_target_mixes_explicit_and_implicit_headings() {
    let root = tmpdir("style-mix");
    write(
        &root,
        "docs/x.md",
        "# Plain Heading\n\n## Explicit {#custom-id}\n",
    );
    write(&root, "src/a.rs", "// DOCREF(/docs/x.md#plain-heading)\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "docref": {
                "kind":   "local",
                "regex":  "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
                "target": "${path}#${anchor}"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let found = report
        .diagnostics
        .iter()
        .any(|d| d.check == "anchor.styleMismatch");
    assert!(found, "got: {:#?}", report.diagnostics);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_anchor_style_mismatch_silent_when_pattern_uses_pandoc_slugifier() {
    let root = tmpdir("style-pandoc");
    write(
        &root,
        "docs/x.md",
        "# Plain Heading\n\n## Explicit {#custom-id}\n",
    );
    write(&root, "src/a.rs", "// DOCREF(/docs/x.md#plain-heading)\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "docref": {
                "kind":   "local",
                "regex":  "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
                "target": "${path}#${anchor}",
                "resolve": { "slugifier": "pandoc" }
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "anchor.styleMismatch"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_coupled_composable_typo_fires_when_id_almost_matches_pattern() {
    // JIRA pattern accepts uppercase-prefix-digits. The block uses
    // `JIRA(PROJ1234)` — missing the `-` between prefix and number.
    // One-edit insertion away from `JIRA(PROJ-1234)`, which the
    // pattern accepts.
    let root = tmpdir("typo");
    write(
        &root,
        "src/a.py",
        "# IfChange(JIRA(PROJ1234))\nX = 1\n# ThenChange\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "ic":   { "kind": "ifchange", "regex": "(unused)" },
            "jira": {
                "regex":  "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
                "target": "https://jira.example/${t}"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let found = report
        .diagnostics
        .iter()
        .any(|d| d.check == "coupled.composableTypo");
    assert!(found, "got: {:#?}", report.diagnostics);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_coupled_composable_typo_silent_when_id_resolves() {
    let root = tmpdir("typo-clean");
    write(
        &root,
        "src/a.py",
        "# IfChange(JIRA(PROJ-1234))\nX = 1\n# ThenChange\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": {
            "ic":   { "kind": "ifchange", "regex": "(unused)" },
            "jira": {
                "regex":  "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
                "target": "https://jira.example/${t}"
            }
        } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "coupled.composableTypo"));
    fs::remove_dir_all(&root).unwrap();
}
