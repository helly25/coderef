//! End-to-end scan test against a fixture workspace built in `tmp`.
//!
//! Builds a minimal workspace on disk under a unique temp directory,
//! runs `scan_workspace` with a config that exercises `commentsOnly`
//! filtering across multiple languages, and asserts the expected
//! reference set.

use coderef_core::config::Config;
use coderef_core::scan::scan_workspace;
use std::fs;
use std::path::{Path, PathBuf};

fn tmpdir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "coderef-integration-{label}-{pid}-{nanos}",
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
  "variables": { "usersBase": "https://users.example.com" },
  "ignore": ["**/vendor/**"],
  "patterns": {
    "todo-user": {
      "regex":  "TODO\\(@(?<user>[a-zA-Z][\\w.-]*)\\)",
      "target": "${config:usersBase}/${user}",
      "scope":  { "commentsOnly": true }
    },
    "jira": {
      "regex":  "JIRA\\((?<ticket>[A-Z]+-[0-9]+)\\)",
      "target": "https://jira.example.com/browse/${ticket}"
    }
  }
}
"#;

#[test]
fn integration_scan_finds_refs_across_languages_with_comments_only_filtering() {
    let root = tmpdir("multilang");
    // Code in comment (should match).
    write(&root, "src/a.rs", "fn x() { // TODO(@alice)\n}");
    write(&root, "src/b.py", "x = 1  # TODO(@bob)");
    // TODO inside string literal: should NOT match because commentsOnly.
    write(
        &root,
        "src/c.rs",
        "let s = \"TODO(@code)\";\n// TODO(@cdoc)",
    );
    // JIRA pattern has no commentsOnly scope; matches anywhere.
    write(
        &root,
        "src/d.go",
        "// JIRA(PROJ-42) anchored\nx := \"JIRA(NOMATCH-1)\"",
    );
    // Vendored file should be ignored by config.ignore.
    write(&root, "vendor/skipped.rs", "// TODO(@vendored)");

    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();

    let summary: Vec<(String, String, String)> = refs
        .iter()
        .map(|r| (r.file.clone(), r.pattern_id.clone(), r.target.clone()))
        .collect();

    // Comparing as sets is brittle on output order; the scanner is
    // deterministic — so compare the exact ordered list.
    let expected: Vec<(String, String, String)> = vec![
        // a.rs has one TODO in a //-comment.
        (
            "src/a.rs".into(),
            "todo-user".into(),
            "https://users.example.com/alice".into(),
        ),
        // b.py has one TODO after #.
        (
            "src/b.py".into(),
            "todo-user".into(),
            "https://users.example.com/bob".into(),
        ),
        // c.rs: TODO in string filtered out; TODO in //-comment kept.
        (
            "src/c.rs".into(),
            "todo-user".into(),
            "https://users.example.com/cdoc".into(),
        ),
        // d.go: JIRA pattern has no scope filter; the JIRA inside the
        // string also matches. (JIRA's regex requires uppercase letters
        // before `-` so "NOMATCH-1" passes the regex but the second
        // occurrence has a different ticket id.) The Go file has two
        // matches in declaration order (the comment then the string).
        (
            "src/d.go".into(),
            "jira".into(),
            "https://jira.example.com/browse/PROJ-42".into(),
        ),
        (
            "src/d.go".into(),
            "jira".into(),
            "https://jira.example.com/browse/NOMATCH-1".into(),
        ),
    ];

    assert_eq!(summary, expected, "got: {summary:#?}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_scan_empty_workspace_returns_no_refs() {
    let root = tmpdir("empty");
    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();
    assert!(refs.is_empty());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn integration_scan_respects_dot_gitignore_outside_git_repo() {
    let root = tmpdir("gitignore");
    write(&root, ".gitignore", "ignored/\n");
    write(&root, "a.rs", "// TODO(@kept)");
    write(&root, "ignored/b.rs", "// TODO(@skipped)");

    let cfg = Config::from_jsonc_str(CONFIG).unwrap();
    let refs = scan_workspace(&root, &cfg).unwrap();

    let users: Vec<&str> = refs.iter().map(|r| r.captures["user"].as_str()).collect();
    assert_eq!(users, vec!["kept"]);
    fs::remove_dir_all(&root).unwrap();
}
