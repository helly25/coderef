//! `IfChange` / `ThenChange` marker parsing.
//!
//! Walks a source file line-by-line and produces a list of paired
//! blocks. Marker regexes are hardcoded for v0.2 — the common spelling
//! `# IfChange(id?)` / `# ThenChange(targets?)` after a comment-prefix
//! lead, language-agnostic via line trimming. Migration from
//! `LINT.IfChange/ThenChange` (per-pattern `ifChange.regex` overrides)
//! is a v0.3 follow-up; this PR ships the default-spelling path that
//! the vast majority of teams will use.

use fancy_regex::Regex;
use thiserror::Error;

use std::sync::LazyLock;

/// Default marker spellings. Strict enough to avoid false-positive
/// matches inside narrative prose ("if changing the format ..."); the
/// `\b...\b` boundaries and the required `(` / `^` neighbours make the
/// markers explicit-only.
static IF_CHANGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bIfChange(?:\((?<id>[^)]*)\))?").expect("IfChange marker regex is valid")
});
static THEN_CHANGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bThenChange(?:\((?<targets>[^)]*)\))?").expect("ThenChange marker regex is valid")
});
/// `Label('name') ... EndLabel` compat form (DESIGN §10.2). The
/// open marker `Label(id)` is equivalent to `IfChange(id)` and the
/// close marker `EndLabel` is equivalent to `ThenChange` with empty
/// targets. Block ids declared this way participate in the same
/// label-resolution surface as Shape B `IfChange(id)` blocks — a
/// `ThenChange(path:label-name)` elsewhere can target either form.
/// `EndLabel` does not accept targets — `EndLabel(...)` is a parse
/// error if anyone tries it (handled by the regex shape).
static LABEL_OPEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bLabel(?:\((?<id>[^)]*)\))?").expect("Label marker regex is valid")
});
static LABEL_CLOSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bEndLabel\b").expect("EndLabel marker regex is valid"));
/// `NoVerify(coderef:ifchange)` opt-out (DESIGN §10.6). Reason text
/// must follow — the verifier records it for audit and an empty
/// reason is treated as an authoring error elsewhere.
static NO_VERIFY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bNoVerify\(coderef:ifchange\)(?::\s*(?<reason>.*))?")
        .expect("NoVerify marker regex is valid")
});

/// Which marker form opened (and closed) a block. Tracked so the
/// doctor's `label.orphanOpen` / `label.orphanClose` diagnostics
/// (DESIGN §10.3) can fire only for the compat-form (Label/EndLabel
/// or per-pattern-configured) variants without polluting the
/// canonical IfChange/ThenChange error path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkerForm {
    /// `IfChange(...)` / `ThenChange(...)` — the canonical
    /// shape. Default for any block whose open marker matched
    /// `IF_CHANGE_RE`.
    #[default]
    Canonical,
    /// `Label('name') ... EndLabel`, or any per-pattern-configured
    /// open/close marker pair. Default for blocks opened via
    /// `LABEL_OPEN_RE` or a configured `LabelConfig.open.regex`.
    Compat,
}

/// One IfChange/ThenChange block found in a single file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfChangeBlock {
    /// Workspace-relative path of the file this block lives in.
    pub file: String,
    /// 1-indexed line of the `IfChange` marker (the block's open line).
    pub line_start: u32,
    /// 1-indexed line of the matching `ThenChange` marker (block's close).
    pub line_end: u32,
    /// Optional id captured from `IfChange(my-id)`. None / empty for
    /// Shape A blocks.
    pub id: Option<String>,
    /// Explicit `ThenChange` targets (Shape A). Empty for Shape B.
    pub targets: Vec<Target>,
    /// Inline `NoVerify(coderef:ifchange): reason` if found on the
    /// `IfChange` line or the line immediately above it. The verifier
    /// honours this and skips violations for the block.
    pub no_verify_reason: Option<String>,
    /// Which marker form opened this block (canonical
    /// `IfChange`/`ThenChange` vs compat `Label`/`EndLabel` or
    /// per-pattern-configured). Defaults to `Canonical`. See
    /// [`MarkerForm`].
    #[doc(hidden)]
    pub marker_form: MarkerForm,
}

/// One parsed target token from a `ThenChange(...)` argument list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Whole-file: at least one line in the file must change.
    File { path: String },
    /// Single line: `path:N` — line N must be inside a changed hunk.
    FileLine { path: String, line: u32 },
    /// Inclusive line range: `path:N-M` — at least one line in [N, M]
    /// must be inside a changed hunk.
    FileLineRange { path: String, start: u32, end: u32 },
    /// Named anchor (heading slug in a Markdown file): `path#anchor`.
    /// The anchor is resolved by `crate::anchor::verify_anchor`
    /// against the target file's heading slugs; if found, the heading
    /// range is treated as the changed-region requirement. v0.2
    /// semantics (kept simple): if the anchor exists in the target
    /// file *and* any line in the file changed, the target is
    /// satisfied. A richer "heading section range" interpretation
    /// (DESIGN §10.2) lands in v0.3.
    FileAnchor { path: String, anchor: String },
    /// Named-region label: `path:label-name`. Resolves to the block
    /// opened by `IfChange('label-name')` in the target file (DESIGN
    /// §10.2). The block's `[line_start, line_end]` range is treated
    /// as the changed-region requirement. The label form is used
    /// when the `:` is followed by a non-numeric token; line/range
    /// forms still win when the suffix is digits or `N-M`.
    FileLabel { path: String, label: String },
    /// Glob target: `/path/*.md`, `**/*.test.rs`, etc.
    /// `flag = Any`  → at least one matched file must change (default
    ///                for globs).
    /// `flag = All`  → every matched file must change.
    /// Glob patterns are recognised by the presence of `*` (or `?` /
    /// `[...]`) in the path.
    FileGlob { pattern: String, flags: GlobFlags },
}

/// Glob-target match mode (DESIGN §10.2). `Any` is the default and
/// means "at least one matched-and-changed satisfies the target";
/// `All` requires every matched file to be touched (strict semantics
/// land alongside workspace enumeration in a follow-up — see verifier
/// comment).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GlobMode {
    #[default]
    Any,
    All,
}

/// Suffix flag set on a glob target. DESIGN.md §10.2 lists `{any}` /
/// `{all}` as mode flags (mutually exclusive) and `{soft}` as a
/// severity modifier (orthogonal — `{soft,all}` and `{soft,any}` are
/// both legal). Combinations are written comma-separated inside the
/// braces: `/docs/*.md{any,soft}` etc. Whitespace around the comma
/// is allowed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlobFlags {
    pub mode: GlobMode,
    /// `true` iff `{soft}` was set. Demotes constraint-failure
    /// violations from `Error` to `Warning` severity so they're
    /// surfaced in reports but don't fail the exit code.
    pub soft: bool,
}

impl Target {
    /// Path / pattern component shared across all target variants.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::File { path }
            | Self::FileLine { path, .. }
            | Self::FileLineRange { path, .. }
            | Self::FileAnchor { path, .. }
            | Self::FileLabel { path, .. } => path,
            Self::FileGlob { pattern, .. } => pattern,
        }
    }
}

/// Aggregate result of scanning one file for IfChange/ThenChange
/// markers. Includes successfully paired blocks plus any per-file
/// parse errors that the doctor surfaces.
#[derive(Clone, Debug)]
pub struct MarkerParseReport {
    pub blocks: Vec<IfChangeBlock>,
    pub errors: Vec<MarkerParseError>,
}

/// One per-file marker-parse failure.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum MarkerParseError {
    #[error("`{file}:{line}` has an `IfChange` marker with no matching `ThenChange` in the file")]
    OrphanIfChange { file: String, line: u32 },
    #[error("`{file}:{line}` has a `ThenChange` marker without a preceding open `IfChange`")]
    OrphanThenChange { file: String, line: u32 },
    /// Compat-form variant of `OrphanIfChange`. The open marker came
    /// from the `Label('name')` global form or a per-pattern
    /// `label.open.regex` (DESIGN §10.3). Surfaced separately so the
    /// doctor's `label.orphanOpen` diagnostic can be compat-only.
    #[error(
        "`{file}:{line}` has a `Label` / per-pattern open marker with no matching close marker"
    )]
    OrphanLabel { file: String, line: u32 },
    /// Compat-form variant of `OrphanThenChange`. The close marker
    /// came from `EndLabel` or a per-pattern `label.close.regex`.
    /// Surfaced separately for `label.orphanClose`.
    #[error(
        "`{file}:{line}` has an `EndLabel` / per-pattern close marker without a preceding open"
    )]
    OrphanEndLabel { file: String, line: u32 },
    #[error("`{file}:{line}` has a malformed target token `{token}` in `ThenChange(...)`")]
    MalformedTarget {
        file: String,
        line: u32,
        token: String,
    },
}

/// Optional per-pattern compat-form marker regexes (DESIGN §10.3).
/// When supplied, [`extract_blocks_with_markers`] recognises each
/// `open.regex` as an additional open marker (alongside the canonical
/// `IfChange` and global `Label`) and each `close.regex` as an
/// additional close marker (alongside `ThenChange` and `EndLabel`).
/// Blocks opened or closed via any of these extra regexes are tagged
/// `MarkerForm::Compat` for downstream diagnostics.
pub struct MarkerOverrides<'a> {
    /// Extra open-marker regexes. Each entry's named group `id`, if
    /// present, supplies the block's label name (like the canonical
    /// `IfChange(id)` capture).
    pub opens: &'a [Regex],
    /// Extra close-marker regexes. Close markers don't carry an id —
    /// pairing is positional.
    pub closes: &'a [Regex],
}

impl MarkerOverrides<'_> {
    /// Empty override set — equivalent to calling
    /// [`extract_blocks`].
    #[must_use]
    pub fn none() -> Self {
        Self {
            opens: &[],
            closes: &[],
        }
    }
}

/// Extract paired IfChange/ThenChange blocks from `content`. The file
/// path is embedded into the returned blocks unchanged (the caller
/// chooses absolute vs workspace-relative).
///
/// Recognises the default marker set (`IfChange/ThenChange` and the
/// global compat-form `Label/EndLabel`). To add per-pattern compat
/// markers, use [`extract_blocks_with_markers`].
#[must_use]
pub fn extract_blocks(content: &str, file: &str) -> MarkerParseReport {
    extract_blocks_with_markers(content, file, &MarkerOverrides::none())
}

/// Like [`extract_blocks`] but additionally tries the open / close
/// regexes in `extras`. Used by the workspace scanner to plumb
/// per-pattern `LabelConfig` (DESIGN §10.3) into the parser.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn extract_blocks_with_markers(
    content: &str,
    file: &str,
    extras: &MarkerOverrides<'_>,
) -> MarkerParseReport {
    let mut blocks = Vec::new();
    let mut errors = Vec::new();

    // Pending open: line, id, no-verify reason, marker-form. Form
    // determines which Orphan* variant we emit if the block doesn't
    // close.
    let mut open: Option<(u32, Option<String>, Option<String>, MarkerForm)> = None;
    // Carries the previous line's NoVerify reason forward by exactly
    // one line, so a NoVerify *above* the IfChange line is honoured.
    let mut prev_no_verify: Option<String> = None;

    for (zero_idx, line) in content.lines().enumerate() {
        let line_num = u32::try_from(zero_idx + 1).unwrap_or(u32::MAX);

        // Capture NoVerify on this line for use by IfChange on the
        // same line OR the next line.
        let this_no_verify = NO_VERIFY_RE.captures(line).ok().flatten().map(|c| {
            c.name("reason")
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default()
        });

        // Match an OPEN marker. Order matters: canonical
        // `IfChange(...)` first, then global `Label(...)` (compat),
        // then any per-pattern compat regexes in `extras.opens`.
        // The keyword string is used by `balanced_id_from` to skip
        // past the marker keyword when extracting an id with nested
        // parens (Shape C). For per-pattern regexes we use the
        // matched text length as a proxy since the parser doesn't
        // know the keyword statically.
        let mut open_match: Option<(fancy_regex::Captures<'_>, usize, MarkerForm)> = IF_CHANGE_RE
            .captures(line)
            .ok()
            .flatten()
            .map(|c| (c, "IfChange".len(), MarkerForm::Canonical));
        if open_match.is_none() {
            open_match = LABEL_OPEN_RE
                .captures(line)
                .ok()
                .flatten()
                .map(|c| (c, "Label".len(), MarkerForm::Compat));
        }
        if open_match.is_none() {
            for re in extras.opens {
                if let Ok(Some(cap)) = re.captures(line) {
                    let m0 = cap.get(0).expect("group 0 always present");
                    // For per-pattern regexes, the "keyword length"
                    // is the full first-match slice's byte length up
                    // to the first `(` if any, else the full match.
                    let full = m0.as_str();
                    let kw_len = full.find('(').unwrap_or(full.len());
                    open_match = Some((cap, kw_len, MarkerForm::Compat));
                    break;
                }
            }
        }

        if let Some((cap, kw_len, form)) = open_match {
            // Reject lines that *also* contain ThenChange/EndLabel on the
            // same line (would otherwise produce a degenerate block).
            // Per DESIGN, the markers occupy their own lines; we treat
            // a same-line both-markers form as `OrphanIfChange` (or
            // its compat sibling `OrphanLabel`).
            if open.is_some() {
                // Nested or overlapping open — close the pending
                // open as orphan-style error and keep going.
                if let Some((open_line, _, _, pending_form)) = open.take() {
                    errors.push(orphan_open_error(file, open_line, pending_form));
                }
            }
            // Capture the id with paren-balanced extraction so Shape C
            // ids like `JIRA(PROJ-1234)` aren't truncated at the first
            // `)`. We can't do this in the regex (fancy-regex doesn't
            // support balanced groups portably); hand-rolled is small.
            // Fall back to the regex's capture for ids without nested
            // parens. The keyword-length is the byte offset from the
            // match start to the first `(` (or full match for
            // marker-only opens).
            let m0 = cap.get(0).expect("group 0 always present");
            let id = balanced_id_from_len(line, m0.start(), kw_len)
                .or_else(|| cap.name("id").map(|m| m.as_str().trim().to_string()));
            let id = id.map(|raw| strip_matching_quotes(raw.trim()).to_string());
            // Take the higher-priority NoVerify: same-line wins;
            // otherwise the line-above reason.
            let nv = this_no_verify.clone().or_else(|| prev_no_verify.clone());
            open = Some((line_num, id, nv, form));
            // Same-line NoVerify gets consumed by the just-opened
            // block; clear so it doesn't leak to a sibling later.
            prev_no_verify = None;
            continue;
        }

        // Match a CLOSE marker. Canonical `ThenChange(...)` may carry
        // targets; the compat-form `EndLabel` + per-pattern regexes
        // never do (close markers in the compat surface are
        // delimiter-only). Order: ThenChange → EndLabel → extras.
        let close_match: Option<(String, MarkerForm)> =
            if let Ok(Some(cap)) = THEN_CHANGE_RE.captures(line) {
                Some((
                    cap.name("targets")
                        .map(|m| m.as_str().trim().to_string())
                        .unwrap_or_default(),
                    MarkerForm::Canonical,
                ))
            } else if LABEL_CLOSE_RE.is_match(line).unwrap_or(false)
                || extras
                    .closes
                    .iter()
                    .any(|re| re.is_match(line).unwrap_or(false))
            {
                // Either the global `EndLabel` marker or any per-pattern
                // configured close — both feed the compat-form path.
                Some((String::new(), MarkerForm::Compat))
            } else {
                None
            };

        if let Some((targets_text, close_form)) = close_match {
            let Some((open_line, id, nv_reason, open_form)) = open.take() else {
                errors.push(orphan_close_error(file, line_num, close_form));
                prev_no_verify = this_no_verify;
                continue;
            };
            let mut targets = Vec::new();
            for raw in split_targets(&targets_text) {
                let raw = raw.trim();
                if raw.is_empty() {
                    continue;
                }
                match parse_target(raw) {
                    Ok(t) => targets.push(t),
                    Err(()) => errors.push(MarkerParseError::MalformedTarget {
                        file: file.to_string(),
                        line: line_num,
                        token: raw.to_string(),
                    }),
                }
            }
            // The block's marker_form is Compat if either the open
            // *or* the close came from the compat surface. Canonical
            // is only when BOTH were canonical.
            let block_form = match (open_form, close_form) {
                (MarkerForm::Canonical, MarkerForm::Canonical) => MarkerForm::Canonical,
                _ => MarkerForm::Compat,
            };
            blocks.push(IfChangeBlock {
                file: file.to_string(),
                line_start: open_line,
                line_end: line_num,
                id: id.filter(|s| !s.is_empty()),
                targets,
                no_verify_reason: nv_reason,
                marker_form: block_form,
            });
            prev_no_verify = None;
            continue;
        }

        // Neither marker on this line — let the NoVerify carry one line.
        prev_no_verify = this_no_verify;
    }

    // Leftover open: no closing marker. Emit with the form that
    // opened it (compat opens get OrphanLabel, canonical get
    // OrphanIfChange).
    if let Some((open_line, _, _, pending_form)) = open {
        errors.push(orphan_open_error(file, open_line, pending_form));
    }

    MarkerParseReport { blocks, errors }
}

/// Convenience: pick the right OrphanOpen-style variant for a form.
fn orphan_open_error(file: &str, line: u32, form: MarkerForm) -> MarkerParseError {
    match form {
        MarkerForm::Canonical => MarkerParseError::OrphanIfChange {
            file: file.to_string(),
            line,
        },
        MarkerForm::Compat => MarkerParseError::OrphanLabel {
            file: file.to_string(),
            line,
        },
    }
}

/// Convenience: pick the right OrphanClose-style variant for a form.
fn orphan_close_error(file: &str, line: u32, form: MarkerForm) -> MarkerParseError {
    match form {
        MarkerForm::Canonical => MarkerParseError::OrphanThenChange {
            file: file.to_string(),
            line,
        },
        MarkerForm::Compat => MarkerParseError::OrphanEndLabel {
            file: file.to_string(),
            line,
        },
    }
}

/// Split a `ThenChange(...)` arg list on commas. Commas inside `{...}`
/// brace groups (glob-flag sets like `{any,soft}`) don't separate
/// targets — they're flag-list separators handled downstream by
/// `strip_glob_flag`. Nested parens for Shape C ids aren't possible
/// in this position (`ThenChange` targets are paths/labels/anchors/
/// globs, never function-call-shaped ids), so we don't track them.
fn split_targets(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut brace_depth: usize = 0;
    for c in s.chars() {
        match c {
            '{' => {
                brace_depth += 1;
                current.push(c);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(c);
            }
            ',' if brace_depth == 0 => {
                out.push(std::mem::take(&mut current).trim().to_string());
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() || !out.is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Strip a trailing `{...}` glob-flag set if present.
///
/// Returns `(stripped_body, Some(flags))` on success; `(raw, None)`
/// when no flag braces are present. Body is comma-separated tokens
/// drawn from `any` / `all` / `soft`. Unknown tokens (typos like
/// `{andy}`) and contradictions (`{any,all}` — modes are mutually
/// exclusive) are rejected as malformed.
fn strip_glob_flag(raw: &str) -> Result<(&str, Option<GlobFlags>), ()> {
    if !raw.ends_with('}') {
        return Ok((raw, None));
    }
    let Some(open) = raw.rfind('{') else {
        return Ok((raw, None));
    };
    let body = &raw[open + 1..raw.len() - 1];
    let head = &raw[..open];
    let mut mode: Option<GlobMode> = None;
    let mut soft = false;
    for token in body.split(',').map(str::trim) {
        match token {
            "any" => {
                if mode.is_some() {
                    return Err(());
                }
                mode = Some(GlobMode::Any);
            }
            "all" => {
                if mode.is_some() {
                    return Err(());
                }
                mode = Some(GlobMode::All);
            }
            "soft" => {
                if soft {
                    return Err(());
                }
                soft = true;
            }
            _ => return Err(()),
        }
    }
    Ok((
        head,
        Some(GlobFlags {
            mode: mode.unwrap_or_default(),
            soft,
        }),
    ))
}

/// Extract the paren-balanced id text following `IfChange` on `line`,
/// starting at `marker_start` (the position of the `I` in `IfChange`).
///
/// Returns `None` when the marker has no parens (Shape A) or the
/// parens are malformed. Returns `Some("")` for bare `IfChange()`.
/// Handles nested parens so `IfChange(JIRA(PROJ-1234))` yields
/// `JIRA(PROJ-1234)` — what the v0.2 marker regex `[^)]*` truncated
/// at the first `)`.
fn balanced_id_from_len(line: &str, marker_start: usize, keyword_len: usize) -> Option<String> {
    // Skip past the matched keyword (e.g. `IfChange`, `Label`) — the
    // regex already matched these chars, so just advance the index.
    let after_keyword = marker_start.checked_add(keyword_len)?;
    let bytes = line.as_bytes();
    if bytes.get(after_keyword) != Some(&b'(') {
        return None;
    }
    let mut depth = 1usize;
    let mut i = after_keyword + 1;
    let start_inside = i;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    // Slice may not land on UTF-8 boundaries if the id
                    // contains multi-byte chars; defensively use
                    // `get` to bail out.
                    return line.get(start_inside..i).map(str::to_string);
                }
            }
            _ => {}
        }
        i += 1;
    }
    // Unbalanced parens — fall back to None so caller uses the regex
    // capture (which truncated at first `)`, matching pre-fix behaviour).
    None
}

/// Strip matching surrounding `'...'` or `"..."` quotes if present.
/// Returns the original slice unchanged when the quotes don't match
/// or aren't present.
fn strip_matching_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return s;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
        // Safe: ASCII quote characters are 1 byte each.
        return &s[1..s.len() - 1];
    }
    s
}

/// Parse a single target token. Supports:
///
/// - `path`
/// - `path:N`
/// - `path:N-M`
/// - `path:label-name`
/// - `path#anchor`
/// - `path-glob` (containing `*` / `?` / `[...]`)
/// - any of the above with a trailing `{any|all|soft}` flag set —
///   comma-separated tokens inside one brace pair. Only meaningful
///   on glob patterns; for non-globs the flag is parsed but ignored.
///
/// Anything else is `MalformedTarget` at the caller. Flag-parsing
/// happens first, then `#` vs `:` disambiguator.
fn parse_target(raw: &str) -> Result<Target, ()> {
    // Strip a trailing `{...}` flag set if present.
    let (raw, flags) = strip_glob_flag(raw)?;

    // Glob detection: pattern characters anywhere in the body trigger
    // the glob branch. Globs are mutually exclusive with `:` /  `#`
    // sub-target syntax — once we see a glob meta-character we treat
    // the whole token as a glob pattern.
    if raw.contains('*') || raw.contains('?') || raw.contains('[') {
        if raw.is_empty() {
            return Err(());
        }
        return Ok(Target::FileGlob {
            pattern: raw.to_string(),
            flags: flags.unwrap_or_default(),
        });
    }

    // A flag on a non-glob target is meaningless (DESIGN §10.2's
    // table says these flags apply to globs); accept silently to
    // preserve forward-compat with future flag additions.
    let _ = flags;
    // Anchor form `path#anchor` first.
    if let Some((path, anchor)) = raw.split_once('#') {
        if path.is_empty() || anchor.is_empty() {
            return Err(());
        }
        return Ok(Target::FileAnchor {
            path: path.to_string(),
            anchor: anchor.to_string(),
        });
    }
    if let Some((path, rest)) = raw.split_once(':') {
        if rest.is_empty() {
            return Err(());
        }
        // path may be empty when raw starts with ":" — same-file
        // shortcut. v0.2 doesn't support that yet; reject.
        if path.is_empty() {
            return Err(());
        }
        // Disambiguator (DESIGN §10.2): a `:` followed by digits or
        // `N-M` is a line/range; anything else is a label-name.
        if let Some((a, b)) = rest.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                if a == 0 || b == 0 || a > b {
                    return Err(());
                }
                return Ok(Target::FileLineRange {
                    path: path.to_string(),
                    start: a,
                    end: b,
                });
            }
            // Suffix has a hyphen but isn't a numeric range — fall
            // through to label-name handling below (slug-style labels
            // commonly contain hyphens, e.g. `argon2-params`).
        }
        if let Ok(n) = rest.parse::<u32>() {
            if n == 0 {
                return Err(());
            }
            return Ok(Target::FileLine {
                path: path.to_string(),
                line: n,
            });
        }
        // Not digits and not a numeric range → label-name.
        // Validate: labels are non-empty and don't contain `:` (would
        // re-trigger ambiguity) or whitespace.
        if rest.contains(':') || rest.chars().any(char::is_whitespace) {
            return Err(());
        }
        return Ok(Target::FileLabel {
            path: path.to_string(),
            label: rest.to_string(),
        });
    }
    // Bare path. Reject empty.
    if raw.is_empty() {
        return Err(());
    }
    Ok(Target::File {
        path: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_basic_shape_a_block_with_explicit_targets() {
        let src = "\
// IfChange
fn x() {}
// ThenChange(/docs/x.md, /src/y.rs:42, /src/z.rs:10-20)
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.errors, Vec::new());
        assert_eq!(report.blocks.len(), 1);
        let b = &report.blocks[0];
        assert_eq!(b.line_start, 1);
        assert_eq!(b.line_end, 3);
        assert_eq!(b.id, None);
        assert_eq!(b.targets.len(), 3);
        assert!(matches!(b.targets[0], Target::File { ref path } if path == "/docs/x.md"));
        assert!(
            matches!(b.targets[1], Target::FileLine { ref path, line: 42 } if path == "/src/y.rs")
        );
        assert!(
            matches!(b.targets[2], Target::FileLineRange { ref path, start: 10, end: 20 } if path == "/src/z.rs")
        );
    }

    #[test]
    fn test_extract_shape_b_id_block() {
        let src = "\
// IfChange(auth-format-v3)
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty());
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.blocks[0].id.as_deref(), Some("auth-format-v3"));
        assert!(report.blocks[0].targets.is_empty());
    }

    #[test]
    fn test_orphan_if_change_reported() {
        let src = "\
// IfChange
fn x() {}
// nothing else
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 0);
        assert_eq!(report.errors.len(), 1);
        assert!(
            matches!(report.errors[0], MarkerParseError::OrphanIfChange { ref file, line: 1 } if file == "a.rs")
        );
    }

    #[test]
    fn test_orphan_then_change_reported() {
        let src = "\
// stuff
// ThenChange(/x.md)
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 0);
        assert_eq!(report.errors.len(), 1);
        assert!(
            matches!(report.errors[0], MarkerParseError::OrphanThenChange { ref file, line: 2 } if file == "a.rs"),
            "got: {:#?}",
            report.errors
        );
    }

    #[test]
    fn test_multiple_blocks_in_one_file_each_pair() {
        let src = "\
// IfChange(a)
fn aa() {}
// ThenChange
// IfChange(b)
fn bb() {}
// ThenChange(/x.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks.len(), 2);
        assert_eq!(report.blocks[0].id.as_deref(), Some("a"));
        assert_eq!(report.blocks[1].id.as_deref(), Some("b"));
    }

    #[test]
    fn test_malformed_target_reported_other_targets_kept() {
        // `:zz` is a valid *label-name* under v0.2's
        // labels-after-non-numeric-colon rule, so use a label that
        // collides with a line/range numeric (`/bad.md:0`) — line 0
        // is rejected as out-of-range.
        let src = "\
// IfChange
fn x() {}
// ThenChange(/ok.md, /bad.md:0, /also-ok.md:5)
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.blocks[0].targets.len(), 2);
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::MalformedTarget { ref token, .. } if token == "/bad.md:0"
        ));
    }

    #[test]
    fn test_no_verify_inline_same_line_honoured() {
        let src = "\
// IfChange — NoVerify(coderef:ifchange): one-shot refactor
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(
            report.blocks[0].no_verify_reason.as_deref(),
            Some("one-shot refactor")
        );
    }

    #[test]
    fn test_no_verify_inline_line_above_honoured() {
        let src = "\
// NoVerify(coderef:ifchange): peer block intentionally lagging
// IfChange
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(
            report.blocks[0].no_verify_reason.as_deref(),
            Some("peer block intentionally lagging")
        );
    }

    #[test]
    fn test_no_verify_two_lines_above_not_honoured() {
        let src = "\
// NoVerify(coderef:ifchange): too far up
// unrelated comment
// IfChange
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].no_verify_reason.is_none());
    }

    #[test]
    fn test_nested_open_reported_as_orphan_inner_pair_still_parses() {
        // Sequence: IfChange, IfChange, ThenChange. The first
        // IfChange is orphaned (close not paired before the second
        // open consumes the next ThenChange).
        let src = "\
// IfChange
// IfChange
fn x() {}
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::OrphanIfChange { line: 1, .. }
        ));
    }

    #[test]
    fn test_then_change_with_no_args_yields_shape_b_block_with_no_targets() {
        let src = "\
// IfChange(grp)
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.blocks[0].id.as_deref(), Some("grp"));
    }

    #[test]
    fn test_then_change_arg_with_only_whitespace_yields_no_targets_no_error() {
        let src = "\
// IfChange
x
// ThenChange( , , )
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].targets.is_empty());
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
    }

    #[test]
    fn test_parse_anchor_target() {
        let src = "\
// IfChange
x
// ThenChange(/docs/security.md#hashing)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks.len(), 1);
        assert_eq!(report.blocks[0].targets.len(), 1);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileAnchor { ref path, ref anchor }
                if path == "/docs/security.md" && anchor == "hashing"
        ));
    }

    #[test]
    fn test_parse_anchor_target_with_hyphens_and_digits_in_anchor() {
        // Real-world heading slugs include hyphens, digits, and the
        // github double-hyphen.
        let src = "\
// IfChange
x
// ThenChange(/docs/x.md#argon2-params, /docs/y.md#section--2)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].targets.len(), 2);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileAnchor { ref anchor, .. } if anchor == "argon2-params"
        ));
        assert!(matches!(
            report.blocks[0].targets[1],
            Target::FileAnchor { ref anchor, .. } if anchor == "section--2"
        ));
    }

    #[test]
    fn test_parse_anchor_target_with_empty_anchor_rejected() {
        let src = "\
// IfChange
x
// ThenChange(/docs/x.md#)
";
        let report = extract_blocks(src, "a.rs");
        // The first target malforms (empty anchor); no successful
        // targets land, but the block still parses.
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::MalformedTarget { ref token, .. } if token == "/docs/x.md#"
        ));
    }

    #[test]
    fn test_parse_anchor_target_with_empty_path_rejected() {
        let src = "\
// IfChange
x
// ThenChange(#dangling)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_parse_anchor_and_line_range_targets_coexist_in_same_marker() {
        let src = "\
// IfChange
x
// ThenChange(/docs/x.md#hashing, /src/y.rs:10-20, /a.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        let ts = &report.blocks[0].targets;
        assert_eq!(ts.len(), 3);
        assert!(matches!(ts[0], Target::FileAnchor { .. }));
        assert!(matches!(ts[1], Target::FileLineRange { .. }));
        assert!(matches!(ts[2], Target::File { .. }));
    }

    #[test]
    fn test_parse_label_target_alphanumeric_suffix() {
        let src = "\
// IfChange
x
// ThenChange(/docs/security.md:argon2-params)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].targets.len(), 1);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileLabel { ref path, ref label }
                if path == "/docs/security.md" && label == "argon2-params"
        ));
    }

    #[test]
    fn test_parse_label_target_disambiguator_digits_win() {
        // `:42` is a line number, not a label called "42".
        let src = "\
// IfChange
x
// ThenChange(/a.rs:42)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileLine { .. }
        ));
    }

    #[test]
    fn test_parse_label_target_with_hyphens_treated_as_label_not_range() {
        // `:foo-bar` is a label called "foo-bar" — not a range,
        // because "foo" and "bar" don't parse as u32.
        let src = "\
// IfChange
x
// ThenChange(/a.rs:foo-bar)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileLabel { ref label, .. } if label == "foo-bar"
        ));
    }

    #[test]
    fn test_parse_label_target_with_double_colon_rejected() {
        let src = "\
// IfChange
x
// ThenChange(/a.rs:foo:bar)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_if_change_strips_matching_single_quotes_around_id() {
        // README's canonical form uses `IfChange('hash-params')`.
        // Should produce id = "hash-params", not "'hash-params'".
        let src = "\
// IfChange('hash-params')
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("hash-params"));
    }

    #[test]
    fn test_if_change_strips_matching_double_quotes_around_id() {
        let src = "\
// IfChange(\"my-id\")
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("my-id"));
    }

    #[test]
    fn test_if_change_preserves_bare_id_without_quotes() {
        let src = "\
// IfChange(my-id)
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks[0].id.as_deref(), Some("my-id"));
    }

    #[test]
    fn test_if_change_preserves_unmatched_quote() {
        // A leading-only quote isn't matched; preserved as-is.
        let src = "\
// IfChange('mismatched)
x
// ThenChange
";
        let report = extract_blocks(src, "a.rs");
        assert_eq!(report.blocks[0].id.as_deref(), Some("'mismatched"));
    }

    #[test]
    fn test_parse_label_and_line_targets_coexist_in_same_marker() {
        let src = "\
// IfChange
x
// ThenChange(/a.md:my-section, /b.rs:42, /c.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        let ts = &report.blocks[0].targets;
        assert_eq!(ts.len(), 3);
        assert!(matches!(ts[0], Target::FileLabel { .. }));
        assert!(matches!(ts[1], Target::FileLine { .. }));
        assert!(matches!(ts[2], Target::File { .. }));
    }

    #[test]
    fn test_parse_glob_target_defaults_to_any() {
        let src = "\
// IfChange
x
// ThenChange(/docs/api/*.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileGlob {
                ref pattern,
                flags: GlobFlags { mode: GlobMode::Any, soft: false },
            }
                if pattern == "/docs/api/*.md"
        ));
    }

    #[test]
    fn test_parse_glob_target_with_all_flag() {
        let src = "\
// IfChange
x
// ThenChange(/docs/api/*.md{all})
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileGlob {
                flags: GlobFlags {
                    mode: GlobMode::All,
                    soft: false
                },
                ..
            }
        ));
    }

    #[test]
    fn test_parse_glob_target_with_any_flag_explicit() {
        let src = "\
// IfChange
x
// ThenChange(/docs/**/*.md{any})
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileGlob {
                flags: GlobFlags {
                    mode: GlobMode::Any,
                    soft: false
                },
                ..
            }
        ));
    }

    #[test]
    fn test_parse_glob_target_with_soft_flag() {
        let src = "\
// IfChange
x
// ThenChange(/docs/*.md{soft})
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        // `{soft}` alone: implicit any-mode + warning severity.
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileGlob {
                flags: GlobFlags {
                    mode: GlobMode::Any,
                    soft: true
                },
                ..
            }
        ));
    }

    #[test]
    fn test_parse_glob_target_with_all_and_soft_flag() {
        // `{all,soft}` and `{soft,all}` both legal — flags are
        // comma-separated, order-insensitive.
        for src in [
            "// IfChange\nx\n// ThenChange(/docs/*.md{all,soft})\n",
            "// IfChange\nx\n// ThenChange(/docs/*.md{soft,all})\n",
            "// IfChange\nx\n// ThenChange(/docs/*.md{all, soft})\n",
        ] {
            let report = extract_blocks(src, "a.rs");
            assert!(report.errors.is_empty(), "{src:?}: {:#?}", report.errors);
            assert!(
                matches!(
                    report.blocks[0].targets[0],
                    Target::FileGlob {
                        flags: GlobFlags {
                            mode: GlobMode::All,
                            soft: true
                        },
                        ..
                    }
                ),
                "src = {src:?}"
            );
        }
    }

    #[test]
    fn test_parse_glob_flag_duplicates_rejected() {
        // `{any,all}` is contradictory (mutually exclusive modes); the
        // parser refuses it as malformed.
        let src = "\
// IfChange
x
// ThenChange(/docs/*.md{any,all})
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
        // `{soft,soft}` is also rejected.
        let src = "\
// IfChange
x
// ThenChange(/docs/*.md{soft,soft})
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_parse_unknown_glob_flag_rejected() {
        // Unknown tokens are rejected as malformed (catches typos like
        // `{andy}` rather than silently treating them as the default).
        let src = "\
// IfChange
x
// ThenChange(/docs/*.md{andy})
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks[0].targets.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_parse_glob_target_with_question_mark() {
        let src = "\
// IfChange
x
// ThenChange(/docs/?.md)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(matches!(
            report.blocks[0].targets[0],
            Target::FileGlob { .. }
        ));
    }

    #[test]
    fn test_parse_glob_and_label_coexist_in_same_marker() {
        let src = "\
// IfChange
x
// ThenChange(/docs/*.md, /a.rs:my-block)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        let ts = &report.blocks[0].targets;
        assert_eq!(ts.len(), 2);
        assert!(matches!(ts[0], Target::FileGlob { .. }));
        assert!(matches!(ts[1], Target::FileLabel { .. }));
    }

    // -----------------------------------------------------------------
    // Label('name') ... EndLabel compat form (DESIGN §10.2).
    // -----------------------------------------------------------------

    #[test]
    fn test_label_endlabel_produces_block_with_id_and_no_targets() {
        let src = "\
// Label('my-region')
x
// EndLabel
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks.len(), 1);
        let b = &report.blocks[0];
        assert_eq!(b.id.as_deref(), Some("my-region"));
        assert!(b.targets.is_empty());
    }

    #[test]
    fn test_label_endlabel_id_quotes_stripped() {
        // `Label('x')`, `Label("x")`, and `Label(x)` all yield id="x".
        for src in [
            "// Label('x')\ny\n// EndLabel\n",
            "// Label(\"x\")\ny\n// EndLabel\n",
            "// Label(x)\ny\n// EndLabel\n",
        ] {
            let report = extract_blocks(src, "a.rs");
            assert!(report.errors.is_empty(), "{src:?}: {:#?}", report.errors);
            assert_eq!(report.blocks[0].id.as_deref(), Some("x"), "src = {src:?}");
        }
    }

    #[test]
    fn test_label_can_target_endlabel_block_via_thenchange() {
        // a.rs uses Label/EndLabel to declare a labelled region;
        // b.rs's IfChange/ThenChange targets it via `a.rs:my-block`.
        // Confirms that the compat form produces a block that's
        // discoverable from another file's ThenChange target.
        let label_src = "\
// Label('my-block')
let payload = 42;
// EndLabel
";
        let report = extract_blocks(label_src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("my-block"));
        // The block's line range frames the labeled region.
        assert_eq!(report.blocks[0].line_start, 1);
        assert_eq!(report.blocks[0].line_end, 3);
    }

    #[test]
    fn test_label_open_paired_with_thenchange_close() {
        // Symmetric cross-form: Label opens, ThenChange closes with
        // targets. Both markers are recognised in the same loop, so
        // they can mix.
        let src = "\
// Label('foo')
x
// ThenChange(/b.rs)
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("foo"));
        assert_eq!(report.blocks[0].targets.len(), 1);
        assert!(matches!(report.blocks[0].targets[0], Target::File { .. }));
    }

    #[test]
    fn test_ifchange_open_paired_with_endlabel_close() {
        // Symmetric cross-form: IfChange opens (no targets needed),
        // EndLabel closes. Block has the id but no targets.
        let src = "\
// IfChange('bar')
x
// EndLabel
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks[0].id.as_deref(), Some("bar"));
        assert!(report.blocks[0].targets.is_empty());
    }

    #[test]
    fn test_orphan_endlabel_reported() {
        // EndLabel with no preceding open marker — emits the
        // compat-form OrphanEndLabel variant (v0.5: per-pattern label
        // config tracking) so the doctor can surface this distinctly
        // from a stray canonical `ThenChange`.
        let src = "\
x
// EndLabel
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::OrphanEndLabel { .. }
        ));
    }

    #[test]
    fn test_orphan_label_reported() {
        // `Label` without a matching close — emits the compat-form
        // OrphanLabel variant. v0.5 split from OrphanIfChange so the
        // doctor's `label.orphanOpen` diagnostic can be compat-only.
        let src = "\
// Label('foo')
x
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(matches!(
            report.errors[0],
            MarkerParseError::OrphanLabel { .. }
        ));
    }

    #[test]
    fn test_endlabel_does_not_match_inside_word() {
        // `\bEndLabel\b` must not match inside a larger identifier
        // like `SuperEndLabelish`. Inverse: `Label` (without End-
        // prefix) shouldn't accidentally trigger on `EndLabel` either.
        let src = "\
// SuperEndLabelish is not a marker
let x = EndLabelExtra;
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.blocks.is_empty());
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
    }

    #[test]
    fn test_label_bare_open_no_id_is_orphan() {
        // `Label` without `('id')` is well-formed syntactically but
        // semantically meaningless — same as `IfChange` without an
        // id, which produces a block with id=None. Pair it with an
        // EndLabel to confirm it parses through cleanly.
        let src = "\
// Label
x
// EndLabel
";
        let report = extract_blocks(src, "a.rs");
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.blocks.len(), 1);
        assert!(report.blocks[0].id.is_none());
    }
}
