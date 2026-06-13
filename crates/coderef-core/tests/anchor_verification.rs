//! Workspace-level integration test for Markdown anchor verification
//! (DESIGN.md §6.3). Builds a fixture workspace with a config whose
//! target template threads the captured `${anchor}` into the resolved
//! path; runs the full scan + verify pipeline; asserts that:
//!
//! - A reference whose anchor matches a heading slug verifies OK.
//! - A reference whose anchor misses produces an `AnchorNotFound`
//!   outcome with a Levenshtein-1 suggestion.
//! - A reference with no `#anchor` portion still verifies (the
//!   existing path-existence semantics).

use coderef_core::check::check_references;
use coderef_core::config::Config;
use coderef_core::scan::scan_workspace;
use coderef_core::verify::{VerifyOptions, VerifyOutcome};
use std::fs;
use std::path::{Path, PathBuf};

fn tmpdir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "coderef-anchor-int-{label}-{pid}-{nanos}",
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

// DOCREF pattern that threads ${anchor} into the resolved target. The
// non-strict variable resolver substitutes the empty string for an
// unset capture, so `DOCREF(/docs/x.md)` resolves to `/docs/x.md#`,
// which the verifier treats the same as `/docs/x.md` (no anchor).
const CONFIG: &str = r#"
{
  "patterns": {
    "docref": {
      "regex":  "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
      "kind":   "local",
      "target": "${path}#${anchor}"
    }
  }
}
"#;

#[test]
fn integration_anchor_present_and_matching_verifies_ok() {
    let root = tmpdir("hit");
    write(
        &root,
        "docs/security.md",
        "# Hashing\n\n## Hash Params\n\nbody\n",
    );
    // The DOCREF reference is in a Rust source file so the scanner
    // picks it up under default scope (no commentsOnly filter).
    write(
        &root,
        "src/code.rs",
        "// DOCREF(/docs/security.md#hash-params) hashes are here\n",
    );

    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    let opts = VerifyOptions {
        workspace_root: root.clone(),
        ..VerifyOptions::default()
    };
    let report = check_references(refs, &opts).unwrap();
    assert!(
        report.passed(),
        "expected passed; results: {:#?}",
        report.results
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_anchor_present_but_missing_returns_anchor_not_found_with_suggestion() {
    let root = tmpdir("miss");
    write(&root, "docs/x.md", "# Hashing\n");
    write(
        &root,
        "src/code.rs",
        "// DOCREF(/docs/x.md#hashin) typo'd reference\n",
    );

    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    let opts = VerifyOptions {
        workspace_root: root.clone(),
        ..VerifyOptions::default()
    };
    let report = check_references(refs, &opts).unwrap();
    assert_eq!(report.broken, 1, "{:#?}", report.results);
    match &report.results[0].outcome {
        VerifyOutcome::AnchorNotFound {
            anchor, suggestion, ..
        } => {
            assert_eq!(anchor, "hashin");
            assert_eq!(suggestion.as_deref(), Some("hashing"));
        }
        other => panic!("expected AnchorNotFound, got {other:?}"),
    }
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_no_anchor_in_reference_still_passes_when_file_exists() {
    let root = tmpdir("noanchor");
    write(&root, "docs/x.md", "# Whatever\n");
    write(
        &root,
        "src/code.rs",
        "// DOCREF(/docs/x.md) plain reference, no anchor\n",
    );

    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    let opts = VerifyOptions {
        workspace_root: root.clone(),
        ..VerifyOptions::default()
    };
    let report = check_references(refs, &opts).unwrap();
    assert!(report.passed(), "{:#?}", report.results);
    fs::remove_dir_all(&root).unwrap();
}
