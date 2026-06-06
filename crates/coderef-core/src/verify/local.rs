//! Local-path verification.
//!
//! Workspace-anchored: `/path` and `path` both resolve under the
//! workspace root (DESIGN.md §6.1 default anchor mode `workspace`).
//! v0.2 will add per-pattern anchor-mode + extension-search.

use std::path::Path;

use super::{join_under_workspace, VerifyOutcome};

pub fn verify_local(target: &str, workspace_root: &Path) -> VerifyOutcome {
    let resolved = join_under_workspace(workspace_root, target);
    if resolved.exists() {
        VerifyOutcome::Ok
    } else {
        VerifyOutcome::NotFound {
            path: resolved.to_string_lossy().to_string(),
        }
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
}
