//! End-to-end test: a fixture workspace with IfChange/ThenChange
//! blocks + a synthetic unified diff flows through `extract_blocks`
//! → `parse_unified_diff` → `verify_changes` and produces the
//! expected pass / fail.
//!
//! We don't shell out to `git` from the test — that would tie the
//! test to repo state. Instead we hand-craft the diff text the same
//! way `git diff -U0` would emit it.

use coderef_core::config::Config;
use coderef_core::ifchange::{parse_unified_diff, scan_workspace_blocks, verify_changes};
use std::fs;
use std::path::{Path, PathBuf};

fn tmpdir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "coderef-ifchange-{label}-{pid}-{nanos}",
        label = label,
        pid = std::process::id(),
        nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, content).unwrap();
}

const CONFIG: &str = r#"
{
  "patterns": {
    "ic": {
      "kind":  "ifchange",
      "regex": "(unused for ifchange)"
    }
  }
}
"#;

#[test]
fn integration_shape_a_missing_target_fails() {
    let root = tmpdir("shape-a-miss");
    write(
        &root,
        "src/hash.py",
        "# IfChange\nHASH = 'argon2id$...'\n# ThenChange(/docs/security.md)\n",
    );
    write(&root, "docs/security.md", "# Hashing\n");
    // Diff: only src/hash.py was changed (line 2). docs/security.md
    // was not.
    let diff = "\
+++ b/src/hash.py
@@ -2 +2 @@
-HASH = 'sha256$...'
+HASH = 'argon2id$...'
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert_eq!(errors, Vec::new());
    assert_eq!(blocks.len(), 1);
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert!(!report.passed(), "{report:#?}");
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].kind, "missing-target");
    assert!(report.violations[0].message.contains("/docs/security.md"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_shape_a_target_also_changed_passes() {
    let root = tmpdir("shape-a-ok");
    write(
        &root,
        "src/hash.py",
        "# IfChange\nHASH = 'argon2id$...'\n# ThenChange(/docs/security.md)\n",
    );
    write(&root, "docs/security.md", "# Hashing — argon2id now.\n");
    let diff = "\
+++ b/src/hash.py
@@ -2 +2 @@
-HASH = 'sha256$...'
+HASH = 'argon2id$...'
+++ b/docs/security.md
@@ -1 +1 @@
-# Hashing
+# Hashing — argon2id now.
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert!(errors.is_empty());
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert!(report.passed(), "{report:#?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_shape_b_peer_unchanged_fails() {
    let root = tmpdir("shape-b-miss");
    write(
        &root,
        "src/a.py",
        "# IfChange(auth-format-v3)\nFMT_A = 1\n# ThenChange\n",
    );
    write(
        &root,
        "src/b.py",
        "# IfChange(auth-format-v3)\nFMT_B = 2\n# ThenChange\n",
    );
    // Only src/a.py is changed at line 2.
    let diff = "\
+++ b/src/a.py
@@ -2 +2 @@
-FMT_A = 0
+FMT_A = 1
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert!(errors.is_empty());
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].kind, "missing-peer");
    assert!(report.violations[0].message.contains("src/b.py"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_no_verify_silences_violation() {
    let root = tmpdir("no-verify");
    write(
        &root,
        "src/a.py",
        "# NoVerify(coderef:ifchange): one-shot refactor, peer block lands later\n# IfChange(grp)\nX = 1\n# ThenChange\n",
    );
    write(&root, "src/b.py", "# IfChange(grp)\nY = 1\n# ThenChange\n");
    // Only src/a.py is changed at line 3.
    let diff = "\
+++ b/src/a.py
@@ -3 +3 @@
-X = 0
+X = 1
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert!(errors.is_empty());
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert!(report.passed(), "{report:#?}");
    assert_eq!(report.no_verify_block_count, 1);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_anchor_target_in_thenchange_passes_when_target_file_changed() {
    // ThenChange(/docs/security.md#hashing) on a block that's
    // changed; the diff also touches docs/security.md → satisfied.
    let root = tmpdir("anchor-pass");
    write(
        &root,
        "src/hash.py",
        "# IfChange\nHASH = 'argon2id'\n# ThenChange(/docs/security.md#hashing)\n",
    );
    write(&root, "docs/security.md", "# Hashing\n\nbody\n");
    let diff = "\
+++ b/src/hash.py
@@ -2 +2 @@
-HASH = 'sha256'
+HASH = 'argon2id'
+++ b/docs/security.md
@@ -3 +3 @@
-old
+new
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(blocks.len(), 1);
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert!(report.passed(), "{report:#?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_anchor_target_in_thenchange_fails_when_target_file_unchanged() {
    let root = tmpdir("anchor-fail");
    write(
        &root,
        "src/hash.py",
        "# IfChange\nHASH = 'argon2id'\n# ThenChange(/docs/security.md#hashing)\n",
    );
    write(&root, "docs/security.md", "# Hashing\n");
    let diff = "\
+++ b/src/hash.py
@@ -2 +2 @@
-HASH = 'sha256'
+HASH = 'argon2id'
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].kind, "missing-target");
    assert!(report.violations[0].message.contains("#hashing"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_label_target_resolves_to_named_block_and_passes() {
    let root = tmpdir("label-pass");
    // src/a.py block targets the `params` label in docs/x.md.
    // docs/x.md contains `IfChange('params')`.
    write(
        &root,
        "src/a.py",
        "# IfChange\nFOO = 1\n# ThenChange(/docs/x.md:params)\n",
    );
    write(
        &root,
        "docs/x.md",
        "# Section\n\n# IfChange('params')\nDETAIL = 2\n# ThenChange\n",
    );
    // Diff touches src/a.py line 2 and docs/x.md line 4 (inside the
    // 'params' block which spans lines 3..5).
    let diff = "\
+++ b/src/a.py
@@ -2 +2 @@
-FOO = 0
+FOO = 1
+++ b/docs/x.md
@@ -4 +4 @@
-DETAIL = 1
+DETAIL = 2
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert!(errors.is_empty(), "{errors:#?}");
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert!(report.passed(), "{report:#?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_label_target_diff_outside_named_block_fails() {
    let root = tmpdir("label-miss");
    write(
        &root,
        "src/a.py",
        "# IfChange\nFOO = 1\n# ThenChange(/docs/x.md:params)\n",
    );
    // Five preamble lines so the IfChange('params') block sits
    // at lines 6..8 — well past the diff's line 1 change.
    write(
        &root,
        "docs/x.md",
        "L1\nL2\nL3\nL4\nL5\n# IfChange('params')\nDETAIL = 2\n# ThenChange\n",
    );
    let diff = "\
+++ b/src/a.py
@@ -2 +2 @@
-FOO = 0
+FOO = 1
+++ b/docs/x.md
@@ -1 +1 @@
-L1
+L1 updated
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].kind, "missing-target");
    assert!(report.violations[0].message.contains(":params"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_shape_c_composable_id_groups_via_jira_pattern() {
    // Config declares an `ifchange` pattern AND a `jira` URL pattern.
    // Both blocks use `IfChange(JIRA(PROJ-1))`. Shape C resolves the
    // id through the JIRA pattern, so the two blocks group together.
    let root = tmpdir("shape-c-pass");
    let cfg_str = r#"
{
  "patterns": {
    "ic":   { "kind": "ifchange", "regex": "(unused)" },
    "jira": {
      "regex":  "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
      "target": "https://jira.example.com/${t}"
    }
  }
}
"#;
    write(
        &root,
        "src/a.py",
        "# IfChange(JIRA(PROJ-1))\nA = 1\n# ThenChange\n",
    );
    write(
        &root,
        "src/b.py",
        "# IfChange(JIRA(PROJ-1))\nB = 1\n# ThenChange\n",
    );
    // Both peers change → Shape C resolution groups them, peer
    // requirement is satisfied.
    let diff = "\
+++ b/src/a.py
@@ -2 +2 @@
-A = 0
+A = 1
+++ b/src/b.py
@@ -2 +2 @@
-B = 0
+B = 1
";
    let cfg = Config::from_jsonc_str(cfg_str).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    assert!(errors.is_empty(), "{errors:#?}");
    let cl = parse_unified_diff(diff);
    let resolver = |id: &str| coderef_core::ifchange::resolve_composable_id(&cfg, id);
    let report =
        coderef_core::ifchange::verify_changes_composable(&blocks, &errors, &cl, Some(&resolver));
    assert!(report.passed(), "{report:#?}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_shape_c_one_peer_unchanged_emits_missing_peer() {
    let root = tmpdir("shape-c-miss");
    let cfg_str = r#"
{
  "patterns": {
    "ic":   { "kind": "ifchange", "regex": "(unused)" },
    "jira": {
      "regex":  "JIRA\\((?<t>[A-Z]+-\\d+)\\)",
      "target": "https://jira.example.com/${t}"
    }
  }
}
"#;
    write(
        &root,
        "src/a.py",
        "# IfChange(JIRA(PROJ-9))\nA = 1\n# ThenChange\n",
    );
    write(
        &root,
        "src/b.py",
        "# IfChange(JIRA(PROJ-9))\nB = 1\n# ThenChange\n",
    );
    // Only src/a.py changes.
    let diff = "\
+++ b/src/a.py
@@ -2 +2 @@
-A = 0
+A = 1
";
    let cfg = Config::from_jsonc_str(cfg_str).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    let cl = parse_unified_diff(diff);
    let resolver = |id: &str| coderef_core::ifchange::resolve_composable_id(&cfg, id);
    let report =
        coderef_core::ifchange::verify_changes_composable(&blocks, &errors, &cl, Some(&resolver));
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].kind, "missing-peer");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_unrelated_diff_does_not_fire_blocks() {
    let root = tmpdir("noise");
    write(&root, "src/a.py", "# IfChange(grp)\nX = 1\n# ThenChange\n");
    write(&root, "src/b.py", "# IfChange(grp)\nY = 1\n# ThenChange\n");
    // Diff touches a file that has no IfChange block.
    let diff = "\
+++ b/src/unrelated.py
@@ -1 +1 @@
-x = 1
+x = 2
";
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let (blocks, errors) = scan_workspace_blocks(&root, &cfg).unwrap();
    let cl = parse_unified_diff(diff);
    let report = verify_changes(&blocks, &errors, &cl);
    assert!(report.passed(), "{report:#?}");
    assert_eq!(report.changed_block_count, 0);
    fs::remove_dir_all(&root).unwrap();
}
