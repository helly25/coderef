//! Coupled-change verification (DESIGN.md §10).
//!
//! `IfChange` / `ThenChange` marker pairs declare that a block of code
//! must change together with one or more *targets* (other files,
//! optionally line ranges) or with one or more *peer blocks* (every
//! block in the workspace sharing the same id). `coderef changes`
//! walks a git diff of staged or working-tree changes, finds every
//! block touched by the diff, and verifies that every required peer /
//! target was also touched.
//!
//! v0.2 scope:
//! - Shape A: explicit `ThenChange(/path, /path:N, /path:N-M)` targets.
//! - Shape B: id-anchored cross-file groups (`IfChange(my-id)` ...
//!   `ThenChange`).
//! - Block bounding: `paired` (each `IfChange` pairs with the next
//!   `ThenChange` in the same file).
//! - `NoVerify` escape hatch inline above the `IfChange` marker.
//!
//! v0.2 additions (later PRs):
//! - Shape C composable ids (this file's `resolve_composable_id` +
//!   `verify_changes_composable`): the `IfChange` id is passed
//!   through the reference engine before grouping, so
//!   `IfChange(JIRA(PROJ-123))` blocks in different files coalesce
//!   into one group.
//! - Glob, anchor, and label sub-region targets in `ThenChange`.
//!
//! Still deferred (DESIGN §10.4 / §10.2 / §10.6):
//! - `bounding: multipleThenChange` and `allowNesting`.
//! - Per-commit-message `NoVerify` lines.
//! - `{soft}` glob flag (warning severity).
//! - Strict `{all}` semantics (workspace enumeration).

mod diff;
mod parse;
mod verify;

pub use self::diff::{parse_unified_diff, ChangedLines};
pub use self::parse::{extract_blocks, IfChangeBlock, MarkerParseError, MarkerParseReport, Target};
pub use self::verify::{
    verify_changes, verify_changes_composable, ChangesReport, Violation, ViolationKind,
};

use crate::config::Config;

/// Scan every in-scope file in a workspace for IfChange/ThenChange
/// blocks. Reuses the workspace walker from `crate::scan` so the same
/// ignore-globs / .gitignore handling applies.
///
/// Returns the parsed blocks (workspace-wide, sorted by file then
/// line) plus any per-file marker-parse errors collected along the
/// way.
pub fn scan_workspace_blocks(
    root: impl AsRef<std::path::Path>,
    cfg: &Config,
) -> Result<(Vec<IfChangeBlock>, Vec<MarkerParseError>), ScanBlocksError> {
    let root_ref = root.as_ref();

    // Re-use the cfg's ignore globs the same way `scan::scan_workspace`
    // does. The `ignore` crate's override builder wants `!` for
    // excludes.
    let mut overrides = ignore::overrides::OverrideBuilder::new(root_ref);
    for g in &cfg.ignore {
        overrides
            .add(&format!("!{g}"))
            .map_err(|e| ScanBlocksError::Overrides(e.to_string()))?;
    }
    let overrides = overrides
        .build()
        .map_err(|e| ScanBlocksError::Overrides(e.to_string()))?;

    let mut walker_b = ignore::WalkBuilder::new(root_ref);
    walker_b.overrides(overrides);

    let mut all_blocks = Vec::new();
    let mut all_errors = Vec::new();

    for entry in walker_b.build() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root_ref).unwrap_or(path);
        let rel_str = rel.to_string_lossy().to_string();
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        // Quick skip — saves regex work on files that obviously
        // contain no marker.
        if !content.contains("IfChange") && !content.contains("ThenChange") {
            continue;
        }
        let report = extract_blocks(&content, &rel_str);
        all_blocks.extend(report.blocks);
        all_errors.extend(report.errors);
    }

    Ok((all_blocks, all_errors))
}

/// Failures from `scan_workspace_blocks`.
#[derive(Debug, thiserror::Error)]
pub enum ScanBlocksError {
    #[error("failed to build ignore overrides: {0}")]
    Overrides(String),
}

/// `true` when the config declares at least one `kind: "ifchange"`
/// pattern.
///
/// IfChange/ThenChange marker detection is opt-in. Avoids surprise
/// behaviour for users who only configure `kind: "url"` and
/// `kind: "local"` patterns.
#[must_use]
pub fn ifchange_enabled(cfg: &Config) -> bool {
    cfg.patterns
        .values()
        .any(|p| p.kind == crate::config::PatternKind::IfChange)
}

/// Resolve a Shape C composable `IfChange(<id>)` id text through the
/// reference engine (DESIGN §10.7).
///
/// Returns the *resolved target* of the first matching `kind: "url"`
/// or `kind: "local"` pattern; `None` if no pattern matches. Used as
/// the canonical group key by [`verify_changes_composable`] so
/// `IfChange(JIRA(PROJ-123))` in different files coalesces into one
/// Shape B group regardless of where it appears.
#[must_use]
pub fn resolve_composable_id(config: &Config, id_text: &str) -> Option<String> {
    use crate::config::PatternKind;
    let report = crate::explain::explain(config, id_text);
    for m in report.matches {
        if matches!(m.pattern_kind, PatternKind::Url | PatternKind::Local) {
            return Some(m.target);
        }
    }
    None
}
