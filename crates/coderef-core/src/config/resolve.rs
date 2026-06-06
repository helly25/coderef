//! Local-path resolver configuration. See `DESIGN.md` §6.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How to interpret `kind: "local"` paths. See `DESIGN.md` §6.1.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AnchorMode {
    /// Both `/path` and `path` anchor at workspace root.
    #[default]
    Workspace,
    /// `path` is file-relative; `/path` is workspace-rooted.
    File,
    /// As `File`, but `./path` forces file-relative.
    RootedOrFile,
}

/// Filesystem case-sensitivity policy. See `DESIGN.md` §6.2.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CaseSensitivity {
    /// Honour the filesystem (insensitive on macOS default + Windows,
    /// sensitive on Linux).
    #[default]
    Fs,
    /// Always case-sensitive, regardless of filesystem.
    Always,
    /// Always case-insensitive, regardless of filesystem.
    Never,
}

/// Local-path resolution for `kind: "local"` patterns. See `DESIGN.md` §6.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LocalResolveConfig {
    /// Search root; defaults to `${workspaceFolder}`. Supports variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,

    /// Anchor mode for path resolution.
    #[serde(default)]
    pub anchor_mode: AnchorMode,

    /// File extensions to try when the literal path doesn't resolve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,

    /// Index file names to try when the candidate is a directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_files: Vec<String>,

    /// Filesystem case-sensitivity policy.
    #[serde(default)]
    pub case_sensitive: CaseSensitivity,

    /// Capture template for the anchor name (e.g. `${anchor}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,

    /// Anchor verification mode (v0.2; §6.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_verify: Option<String>,

    /// Markdown slugifier (v0.2; §6.3.2). Free-form to allow custom
    /// configuration objects without locking the schema now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slugifier: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_mode_default_is_workspace() {
        let r: LocalResolveConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(r.anchor_mode, AnchorMode::Workspace);
    }

    #[test]
    fn test_case_sensitivity_default_is_fs() {
        let r: LocalResolveConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(r.case_sensitive, CaseSensitivity::Fs);
    }

    #[test]
    fn test_local_resolve_full_example_parses() {
        let src = r#"{
            "root": "${workspaceFolder}",
            "anchorMode": "workspace",
            "extensions": [".md", ".mdx"],
            "indexFiles": ["README.md"],
            "caseSensitive": "fs",
            "anchor": "${anchor}"
        }"#;
        let r: LocalResolveConfig = serde_json::from_str(src).unwrap();
        assert_eq!(r.root.as_deref(), Some("${workspaceFolder}"));
        assert_eq!(r.extensions, vec![".md", ".mdx"]);
        assert_eq!(r.index_files, vec!["README.md"]);
    }

    #[test]
    fn test_local_resolve_anchor_mode_file_parses() {
        let r: LocalResolveConfig = serde_json::from_str(r#"{ "anchorMode": "file" }"#).unwrap();
        assert_eq!(r.anchor_mode, AnchorMode::File);
    }

    #[test]
    fn test_local_resolve_unknown_field_rejected() {
        let err = serde_json::from_str::<LocalResolveConfig>(r#"{ "bogus": 1 }"#).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }
}
