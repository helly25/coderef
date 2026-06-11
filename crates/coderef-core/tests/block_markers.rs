//! End-to-end test: a `kind: "block"` pattern in a fixture workspace
//! is found by the scanner, classified as a `BlockMarker` outcome by
//! the verifier, and counted as broken by `check_references` — i.e.
//! `coderef check` would exit 1.

use coderef_core::check::check_references;
use coderef_core::config::Config;
use coderef_core::scan::scan_workspace;
use coderef_core::verify::{VerifyOptions, VerifyOutcome};
use std::fs;
use std::path::{Path, PathBuf};

fn tmpdir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "coderef-block-{label}-{pid}-{nanos}",
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

// Mirrors examples/block-markers.coderef.jsonc — the regex covers the
// six default spellings (NOCOMMIT / DONOTCOMMIT / DONOTMERGE / DO NOT
// {COMMIT,MERGE,SUBMIT}) and respects `commentsOnly: true`.
const BLOCK_CONFIG: &str = r#"
{
  "patterns": {
    "block-default": {
      "kind":  "block",
      "regex": "\\b(NOCOMMIT|DONOTCOMMIT|DONOTMERGE|DO\\s+NOT\\s+(COMMIT|MERGE|SUBMIT))\\b",
      "scope": { "commentsOnly": true }
    }
  }
}
"#;

#[test]
fn integration_block_marker_in_comment_is_broken_in_check_report() {
    let root = tmpdir("present");
    // One file with a DONOTMERGE in a comment — must surface as broken.
    write(
        &root,
        "src/lib.rs",
        "// DONOTMERGE — finish flake before push\nfn main() {}\n",
    );

    let cfg = Config::from_jsonc_str(BLOCK_CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    assert_eq!(refs.len(), 1, "expected one block-marker match");

    let opts = VerifyOptions {
        workspace_root: root.clone(),
        ..VerifyOptions::default()
    };
    let report = check_references(refs, &opts).unwrap();
    assert_eq!(report.total, 1);
    assert_eq!(report.broken, 1, "block marker must count as broken");
    assert_eq!(report.ok, 0);
    assert!(!report.passed(), "presence of block marker must fail check");
    match &report.results[0].outcome {
        VerifyOutcome::BlockMarker { matched_text } => {
            assert_eq!(matched_text, "DONOTMERGE");
        }
        other => panic!("expected BlockMarker, got {other:?}"),
    }

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_block_marker_outside_comment_is_filtered_by_scope() {
    let root = tmpdir("identifier");
    // `DONOTMERGE` appears only inside an identifier in real code — no
    // comment, no string. `commentsOnly: true` filters it out, so
    // `coderef check` passes.
    write(&root, "src/lib.rs", "fn DONOTMERGE_helper() -> u8 { 42 }\n");

    let cfg = Config::from_jsonc_str(BLOCK_CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    assert!(
        refs.is_empty(),
        "identifier match must be filtered by commentsOnly"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_clean_workspace_block_check_passes() {
    let root = tmpdir("clean");
    write(&root, "src/lib.rs", "// regular code\nfn main() {}\n");

    let cfg = Config::from_jsonc_str(BLOCK_CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    let opts = VerifyOptions {
        workspace_root: root.clone(),
        ..VerifyOptions::default()
    };
    let report = check_references(refs, &opts).unwrap();
    assert!(report.passed());

    fs::remove_dir_all(&root).unwrap();
}
