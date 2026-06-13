//! Markdown anchor verification (DESIGN.md §6.3).
//!
//! For `kind: "local"` references whose resolved target carries a
//! `#anchor` suffix, verify that the anchor matches a heading slug in
//! the target file. v0.2 ships the `github` slugifier (the de-facto
//! default for in-repo Markdown), the corresponding heading
//! extractor, and Pandoc-style explicit `{#id}` overrides.
//!
//! Scope decisions for v0.2:
//! - Markdown only (`.md`, `.markdown`). Other extensions yield
//!   `AnchorOutcome::Skipped` so the file still resolves.
//! - Slugifier choices `pandoc` / `gitlab` / `hugo` /
//!   `mkdocs-material` are accepted by the schema (so users don't get
//!   parse errors) but currently fall back to `github` semantics with
//!   a `Skipped` note. They land in a follow-up.
//! - `anchorVerify` modes: only `ifPresent` (the default) is
//!   exercised; `always` / `never` parse but degrade to `ifPresent`.

use std::fs;
use std::path::Path;

/// Outcome of looking up an anchor against a target file's headings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorOutcome {
    /// The anchor matched a heading slug (or an explicit `{#id}`).
    Found,
    /// The anchor wasn't found; `available` is a sample of nearby
    /// candidates for the diagnostic. `suggestion` is the
    /// Levenshtein-1 hit if one exists.
    NotFound {
        available_sample: Vec<String>,
        suggestion: Option<String>,
    },
    /// Verification is skipped — unsupported file extension, file
    /// unreadable, or similar non-fatal condition. The caller treats
    /// the reference as resolved (file existed) but doesn't fail it.
    Skipped { reason: String },
}

/// Verify a `#anchor` against the headings of `file_path` (Markdown).
/// `slugifier` is currently advisory — anything other than `github` /
/// `None` skips with a reason.
#[must_use]
pub fn verify_anchor(file_path: &Path, anchor: &str, slugifier: Option<&str>) -> AnchorOutcome {
    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(ext.as_str(), "md" | "markdown") {
        return AnchorOutcome::Skipped {
            reason: format!("anchor verification not implemented for `.{ext}` files in v0.2"),
        };
    }

    // Slugifier dispatch. v0.2 ships `github` (the default + most
    // common), `pandoc`, `gitlab`, `hugo`, and `mkdocs-material`. An
    // unrecognised name still scans the file but falls back to
    // github-style slugs — surfaced via the `Skipped` outcome with a
    // descriptive reason so authors notice the typo.
    let slug = slugifier.unwrap_or("github");
    if !matches!(
        slug,
        "github" | "pandoc" | "gitlab" | "hugo" | "mkdocs-material"
    ) {
        return AnchorOutcome::Skipped {
            reason: format!(
                "slugifier `{slug}` not recognised; supported: \
                 github, pandoc, gitlab, hugo, mkdocs-material"
            ),
        };
    }

    let content = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            return AnchorOutcome::Skipped {
                reason: format!("could not read `{}`: {e}", file_path.display()),
            };
        }
    };
    let headings = extract_headings(&content);
    // Use the configured slugifier; Heading::slug honours explicit_id
    // unconditionally and only slugifies the text when no override.
    let slugs: Vec<String> = headings.iter().map(|h| h.slug(slug)).collect();
    if slugs.iter().any(|s| s == anchor) {
        AnchorOutcome::Found
    } else {
        AnchorOutcome::NotFound {
            available_sample: slugs.iter().take(5).cloned().collect(),
            suggestion: levenshtein_1_match(anchor, &slugs),
        }
    }
}

/// One heading found in a Markdown source.
///
/// `anchor` is the *github-slugified* form of `text` (kept so the
/// old API stays callable). For per-slugifier anchor lookups,
/// `Heading::slug(slugifier)` re-computes the slug on demand,
/// preferring `explicit_id` (Pandoc `{#id}` override) when present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub anchor: String,
    /// `Some("custom-id")` when the heading line ended with
    /// `{#custom-id}` (Pandoc-style explicit anchor override).
    /// `None` when the slug should be derived from `text`.
    pub explicit_id: Option<String>,
}

impl Heading {
    /// Resolve this heading's anchor under the given slugifier name.
    /// `explicit_id` always wins; otherwise the slugifier is applied
    /// to `text`. Unknown slugifier names fall through to `github`.
    #[must_use]
    pub fn slug(&self, slugifier: &str) -> String {
        if let Some(id) = self.explicit_id.as_deref() {
            return id.to_string();
        }
        slugify(&self.text, slugifier)
    }
}

/// Slugify `text` per the named algorithm.
///
/// Unknown names fall back to `github` (and surface no diagnostic —
/// the named-slugifier dispatch in `verify_anchor` is the place that
/// says "I didn't recognise this"; this helper is just a pure
/// mapping).
#[must_use]
pub fn slugify(text: &str, slugifier: &str) -> String {
    match slugifier {
        "pandoc" => pandoc_slug(text),
        "gitlab" => gitlab_slug(text),
        "hugo" => hugo_slug(text),
        "mkdocs-material" => mkdocs_material_slug(text),
        _ => github_slug(text),
    }
}

/// Extract all ATX headings (`#`-prefixed) from `content`. Skips
/// headings inside fenced code blocks. Honours Pandoc-style
/// `{#explicit-id}` annotations on the heading line.
#[must_use]
pub fn extract_headings(content: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut fence_marker: Option<char> = None;
    for line in content.lines() {
        let trimmed = line.trim_start();

        // Fenced-code-block tracking — only ``` and ~~~ count;
        // length must be at least three. We don't bother with the
        // indented-code-block form because GitHub's heading
        // extraction ignores headings inside any code block, and
        // indented code blocks rarely contain `# ` patterns at the
        // very start of a line anyway.
        if let Some(fm) = fence_marker {
            if trimmed.starts_with(fm) && trimmed.chars().take_while(|&c| c == fm).count() >= 3 {
                in_fence = false;
                fence_marker = None;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = true;
            fence_marker = Some(trimmed.chars().next().unwrap());
            continue;
        }
        if in_fence {
            continue;
        }

        // ATX heading: 1..=6 leading `#`s, then a space, then the text.
        let leading_hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if leading_hashes == 0 || leading_hashes > 6 {
            continue;
        }
        let after_hashes = &trimmed[leading_hashes..];
        if !after_hashes.starts_with(' ') {
            continue;
        }
        let text_part = after_hashes.trim_start();
        // Strip trailing closing `#`s (some authors write `## Header ##`).
        let text_part = text_part.trim_end_matches(|c: char| c == '#' || c.is_whitespace());

        // Pandoc-style explicit id: `## Heading {#anchor-id}`. Match
        // greedily on the rightmost `{#...}` so heading text
        // containing literal `{` doesn't false-match.
        let (text, explicit_id) = split_explicit_id(text_part);
        let level = u8::try_from(leading_hashes).unwrap_or(1);
        let anchor = explicit_id
            .as_deref()
            .map_or_else(|| github_slug(&text), str::to_string);
        out.push(Heading {
            level,
            text,
            anchor,
            explicit_id,
        });
    }
    out
}

/// Detect a trailing `{#explicit-id}` on a heading line. Returns the
/// heading text (without the `{...}`) and the explicit id if present.
fn split_explicit_id(text: &str) -> (String, Option<String>) {
    let trimmed = text.trim_end();
    if !trimmed.ends_with('}') {
        return (trimmed.to_string(), None);
    }
    let Some(open) = trimmed.rfind("{#") else {
        return (trimmed.to_string(), None);
    };
    // Body is between `{#` and the closing `}`.
    let body = &trimmed[open + 2..trimmed.len() - 1];
    // Reject bodies with whitespace — those aren't ids.
    if body.is_empty() || body.contains(char::is_whitespace) {
        return (trimmed.to_string(), None);
    }
    let head_text = trimmed[..open].trim_end().to_string();
    (head_text, Some(body.to_string()))
}

/// GitHub-flavoured slug.
///
/// 1. Lowercase.
/// 2. Drop every character that isn't ASCII alphanumeric, space,
///    hyphen, or underscore.
/// 3. Replace spaces with hyphens.
///
/// Canonical example (DESIGN §6.3.2): `## My Heading & v2.0!` →
/// `my-heading--v20`. Consecutive hyphens are preserved — that's
/// GitHub's behaviour for adjacent dropped characters.
#[must_use]
pub fn github_slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() || lc == '-' || lc == '_' {
            out.push(lc);
        } else if lc == ' ' {
            out.push('-');
        }
        // Everything else (punctuation, &, !, ., etc.) is dropped.
    }
    out
}

/// Pandoc-flavoured slug.
///
/// Pandoc's `auto_identifiers` rule: lowercase; strip everything that
/// isn't a letter, digit, hyphen, underscore, or space; collapse
/// runs of spaces *and* runs of consecutive non-word characters into
/// a single hyphen; trim leading/trailing hyphens.
///
/// Canonical example (DESIGN §6.3.2): `## My Heading & v2.0!` →
/// `my-heading-v20` (single hyphen between `heading` and `v20`,
/// versus github's double).
#[must_use]
pub fn pandoc_slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_hyphen = false;
    for c in text.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() || lc == '_' {
            out.push(lc);
            last_was_hyphen = false;
        } else if (lc == ' ' || lc == '-') && !last_was_hyphen && !out.is_empty() {
            out.push('-');
            last_was_hyphen = true;
        }
        // Everything else is dropped without inserting a separator.
    }
    // Trim trailing hyphen.
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// GitLab-flavoured slug.
///
/// GitLab renders any non-alphanumeric run as a single hyphen, and
/// preserves embedded digits and periods alike (so `v2.0` becomes
/// `v2-0`, not `v20`). Trims leading/trailing hyphens.
///
/// Canonical example (DESIGN §6.3.2): `## My Heading & v2.0!` →
/// `my-heading-v2-0`.
#[must_use]
pub fn gitlab_slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_hyphen = false;
    for c in text.chars() {
        let lc = c.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() || lc == '_' {
            out.push(lc);
            last_was_hyphen = false;
        } else if !last_was_hyphen && !out.is_empty() {
            out.push('-');
            last_was_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Hugo-flavoured slug.
///
/// Hugo (with default `[markup.tableOfContents]` and goldmark) uses
/// the same algorithm as GitLab in the spec's canonical example, so
/// we share the implementation.
#[must_use]
pub fn hugo_slug(text: &str) -> String {
    gitlab_slug(text)
}

/// MkDocs-Material-flavoured slug.
///
/// MkDocs-Material (via `pymdown-extensions`'s default slugifier)
/// matches GitHub-style: drop punctuation entirely, replace spaces
/// with hyphens, preserve consecutive hyphens.
///
/// Canonical example (DESIGN §6.3.2): `## My Heading & v2.0!` →
/// `my-heading--v20`.
#[must_use]
pub fn mkdocs_material_slug(text: &str) -> String {
    github_slug(text)
}

/// Levenshtein-distance-1 match against `candidates`, returning the
/// first hit. Used to power "did you mean X?" hints on anchor misses.
#[must_use]
fn levenshtein_1_match(needle: &str, candidates: &[String]) -> Option<String> {
    for c in candidates {
        if levenshtein_at_most_1(needle, c) {
            return Some(c.clone());
        }
    }
    None
}

/// `true` iff the Levenshtein distance between `a` and `b` is ≤ 1.
/// Specialised — we don't need the full DP table.
fn levenshtein_at_most_1(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let diff = av.len().abs_diff(bv.len());
    if diff > 1 {
        return false;
    }
    let (short, long) = if av.len() <= bv.len() {
        (&av, &bv)
    } else {
        (&bv, &av)
    };
    if short.len() == long.len() {
        // Substitution case: exactly one position differs.
        let mut diffs = 0;
        for i in 0..short.len() {
            if short[i] != long[i] {
                diffs += 1;
                if diffs > 1 {
                    return false;
                }
            }
        }
        diffs == 1
    } else {
        // Insertion/deletion case: one extra char in `long`. Skip the
        // first mismatch position in `long`.
        let mut i = 0;
        let mut j = 0;
        let mut skipped = false;
        while i < short.len() && j < long.len() {
            if short[i] == long[j] {
                i += 1;
                j += 1;
            } else if skipped {
                return false;
            } else {
                skipped = true;
                j += 1;
            }
        }
        // OK as long as we consumed all of `short` and at most one
        // trailing char of `long`.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_slug_canonical_design_example() {
        // DESIGN.md §6.3.2 canonical example.
        assert_eq!(github_slug("My Heading & v2.0!"), "my-heading--v20");
    }

    #[test]
    fn test_github_slug_lowercases_and_preserves_hyphens() {
        assert_eq!(github_slug("Hash-Params"), "hash-params");
    }

    #[test]
    fn test_github_slug_drops_punctuation() {
        assert_eq!(github_slug("Section: One!"), "section-one");
    }

    #[test]
    fn test_github_slug_preserves_underscores() {
        assert_eq!(github_slug("snake_case_heading"), "snake_case_heading");
    }

    #[test]
    fn test_pandoc_slug_canonical_design_example() {
        // DESIGN.md §6.3.2 canonical example.
        assert_eq!(pandoc_slug("My Heading & v2.0!"), "my-heading-v20");
    }

    #[test]
    fn test_pandoc_slug_collapses_consecutive_separators() {
        // `My  Heading` (two spaces) and `My - Heading` (space-dash-space)
        // both collapse to a single hyphen.
        assert_eq!(pandoc_slug("My  Heading"), "my-heading");
        assert_eq!(pandoc_slug("My - Heading"), "my-heading");
    }

    #[test]
    fn test_pandoc_slug_trims_trailing_hyphens() {
        assert_eq!(pandoc_slug("Hello world!!!"), "hello-world");
    }

    #[test]
    fn test_gitlab_slug_canonical_design_example() {
        assert_eq!(gitlab_slug("My Heading & v2.0!"), "my-heading-v2-0");
    }

    #[test]
    fn test_gitlab_slug_period_becomes_hyphen() {
        // Unlike github (drops `.`), gitlab inserts a separator.
        assert_eq!(gitlab_slug("v2.0"), "v2-0");
    }

    #[test]
    fn test_hugo_slug_matches_gitlab_canonical_example() {
        // DESIGN §6.3.2 lists hugo and gitlab with identical output.
        assert_eq!(hugo_slug("My Heading & v2.0!"), "my-heading-v2-0");
    }

    #[test]
    fn test_mkdocs_material_slug_matches_github_canonical_example() {
        // DESIGN §6.3.2 lists mkdocs-material and github with identical output.
        assert_eq!(
            mkdocs_material_slug("My Heading & v2.0!"),
            "my-heading--v20"
        );
    }

    #[test]
    fn test_slugify_dispatches_to_each_named_slugifier() {
        let t = "My Heading & v2.0!";
        assert_eq!(slugify(t, "github"), "my-heading--v20");
        assert_eq!(slugify(t, "pandoc"), "my-heading-v20");
        assert_eq!(slugify(t, "gitlab"), "my-heading-v2-0");
        assert_eq!(slugify(t, "hugo"), "my-heading-v2-0");
        assert_eq!(slugify(t, "mkdocs-material"), "my-heading--v20");
    }

    #[test]
    fn test_slugify_unknown_name_falls_back_to_github() {
        assert_eq!(slugify("My Heading", "unknown-slug"), "my-heading");
    }

    #[test]
    fn test_heading_slug_honours_explicit_id_under_any_slugifier() {
        let h = Heading {
            level: 2,
            text: "My Heading".into(),
            anchor: "my-heading".into(),
            explicit_id: Some("custom-id".into()),
        };
        assert_eq!(h.slug("github"), "custom-id");
        assert_eq!(h.slug("pandoc"), "custom-id");
        assert_eq!(h.slug("gitlab"), "custom-id");
    }

    #[test]
    fn test_extract_headings_basic_atx() {
        let md = "# H1\n\n## H 2\n\n### Heading & v2";
        let hs = extract_headings(md);
        assert_eq!(hs.len(), 3);
        assert_eq!(hs[0].anchor, "h1");
        assert_eq!(hs[1].anchor, "h-2");
        assert_eq!(hs[2].anchor, "heading--v2");
    }

    #[test]
    fn test_extract_headings_skips_inside_fenced_code_block() {
        let md = "# Real\n\n```\n# Not a heading\n```\n\n# Real Two";
        let hs = extract_headings(md);
        assert_eq!(
            hs.iter().map(|h| h.anchor.clone()).collect::<Vec<_>>(),
            vec!["real", "real-two"]
        );
    }

    #[test]
    fn test_extract_headings_skips_inside_tilde_fenced_block() {
        let md = "# X\n\n~~~\n# Skipped\n~~~\n\n# Y";
        let hs = extract_headings(md);
        assert_eq!(
            hs.iter().map(|h| h.anchor.clone()).collect::<Vec<_>>(),
            vec!["x", "y"]
        );
    }

    #[test]
    fn test_extract_headings_honours_explicit_id() {
        let md = "## My Heading {#explicit-id}\n";
        let hs = extract_headings(md);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].anchor, "explicit-id");
        assert_eq!(hs[0].text, "My Heading");
    }

    #[test]
    fn test_extract_headings_strips_trailing_hashes() {
        let md = "## Closing Hashes ##\n";
        let hs = extract_headings(md);
        assert_eq!(hs[0].anchor, "closing-hashes");
    }

    #[test]
    fn test_extract_headings_seven_hashes_not_a_heading() {
        // ATX caps at 6. `#######` is treated as paragraph text.
        let md = "####### Not a heading\n# Yes\n";
        let hs = extract_headings(md);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].anchor, "yes");
    }

    #[test]
    fn test_verify_anchor_finds_existing_heading() {
        let dir = tempdir("found");
        let md = dir.join("doc.md");
        std::fs::write(&md, "# Hash Params\n\nbody\n## Argon2 Params\n").unwrap();
        assert_eq!(
            verify_anchor(&md, "argon2-params", Some("github")),
            AnchorOutcome::Found
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_verify_anchor_missing_returns_not_found_with_suggestion() {
        let dir = tempdir("missing");
        let md = dir.join("doc.md");
        std::fs::write(&md, "# Hashing\n\n## Params\n").unwrap();
        match verify_anchor(&md, "hashin", Some("github")) {
            AnchorOutcome::NotFound { suggestion, .. } => {
                assert_eq!(suggestion.as_deref(), Some("hashing"));
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_verify_anchor_unknown_extension_skipped() {
        let dir = tempdir("skip");
        let f = dir.join("doc.txt");
        std::fs::write(&f, "x").unwrap();
        match verify_anchor(&f, "anything", Some("github")) {
            AnchorOutcome::Skipped { reason } => assert!(reason.contains(".txt")),
            other => panic!("expected Skipped, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_verify_anchor_pandoc_resolves_with_pandoc_slug() {
        // The heading `## v2.0 Notes` produces different slugs:
        //   github  → `v20-notes`
        //   pandoc  → `v20-notes` (same as github for this example)
        //   gitlab  → `v2-0-notes`
        // We hand the verifier the pandoc-form anchor; only the
        // pandoc slugifier should resolve it.
        let dir = tempdir("pandoc");
        let md = dir.join("doc.md");
        std::fs::write(&md, "## v2.0 Notes\n").unwrap();
        assert_eq!(
            verify_anchor(&md, "v20-notes", Some("pandoc")),
            AnchorOutcome::Found
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_verify_anchor_gitlab_resolves_with_gitlab_slug() {
        let dir = tempdir("gitlab");
        let md = dir.join("doc.md");
        std::fs::write(&md, "## v2.0 Notes\n").unwrap();
        assert_eq!(
            verify_anchor(&md, "v2-0-notes", Some("gitlab")),
            AnchorOutcome::Found
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_verify_anchor_unknown_slugifier_skipped_with_supported_list() {
        let dir = tempdir("unknown");
        let md = dir.join("doc.md");
        std::fs::write(&md, "# X\n").unwrap();
        match verify_anchor(&md, "x", Some("kramdown")) {
            AnchorOutcome::Skipped { reason } => {
                assert!(reason.contains("kramdown"));
                assert!(reason.contains("supported"));
            }
            other => panic!("expected Skipped, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_levenshtein_at_most_1_handles_substitution_insertion_deletion() {
        assert!(levenshtein_at_most_1("hashing", "hashing"));
        assert!(levenshtein_at_most_1("hashing", "hashin")); // deletion
        assert!(levenshtein_at_most_1("hashin", "hashing")); // insertion
        assert!(levenshtein_at_most_1("hashing", "hasking")); // substitution
        assert!(!levenshtein_at_most_1("hashing", "hashed"));
        assert!(!levenshtein_at_most_1("a", "abc"));
    }

    fn tempdir(label: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "coderef-anchor-{label}-{pid}-{nanos}",
            pid = std::process::id(),
            nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
