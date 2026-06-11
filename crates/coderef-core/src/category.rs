//! Pattern categories (DESIGN.md §5.7).
//!
//! Categories mirror the visible cues authors use to recognise
//! references in source (`/path`, `@user`, `PROJ-123`, `RFC(8259)`)
//! rather than the config ids that produced them. They drive the
//! references browser's grouping, the visual editor's style templates,
//! and category-aware doctor checks.
//!
//! v0.2 ships the data plumbing + the two static doctor checks
//! (`category.unset`, `category.tooBroadOther`). The scan-dependent
//! `category.mismatch` heuristic lives in `doctor::scan_dependent`
//! once that wiring lands; for v0.2 it remains a design entry.

use crate::config::PatternKind;

/// Built-in category names. Order matches the display order in the
/// references browser (DESIGN.md §5.7.3).
pub const BUILTIN_CATEGORIES: &[&str] = &[
    "files",
    "people",
    "tickets",
    "standards",
    "urls",
    "coupled-change",
    "other",
];

/// Default `category` for a pattern that doesn't declare one,
/// inferred from `kind` per DESIGN.md §5.7.2:
///
/// - `local`    → `files`
/// - `ifchange` → `coupled-change`
/// - `block`    → `other` (block markers don't fit any cue-based bucket
///                — they're absence-checked, not referenced)
/// - `url`      → `other` (doctor's `category.unset` will suggest one)
/// - `command`  → `other` (post-v0.4 backlog)
#[must_use]
pub fn infer_category(kind: PatternKind) -> &'static str {
    match kind {
        PatternKind::Local => "files",
        PatternKind::IfChange => "coupled-change",
        PatternKind::Url | PatternKind::Command | PatternKind::Block => "other",
    }
}

/// Display-order index for a category. Built-ins use their position in
/// `BUILTIN_CATEGORIES`; user-defined categories slot between
/// `coupled-change` and `other` in alphabetical order. Used by the CLI
/// `--by-category` view and (in v0.3+) the references browser.
#[must_use]
pub fn display_order(category: &str) -> u32 {
    if let Some(idx) = BUILTIN_CATEGORIES.iter().position(|c| *c == category) {
        // 0..=6 for the built-ins, with `other` last (idx 6).
        let pos = u32::try_from(idx).unwrap_or(u32::MAX);
        // Move `other` to the end of the keyspace so user-defined
        // categories slot before it. Built-ins keep their relative
        // order otherwise.
        if category == "other" {
            u32::MAX
        } else {
            pos
        }
    } else {
        // User-defined categories sort between `coupled-change` (idx 5)
        // and `other` (idx MAX) by name. We reserve 100..=u32::MAX-1
        // for them and hash the name into that range. A simple
        // approach: a constant offset; ties are broken alphabetically
        // by the caller's sort.
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_list_matches_design_section_5_7_1() {
        assert_eq!(
            BUILTIN_CATEGORIES,
            &[
                "files",
                "people",
                "tickets",
                "standards",
                "urls",
                "coupled-change",
                "other"
            ]
        );
    }

    #[test]
    fn test_infer_category_local_is_files() {
        assert_eq!(infer_category(PatternKind::Local), "files");
    }

    #[test]
    fn test_infer_category_ifchange_is_coupled_change() {
        assert_eq!(infer_category(PatternKind::IfChange), "coupled-change");
    }

    #[test]
    fn test_infer_category_url_is_other() {
        // `url` patterns produce a `category.unset` doctor info when
        // they don't declare a category; the inference is a fallback,
        // not a recommendation.
        assert_eq!(infer_category(PatternKind::Url), "other");
    }

    #[test]
    fn test_infer_category_block_is_other() {
        assert_eq!(infer_category(PatternKind::Block), "other");
    }

    #[test]
    fn test_display_order_builtins_sort_by_design_order() {
        let mut got: Vec<&str> = BUILTIN_CATEGORIES.iter().copied().collect();
        got.sort_by_key(|c| display_order(c));
        assert_eq!(
            got,
            vec![
                "files",
                "people",
                "tickets",
                "standards",
                "urls",
                "coupled-change",
                "other",
            ]
        );
    }

    #[test]
    fn test_display_order_user_categories_slot_before_other_after_coupled_change() {
        assert!(display_order("slack-channels") > display_order("coupled-change"));
        assert!(display_order("slack-channels") < display_order("other"));
    }
}
