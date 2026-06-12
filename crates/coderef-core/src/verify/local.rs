//! Local-path verification.
//!
//! Workspace-anchored: `/path` and `path` both resolve under the
//! workspace root (DESIGN.md §6.1 default anchor mode `workspace`).
//! v0.2 adds Markdown anchor verification (`#anchor` suffix on the
//! target); see DESIGN.md §6.3 and `crate::anchor` for the slug
//! algorithm + heading extractor.

use std::path::Path;

use super::{join_under_workspace, VerifyOutcome};
use crate::anchor::{verify_anchor, AnchorOutcome};

pub fn verify_local(target: &str, workspace_root: &Path) -> VerifyOutcome {
    // Split off a trailing `#anchor` if present. Filesystem paths
    // can't contain `#` on disk in any meaningful sense (the engine
    // would never resolve one), so the first `#` unambiguously marks
    // the anchor boundary.
    let (path_part, anchor) = match target.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (target, None),
    };

    let resolved = join_under_workspace(workspace_root, path_part);
    if !resolved.exists() {
        return VerifyOutcome::NotFound {
            path: resolved.to_string_lossy().to_string(),
        };
    }

    // No anchor → file existence is enough. Empty anchor (e.g.
    // `path#`) is treated the same; it's effectively "no anchor".
    let Some(anchor) = anchor.filter(|a| !a.is_empty()) else {
        return VerifyOutcome::Ok;
    };

    match verify_anchor(&resolved, anchor, Some("github")) {
        AnchorOutcome::Found | AnchorOutcome::Skipped { .. } => VerifyOutcome::Ok,
        AnchorOutcome::NotFound {
            suggestion,
            available_sample: _,
        } => VerifyOutcome::AnchorNotFound {
            path: resolved.to_string_lossy().to_string(),
            anchor: anchor.to_string(),
            suggestion,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(label: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "coderef-local-{label}-{pid}-{nanos}",
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
    fn test_local_existing_relative_path_is_ok() {
        let root = tmpdir("exists");
        fs::write(root.join("a.md"), "x").unwrap();
        assert_eq!(verify_local("a.md", &root), VerifyOutcome::Ok);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_existing_leading_slash_path_resolves_under_root() {
        let root = tmpdir("slash");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/a.md"), "x").unwrap();
        assert_eq!(verify_local("/docs/a.md", &root), VerifyOutcome::Ok);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_missing_path_is_not_found_with_resolved_path() {
        let root = tmpdir("missing");
        match verify_local("nope.md", &root) {
            VerifyOutcome::NotFound { path } => {
                assert!(path.ends_with("nope.md"), "got: {path}");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_existing_directory_is_ok() {
        let root = tmpdir("dir");
        fs::create_dir_all(root.join("subdir")).unwrap();
        assert_eq!(verify_local("subdir", &root), VerifyOutcome::Ok);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_with_anchor_match_is_ok() {
        let root = tmpdir("anchor-ok");
        fs::write(root.join("doc.md"), "# Hashing\n\nbody\n## Argon2 Params\n").unwrap();
        assert_eq!(
            verify_local("doc.md#argon2-params", &root),
            VerifyOutcome::Ok
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_with_anchor_miss_returns_anchor_not_found_with_suggestion() {
        let root = tmpdir("anchor-miss");
        fs::write(root.join("doc.md"), "# Hashing\n").unwrap();
        match verify_local("doc.md#hashin", &root) {
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
    fn test_local_with_empty_anchor_after_hash_treats_as_no_anchor() {
        // `doc.md#` — degenerate trailing hash. Should be Ok if the
        // file exists, not AnchorNotFound.
        let root = tmpdir("anchor-empty");
        fs::write(root.join("doc.md"), "# X\n").unwrap();
        assert_eq!(verify_local("doc.md#", &root), VerifyOutcome::Ok);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_with_anchor_on_non_markdown_file_is_ok_via_skipped() {
        // `.txt` files have no notion of headings; skip anchor
        // verification and treat as Ok.
        let root = tmpdir("anchor-nonmd");
        fs::write(root.join("notes.txt"), "anything\n").unwrap();
        assert_eq!(verify_local("notes.txt#whatever", &root), VerifyOutcome::Ok);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_local_with_anchor_on_missing_file_returns_not_found_not_anchor_error() {
        // File doesn't exist → NotFound wins over AnchorNotFound; we
        // don't try to verify an anchor against a missing file.
        let root = tmpdir("anchor-no-file");
        match verify_local("missing.md#anything", &root) {
            VerifyOutcome::NotFound { path } => assert!(path.ends_with("missing.md")),
            other => panic!("expected NotFound, got {other:?}"),
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
