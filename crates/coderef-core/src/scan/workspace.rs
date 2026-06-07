//! Host-side workspace walker. Walks a root directory respecting
//! `.gitignore` (via the `ignore` crate), applies the config's
//! `ignore` globs, and feeds each file to `scan_file`.
//!
//! Gated `#[cfg(not(target_arch = "wasm32"))]` because it uses
//! `std::fs` and the `ignore` crate, neither of which is available
//! on WASM. WASM hosts hand individual buffers to `scan_file` directly.

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use thiserror::Error;

use crate::comment::language_for_extension;
use crate::config::{Config, Pattern};
use crate::pattern::CompiledPattern;
use crate::reference::Reference;
use crate::scan::file::{scan_file, ScanError, ScanOptions};
use crate::variables::Context;

/// Walk `root`, scanning every non-ignored file with the compiled
/// patterns derived from `config`. Returns a flat list of references
/// sorted by `(file, byte_start, pattern_id)`.
///
/// `ignore`'s default ignores (`.gitignore`, `.ignore`, hidden files)
/// are honoured. Config-level `ignore[]` globs are applied as extra
/// overrides on top.
pub fn scan_workspace(
    root: impl AsRef<Path>,
    config: &Config,
) -> Result<Vec<Reference>, WorkspaceScanError> {
    let root = root.as_ref();
    if !root.exists() {
        return Err(WorkspaceScanError::RootDoesNotExist(
            root.display().to_string(),
        ));
    }

    // Compile every pattern, propagating the first failure. Each
    // pattern carries optional include / exclude path globs in its
    // scope; we precompile them as gitignore-style matchers so we can
    // cheaply filter per file later.
    let mut compiled: Vec<(CompiledPattern, Pattern, ScopeGlobs)> =
        Vec::with_capacity(config.patterns.len());
    for (id, raw) in &config.patterns {
        let c = CompiledPattern::compile(id.clone(), raw)
            .map_err(WorkspaceScanError::PatternCompile)?;
        let globs = ScopeGlobs::compile(root, raw).map_err(|e| WorkspaceScanError::IgnoreGlob {
            pattern: format!("pattern `{id}` scope"),
            message: e,
        })?;
        compiled.push((c, raw.clone(), globs));
    }

    // Build the walker. Apply config.ignore[] globs as overrides.
    let mut overrides = OverrideBuilder::new(root);
    for pat in &config.ignore {
        // `!pat` in OverrideBuilder means "exclude", matching the
        // gitignore convention.
        let exclude_pat = format!("!{pat}");
        overrides
            .add(&exclude_pat)
            .map_err(|e| WorkspaceScanError::IgnoreGlob {
                pattern: pat.clone(),
                message: e.to_string(),
            })?;
    }
    let overrides = overrides
        .build()
        .map_err(|e| WorkspaceScanError::IgnoreGlob {
            pattern: "(build)".into(),
            message: e.to_string(),
        })?;

    // `require_git(false)` so .gitignore is honoured even outside a
    // git repo (e.g. in subdirectory scans and in tests). The standard
    // filters (hidden + global gitignore) stay on.
    let walker = WalkBuilder::new(root)
        .overrides(overrides)
        .standard_filters(true)
        .require_git(false)
        .build();

    let mut all_refs = Vec::new();
    for entry in walker {
        let entry = entry.map_err(|e| WorkspaceScanError::Walk(e.to_string()))?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        // Binary file or non-UTF8 — skip silently. v0.2 will surface
        // these as a doctor check (`file.notUtf8`) per DESIGN §9.1.
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let language = language_for_extension(ext);

        // Filter patterns by their per-pattern scope.include / .exclude
        // globs. A pattern with no include matches every file; one with
        // include matches only files that hit at least one include glob.
        // Excludes are applied on top: a file matching any exclude glob
        // drops the pattern for that file.
        let mut applicable: Vec<(CompiledPattern, Pattern)> = Vec::with_capacity(compiled.len());
        for (cp, raw, globs) in &compiled {
            if globs.applies_to(Path::new(&rel_path)) {
                applicable.push((cp.clone(), raw.clone()));
            }
        }
        if applicable.is_empty() {
            continue;
        }

        let ctx = build_base_context(config);
        let opts = ScanOptions {
            patterns: &applicable,
            language,
            base_context: &ctx,
            file: &rel_path,
        };
        let refs = scan_file(&content, &opts).map_err(WorkspaceScanError::Scan)?;
        all_refs.extend(refs);
    }

    all_refs.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.byte_start.cmp(&b.byte_start))
            .then_with(|| a.pattern_id.cmp(&b.pattern_id))
    });
    Ok(all_refs)
}

/// Build a base resolution context seeded from the config's variables.
/// String values flow into the `config:` namespace; non-string values
/// (numbers, booleans, arrays, objects) are silently skipped for v0.1.
fn build_base_context(config: &Config) -> Context<'static> {
    let mut ctx = Context::new();
    for (k, v) in &config.variables {
        if let Some(s) = v.as_str() {
            ctx = ctx.with_config(k.clone(), s.to_string());
        }
    }
    ctx
}

/// Failures from `scan_workspace`.
#[derive(Debug, Error)]
pub enum WorkspaceScanError {
    #[error("workspace root does not exist: {0}")]
    RootDoesNotExist(String),

    #[error("failed to compile pattern: {0}")]
    PatternCompile(#[from] crate::pattern::PatternError),

    #[error("invalid ignore glob `{pattern}`: {message}")]
    IgnoreGlob { pattern: String, message: String },

    #[error("filesystem walk error: {0}")]
    Walk(String),

    #[error(transparent)]
    Scan(ScanError),
}

/// Precompiled per-pattern path filter built from `scope.include` and
/// `scope.exclude`. Both use gitignore-style globs (matching the
/// workspace-level `ignore` syntax).
struct ScopeGlobs {
    include: Option<Gitignore>,
    exclude: Option<Gitignore>,
}

impl ScopeGlobs {
    fn compile(root: &Path, raw: &Pattern) -> Result<Self, String> {
        let scope = raw.scope.as_ref();
        let include = match scope.map(|s| &s.include).filter(|v| !v.is_empty()) {
            None => None,
            Some(globs) => Some(build_globset(root, globs)?),
        };
        let exclude = match scope.map(|s| &s.exclude).filter(|v| !v.is_empty()) {
            None => None,
            Some(globs) => Some(build_globset(root, globs)?),
        };
        Ok(Self { include, exclude })
    }

    /// True iff the pattern applies to `rel_path` (workspace-relative).
    fn applies_to(&self, rel_path: &Path) -> bool {
        if let Some(inc) = &self.include {
            if inc.matched(rel_path, false).is_ignore() {
                // matched at least one include glob
            } else {
                return false;
            }
        }
        if let Some(exc) = &self.exclude {
            if exc.matched(rel_path, false).is_ignore() {
                return false;
            }
        }
        true
    }
}

fn build_globset(root: &Path, globs: &[String]) -> Result<Gitignore, String> {
    let mut builder = GitignoreBuilder::new(root);
    for g in globs {
        builder
            .add_line(None, g)
            .map_err(|e| format!("invalid glob `{g}`: {e}"))?;
    }
    builder
        .build()
        .map_err(|e| format!("failed to build globset: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "coderef-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
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

    fn cfg_with_todo() -> Config {
        let src = r#"
        {
            "patterns": {
                "todo": {
                    "regex": "TODO\\(@(?<user>\\w+)\\)",
                    "target": "https://x/${user}"
                }
            }
        }
        "#;
        Config::from_jsonc_str(src).unwrap()
    }

    #[test]
    fn test_workspace_scan_finds_matches_in_multiple_files() {
        let root = tmpdir();
        write(&root, "a.rs", "// TODO(@alice)");
        write(&root, "b.rs", "// TODO(@bob)");
        let cfg = cfg_with_todo();
        let refs = scan_workspace(&root, &cfg).unwrap();
        let users: Vec<&str> = refs.iter().map(|r| r.captures["user"].as_str()).collect();
        assert_eq!(users, vec!["alice", "bob"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_workspace_scan_respects_gitignore() {
        let root = tmpdir();
        write(&root, ".gitignore", "ignored/\n");
        write(&root, "a.rs", "// TODO(@kept)");
        write(&root, "ignored/b.rs", "// TODO(@skipped)");
        let cfg = cfg_with_todo();
        let refs = scan_workspace(&root, &cfg).unwrap();
        let users: Vec<&str> = refs.iter().map(|r| r.captures["user"].as_str()).collect();
        assert_eq!(users, vec!["kept"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_workspace_scan_respects_config_ignore_globs() {
        let root = tmpdir();
        write(&root, "a.rs", "// TODO(@kept)");
        write(&root, "vendor/b.rs", "// TODO(@skipped)");
        let src = r#"
        {
            "ignore": ["vendor/**"],
            "patterns": {
                "todo": {
                    "regex": "TODO\\(@(?<user>\\w+)\\)",
                    "target": "x/${user}"
                }
            }
        }
        "#;
        let cfg = Config::from_jsonc_str(src).unwrap();
        let refs = scan_workspace(&root, &cfg).unwrap();
        let users: Vec<&str> = refs.iter().map(|r| r.captures["user"].as_str()).collect();
        assert_eq!(users, vec!["kept"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_workspace_scan_orders_by_file_then_byte_start() {
        let root = tmpdir();
        write(&root, "z.rs", "// TODO(@first)\n// TODO(@second)");
        write(&root, "a.rs", "// TODO(@alpha)");
        let cfg = cfg_with_todo();
        let refs = scan_workspace(&root, &cfg).unwrap();
        let files: Vec<&str> = refs.iter().map(|r| r.file.as_str()).collect();
        // a.rs sorts before z.rs.
        assert_eq!(files, vec!["a.rs", "z.rs", "z.rs"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_workspace_scan_skips_binary_files_silently() {
        let root = tmpdir();
        write(&root, "a.rs", "// TODO(@text)");
        // Non-UTF8 bytes — looks like a binary file.
        fs::write(root.join("binary.bin"), [0xff, 0xfe, 0xfd, 0xfc]).unwrap();
        let cfg = cfg_with_todo();
        let refs = scan_workspace(&root, &cfg).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].captures["user"], "text");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_workspace_scan_nonexistent_root_returns_error() {
        let bad = std::env::temp_dir().join("coderef-does-not-exist-xyz");
        let cfg = cfg_with_todo();
        let err = scan_workspace(&bad, &cfg).unwrap_err();
        assert!(matches!(err, WorkspaceScanError::RootDoesNotExist(_)));
    }

    #[test]
    fn test_workspace_scan_returns_empty_for_empty_workspace() {
        let root = tmpdir();
        let cfg = cfg_with_todo();
        let refs = scan_workspace(&root, &cfg).unwrap();
        assert!(refs.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}
