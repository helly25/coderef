//! Comment-region detection.
//!
//! Identifies which byte ranges of a source buffer fall inside a
//! comment, so the scanner can filter pattern matches against
//! `scope.commentsOnly` (DESIGN.md §5.4.1).
//!
//! v0.1 covers the common families:
//!   - C-family: `//` line, `/* */` block, with `"…"` strings (no
//!     escapes inside strings beyond the literal text).
//!   - Hash: `#` line (Python, Ruby, shell, YAML, Perl).
//!   - Dash: `--` line, `--[[ ]]` Lua-style block.
//!   - Lisp: `;` line.
//!   - XML/HTML: `<!-- -->` block.
//!
//! Unknown languages return an empty range list — `commentsOnly: true`
//! then matches nothing in those files, which is the safe default.
//!
//! This is intentionally a hand-rolled scanner rather than a full
//! grammar parse: comment-region detection only needs to be roughly
//! right (false positives waste a verify; false negatives surface the
//! match where the user didn't want it — both are reported by `doctor`
//! and easy to spot). A proper grammar would buy us correctness on
//! pathological inputs (e.g. nested string delimiters inside templates)
//! at a substantial dependency cost we're not willing to pay yet.

mod detector;
mod languages;
mod markdown;
mod region;

pub use self::detector::{detect_regions, is_in_any_range, Range};
pub use self::languages::{language_for_extension, Language};
pub use self::region::{is_in_comment_like, region_kind_at, ClassifiedRange, RegionKind};
