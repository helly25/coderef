//! `Reference` — one match produced by the scanner.
//!
//! Carries enough context for any consumer (CLI output, LSP hover,
//! verifier) to act without rerunning the regex: the originating
//! pattern, source location, raw matched text, named captures, and the
//! variable-resolved target / title.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::config::PatternKind;

/// A single reference found in a source file.
///
/// `byte_*` are zero-indexed byte offsets into the file content; `line`
/// and `column` are 1-indexed for human display.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reference {
    /// Pattern id that produced this reference.
    pub pattern_id: String,

    /// Resolved pattern kind. The verifier dispatches on this rather
    /// than sniffing the target string, so a `kind: "local"` target
    /// of `http://...` (legal under §6.1) still goes through the
    /// local-path verifier.
    pub pattern_kind: PatternKind,

    /// File path the reference was found in, workspace-relative when
    /// produced by the workspace scanner; absolute when produced from a
    /// stand-alone buffer with no workspace context.
    pub file: String,

    /// 1-indexed line of the start of the match.
    pub line: u32,

    /// 1-indexed column of the start of the match.
    pub column: u32,

    /// Byte offset of the start of the match.
    pub byte_start: usize,

    /// Byte offset one past the end of the match.
    pub byte_end: usize,

    /// The literal text that matched the regex.
    pub matched_text: String,

    /// Named regex captures, in declaration order.
    pub captures: IndexMap<String, String>,

    /// Resolved target after variable substitution.
    pub target: String,

    /// Resolved title after variable substitution, if the pattern set one.
    pub title: Option<String>,

    /// True iff the match falls within a detected comment region (see
    /// `DESIGN.md` §5.4.1). When `scope.commentsOnly` is set on the
    /// pattern, matches with `in_comment = false` are filtered out by
    /// the scanner before the `Reference` is constructed.
    pub in_comment: bool,
}
