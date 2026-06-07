//! Classified regions: byte ranges with a kind tag.
//!
//! v0.2 generalises v0.1's "comment ranges" into four kinds of
//! non-body regions:
//!
//! - `BlockComment`  — `/* */`, `<!-- -->`, `--[[ ]]`, …
//! - `LineComment`   — `//`, `#`, `--`, `;`, …
//! - `CodeSnippet`   — fenced or inline code in markdown (and later:
//!   embedded-language regions in other doc formats)
//! - `StringLiteral` — `"..."`, `'...'`, triple-quoted, etc.
//!
//! The scanner uses kind information to decide which matches survive
//! `scope.commentsOnly` (and future positive/negative region filters).
//! See `DESIGN.md` §5.4.1.

use serde::{Deserialize, Serialize};

use super::detector::Range;

/// Kind of a classified region.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegionKind {
    /// `/* … */`, `<!-- … -->`, `--[[ … ]]`, etc.
    BlockComment,
    /// `// …`, `# …`, `-- …`, `; …`, etc.
    LineComment,
    /// Fenced ` ```…``` ` or inline ` `…` ` code regions in markdown.
    /// Future: embedded-language regions in other doc formats.
    CodeSnippet,
    /// `"…"`, `'…'`, triple-quoted, etc.
    StringLiteral,
}

impl RegionKind {
    /// True iff this kind is treated as "comment-like" by
    /// `scope.commentsOnly: true`. v0.2 model: all four non-body kinds
    /// are comment-like. A reference inside a string literal in a test
    /// fixture, or inside a markdown code block, is still semantically
    /// a reference — the host syntax is irrelevant to the link target.
    /// The opt-out for the rare false-positive case will be a future
    /// `scope.excludeRegions` field, not the default.
    #[must_use]
    pub fn is_comment_like(self) -> bool {
        matches!(
            self,
            Self::BlockComment | Self::LineComment | Self::CodeSnippet | Self::StringLiteral
        )
    }
}

/// A byte range tagged with a region kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassifiedRange {
    pub range: Range,
    pub kind: RegionKind,
}

/// Find the region kind covering `pos`, if any. Searches `regions` in
/// order; in practice the host emits disjoint ranges so the first
/// match is the only match.
#[must_use]
pub fn region_kind_at(regions: &[ClassifiedRange], pos: usize) -> Option<RegionKind> {
    regions
        .iter()
        .find(|r| r.range.contains(pos))
        .map(|r| r.kind)
}

/// True iff `pos` is inside any region whose kind is comment-like
/// (`BlockComment` / `LineComment` / `CodeSnippet` / `StringLiteral`).
#[must_use]
pub fn is_in_comment_like(regions: &[ClassifiedRange], pos: usize) -> bool {
    region_kind_at(regions, pos).is_some_and(RegionKind::is_comment_like)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cr(start: usize, end: usize, kind: RegionKind) -> ClassifiedRange {
        ClassifiedRange {
            range: Range { start, end },
            kind,
        }
    }

    #[test]
    fn test_region_kind_at_returns_kind_when_in_range() {
        let regions = vec![
            cr(0, 10, RegionKind::BlockComment),
            cr(20, 30, RegionKind::CodeSnippet),
        ];
        assert_eq!(region_kind_at(&regions, 5), Some(RegionKind::BlockComment));
        assert_eq!(region_kind_at(&regions, 25), Some(RegionKind::CodeSnippet));
    }

    #[test]
    fn test_region_kind_at_returns_none_when_outside() {
        let regions = vec![cr(0, 10, RegionKind::BlockComment)];
        assert!(region_kind_at(&regions, 15).is_none());
    }

    #[test]
    fn test_is_in_comment_like_true_for_every_known_kind() {
        for kind in [
            RegionKind::BlockComment,
            RegionKind::LineComment,
            RegionKind::CodeSnippet,
            RegionKind::StringLiteral,
        ] {
            let regions = vec![cr(0, 10, kind)];
            assert!(
                is_in_comment_like(&regions, 5),
                "kind {kind:?} should be comment-like"
            );
        }
    }

    #[test]
    fn test_is_in_comment_like_false_when_outside_any_region() {
        let regions = vec![cr(0, 10, RegionKind::BlockComment)];
        assert!(!is_in_comment_like(&regions, 15));
    }

    #[test]
    fn test_region_kind_serializes_as_kebab_case() {
        assert_eq!(
            serde_json::to_string(&RegionKind::BlockComment).unwrap(),
            "\"block-comment\""
        );
        assert_eq!(
            serde_json::to_string(&RegionKind::LineComment).unwrap(),
            "\"line-comment\""
        );
        assert_eq!(
            serde_json::to_string(&RegionKind::CodeSnippet).unwrap(),
            "\"code-snippet\""
        );
        assert_eq!(
            serde_json::to_string(&RegionKind::StringLiteral).unwrap(),
            "\"string-literal\""
        );
    }
}
