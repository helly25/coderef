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

// ---------------------------------------------------------------------
// label.* doctor diagnostics (DESIGN §10.3). All three are
// scan-dependent and fire only when at least one `kind: "ifchange"`
// pattern is configured (the `ifchange_enabled` gate).
// ---------------------------------------------------------------------

#[test]
fn doctor_label_duplicate_in_file_fires_on_collision() {
    // Two `IfChange('k')` blocks in the same file — `ThenChange(a.py:k)`
    // would silently pick one. Flag as Error by default.
    let root = tmpdir("label-dup");
    write(
        &root,
        "a.py",
        "# IfChange('k')\nx = 1\n# ThenChange\n# IfChange('k')\ny = 2\n# ThenChange\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let dup = report
        .diagnostics
        .iter()
        .find(|d| d.check == "label.duplicateInFile");
    assert!(dup.is_some(), "got: {:#?}", report.diagnostics);
    assert_eq!(
        dup.unwrap().severity,
        coderef_core::severity::Severity::Error
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_duplicate_works_across_ifchange_and_label_compat_forms() {
    // Cross-form collision: one `IfChange('k')`, one `Label('k') ...
    // EndLabel`. Should still fire — both produce IfChangeBlocks with
    // id="k" in the same file.
    let root = tmpdir("label-dup-cross");
    write(
        &root,
        "a.py",
        "# IfChange('k')\nx = 1\n# ThenChange\n# Label('k')\ny = 2\n# EndLabel\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.check == "label.duplicateInFile"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_unused_fires_when_no_thenchange_references_the_label() {
    // A labelled block with no cross-file ThenChange(path:k) target
    // and no peer block sharing id="k". Advisory severity (Info).
    let root = tmpdir("label-unused");
    write(&root, "a.py", "# Label('lonely')\nx = 1\n# EndLabel\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let unused = report
        .diagnostics
        .iter()
        .find(|d| d.check == "label.unused");
    assert!(unused.is_some(), "got: {:#?}", report.diagnostics);
    assert_eq!(
        unused.unwrap().severity,
        coderef_core::severity::Severity::Info
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_unused_silent_when_a_then_change_references_it() {
    let root = tmpdir("label-used");
    // a.py: defines label `lonely`. b.py: ThenChange targets it.
    write(&root, "a.py", "# Label('lonely')\nx = 1\n# EndLabel\n");
    write(
        &root,
        "b.py",
        "# IfChange\ny = 2\n# ThenChange(a.py:lonely)\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report.diagnostics.iter().any(|d| d.check == "label.unused"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_unused_silent_when_peer_block_shares_id() {
    // Shape B / C peer matching: two blocks in different files sharing
    // id="k" form a peer group. Neither is "unused" even without a
    // FileLabel reference.
    let root = tmpdir("label-peer");
    write(&root, "a.py", "# IfChange('k')\nx = 1\n# ThenChange\n");
    write(&root, "b.py", "# IfChange('k')\ny = 2\n# ThenChange\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report.diagnostics.iter().any(|d| d.check == "label.unused"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_ambiguous_name_fires_on_pure_digits() {
    // `Label('42')`-style names collide with line/range syntax in
    // `ThenChange(path:N)` targets.
    let root = tmpdir("label-ambig-n");
    write(&root, "a.py", "# Label('42')\nx = 1\n# EndLabel\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let ambig = report
        .diagnostics
        .iter()
        .find(|d| d.check == "label.ambiguousName");
    assert!(ambig.is_some(), "got: {:#?}", report.diagnostics);
    assert_eq!(
        ambig.unwrap().severity,
        coderef_core::severity::Severity::Error
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_ambiguous_name_fires_on_range_form() {
    // `Label('5-10')` collides with the `path:N-M` line-range form.
    let root = tmpdir("label-ambig-range");
    write(&root, "a.py", "# Label('5-10')\nx = 1\n# EndLabel\n");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.check == "label.ambiguousName"));
    fs::remove_dir_all(&root).unwrap();
}

// ---------------------------------------------------------------------
// references.uncategorisedSpike (DESIGN §14.7.3). Advisory check that
// fires when more than 10% of references land in the `other` category.
// ---------------------------------------------------------------------

#[test]
fn doctor_references_uncategorised_spike_fires_at_above_10_percent() {
    // 4 of 10 refs in `other` (no category, kind=url defaults to
    // `other`), 6 in `people` — 40% > 10% threshold.
    let root = tmpdir("uncat-spike");
    for i in 0..4 {
        write(&root, &format!("o{i}.rs"), "// http://uncat.example/x");
    }
    for i in 0..6 {
        write(&root, &format!("p{i}.rs"), "// http://people.example/x");
    }
    let cfg = Config::from_jsonc_str(
        r#"{
            "patterns": {
                "uncat":  { "regex": "http://uncat\\.example/x",  "target": "static-target-u" },
                "people": { "regex": "http://people\\.example/x", "target": "static-target-p", "category": "people" }
            }
        }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.check == "references.uncategorisedSpike");
    assert!(diag.is_some(), "got: {:#?}", report.diagnostics);
    assert_eq!(
        diag.unwrap().severity,
        coderef_core::severity::Severity::Info
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_references_uncategorised_spike_silent_at_exactly_threshold() {
    // 1 of 10 = 10%, not strictly above. Threshold is `>`, not `>=`.
    let root = tmpdir("uncat-quiet");
    write(&root, "o0.rs", "// http://uncat.example/x");
    for i in 0..9 {
        write(&root, &format!("p{i}.rs"), "// http://people.example/x");
    }
    let cfg = Config::from_jsonc_str(
        r#"{
            "patterns": {
                "uncat":  { "regex": "http://uncat\\.example/x",  "target": "static-target-u" },
                "people": { "regex": "http://people\\.example/x", "target": "static-target-p", "category": "people" }
            }
        }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "references.uncategorisedSpike"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_references_uncategorised_spike_silent_with_no_refs() {
    // Empty workspace → empty refs → no division by zero, no
    // spurious diagnostic.
    let root = tmpdir("uncat-empty");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "todo": {
            "regex": "TODO\\(@(?<user>\\w+)\\)",
            "target": "x/${user}"
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "references.uncategorisedSpike"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_references_uncategorised_spike_off_severity_suppresses() {
    let root = tmpdir("uncat-off");
    for i in 0..4 {
        write(&root, &format!("o{i}.rs"), "// http://uncat.example/x");
    }
    for i in 0..6 {
        write(&root, &format!("p{i}.rs"), "// http://people.example/x");
    }
    let cfg = Config::from_jsonc_str(
        r#"{
            "severity": { "references.uncategorisedSpike": "off" },
            "patterns": {
                "uncat":  { "regex": "http://uncat\\.example/x",  "target": "static-target-u" },
                "people": { "regex": "http://people\\.example/x", "target": "static-target-p", "category": "people" }
            }
        }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "references.uncategorisedSpike"));
    fs::remove_dir_all(&root).unwrap();
}

// ---------------------------------------------------------------------
// commitMessage.requiredNeverFires (DESIGN §16.1.1). The check fires
// when a pattern declared `scope.commitMessage: "required"` doesn't
// match any commit message in the corpus the host supplies (typically
// the last N commit bodies). With no corpus (None / empty), the check
// is silently skipped — the host couldn't fetch one, and we'd rather
// be silent than flag every required pattern.
// ---------------------------------------------------------------------

#[test]
fn doctor_commit_required_never_fires_when_corpus_has_no_matches() {
    use coderef_core::doctor::run_doctor_with_workspace_and_commit_corpus;
    let root = tmpdir("cm-required-miss");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "jira": {
            "regex": "JIRA-\\d+",
            "target": "j/${0}",
            "scope": { "commitMessage": "required" }
        } } }"#,
    )
    .unwrap();
    let corpus: Vec<String> = ["fix: bump deps", "chore: rename", "docs: clarify"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let report = run_doctor_with_workspace_and_commit_corpus(&root, &cfg, Some(&corpus)).unwrap();
    let diag = report
        .diagnostics
        .iter()
        .find(|d| d.check == "commitMessage.requiredNeverFires");
    assert!(diag.is_some(), "got: {:#?}", report.diagnostics);
    assert_eq!(
        diag.unwrap().severity,
        coderef_core::severity::Severity::Warning
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_commit_required_silent_when_one_message_matches() {
    use coderef_core::doctor::run_doctor_with_workspace_and_commit_corpus;
    let root = tmpdir("cm-required-hit");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "jira": {
            "regex": "JIRA-\\d+",
            "target": "j/${0}",
            "scope": { "commitMessage": "required" }
        } } }"#,
    )
    .unwrap();
    let corpus: Vec<String> = ["fix JIRA-42: bump deps", "chore: rename"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let report = run_doctor_with_workspace_and_commit_corpus(&root, &cfg, Some(&corpus)).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "commitMessage.requiredNeverFires"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_commit_required_silent_when_corpus_is_empty() {
    // Empty corpus = host couldn't gather one. Don't flag.
    use coderef_core::doctor::run_doctor_with_workspace_and_commit_corpus;
    let root = tmpdir("cm-required-empty");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "jira": {
            "regex": "JIRA-\\d+",
            "target": "j/${0}",
            "scope": { "commitMessage": "required" }
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace_and_commit_corpus(&root, &cfg, Some(&[])).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "commitMessage.requiredNeverFires"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_commit_required_silent_when_corpus_is_none() {
    // None corpus = check doesn't run at all (back-compat with the
    // 2-arg `run_doctor_with_workspace`).
    let root = tmpdir("cm-required-none");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "jira": {
            "regex": "JIRA-\\d+",
            "target": "j/${0}",
            "scope": { "commitMessage": "required" }
        } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "commitMessage.requiredNeverFires"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_commit_required_silent_when_pattern_is_not_required() {
    // `scope.commitMessage: true` (Scan, not Required) shouldn't
    // trigger the never-fires check.
    use coderef_core::doctor::run_doctor_with_workspace_and_commit_corpus;
    let root = tmpdir("cm-scan-not-required");
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "jira": {
            "regex": "JIRA-\\d+",
            "target": "j/${0}",
            "scope": { "commitMessage": true }
        } } }"#,
    )
    .unwrap();
    let corpus: Vec<String> = vec!["fix: bump deps".to_string()];
    let report = run_doctor_with_workspace_and_commit_corpus(&root, &cfg, Some(&corpus)).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "commitMessage.requiredNeverFires"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn doctor_label_ambiguous_name_silent_on_alphanumeric_with_digits() {
    // `Label('block-1')` and `Label('q42')` are fine — they have at
    // least one non-digit character.
    let root = tmpdir("label-ambig-mixed");
    write(
        &root,
        "a.py",
        "# Label('block-1')\nx = 1\n# EndLabel\n# Label('q42')\ny = 2\n# EndLabel\n",
    );
    let cfg = Config::from_jsonc_str(
        r#"{ "patterns": { "ic": { "kind": "ifchange", "regex": "(unused)" } } }"#,
    )
    .unwrap();
    let report = run_doctor_with_workspace(&root, &cfg).unwrap();
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.check == "label.ambiguousName"));
    fs::remove_dir_all(&root).unwrap();
}
