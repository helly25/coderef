//! End-to-end pipeline test: real workspace on disk + local-path
//! verifier. HTTP verification is covered by unit tests with a mock
//! server in `verify::http::tests`.

use coderef_core::check::check_workspace;
use coderef_core::config::Config;
use coderef_core::verify::VerifyOptions;
use std::fs;
use std::path::{Path, PathBuf};

fn tmpdir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "coderef-pipeline-{label}-{pid}-{nanos}",
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

const CFG: &str = r#"
{
  "patterns": {
    "docref": {
      "regex":  "DOCREF\\((?<path>/?[^)\\s#]+)\\)",
      "kind":   "local",
      "target": "${path}",
      "scope":  { "commentsOnly": true }
    }
  }
}
"#;

fn opts_for(root: &Path) -> VerifyOptions {
    VerifyOptions {
        workspace_root: root.to_path_buf(),
        ..Default::default()
    }
}

#[test]
fn pipeline_mixed_existing_and_missing_local_refs() {
    let root = tmpdir("mixed");
    write(&root, "docs/real.md", "x");
    write(&root, "src/a.rs", "// DOCREF(/docs/real.md)");
    write(&root, "src/b.rs", "// DOCREF(/docs/missing.md)");

    let cfg = Config::from_jsonc_str(CFG).unwrap();
    let report = check_workspace(&root, &cfg, &opts_for(&root)).unwrap();

    assert_eq!(report.total, 2);
    assert_eq!(report.ok, 1);
    assert_eq!(report.broken, 1);
    assert_eq!(report.skipped, 0);
    assert!(!report.passed());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn pipeline_passes_when_all_local_refs_resolve() {
    let root = tmpdir("pass");
    write(&root, "docs/one.md", "x");
    write(&root, "docs/two.md", "y");
    write(&root, "a.rs", "// DOCREF(/docs/one.md)");
    write(&root, "b.rs", "// DOCREF(/docs/two.md)");

    let cfg = Config::from_jsonc_str(CFG).unwrap();
    let report = check_workspace(&root, &cfg, &opts_for(&root)).unwrap();
    assert_eq!(report.ok, 2);
    assert_eq!(report.broken, 0);
    assert!(report.passed());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn pipeline_empty_workspace_passes_trivially() {
    let root = tmpdir("empty");
    let cfg = Config::from_jsonc_str(CFG).unwrap();
    let report = check_workspace(&root, &cfg, &opts_for(&root)).unwrap();
    assert_eq!(report.total, 0);
    assert!(report.passed());
    fs::remove_dir_all(&root).unwrap();
}
