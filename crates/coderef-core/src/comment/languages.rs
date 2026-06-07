//! Per-language comment-syntax descriptors.

/// Declarative description of a language's comment + string syntax.
///
/// `'static` because all built-in language defs are compile-time
/// constants; user-supplied languages will need an owned variant in a
/// later version.
#[derive(Clone, Debug)]
pub struct Language {
    /// Human-readable name (`"rust"`, `"python"`, …).
    pub name: &'static str,
    /// Line-comment introducers, e.g. `["//"]` for C-family.
    pub line_comments: &'static [&'static str],
    /// Block-comment delimiters: `(open, close)`.
    pub block_comments: &'static [(&'static str, &'static str)],
    /// String delimiters; matched as `<delim> ... <delim>` with
    /// backslash-escape awareness (no nested-delimiter support yet).
    pub string_delimiters: &'static [&'static str],
}

const C_FAMILY: Language = Language {
    name: "c-family",
    line_comments: &["//"],
    block_comments: &[("/*", "*/")],
    string_delimiters: &["\"", "'"],
};

const RUST: Language = Language {
    name: "rust",
    line_comments: &["//"],
    block_comments: &[("/*", "*/")],
    string_delimiters: &["\""],
};

const PYTHON: Language = Language {
    name: "python",
    line_comments: &["#"],
    block_comments: &[],
    string_delimiters: &["\"\"\"", "'''", "\"", "'"],
};

const HASH_ONLY: Language = Language {
    name: "hash-only",
    line_comments: &["#"],
    block_comments: &[],
    string_delimiters: &["\""],
};

const LUA: Language = Language {
    name: "lua",
    line_comments: &["--"],
    block_comments: &[("--[[", "]]")],
    string_delimiters: &["\"", "'"],
};

const SQL: Language = Language {
    name: "sql",
    line_comments: &["--"],
    block_comments: &[("/*", "*/")],
    string_delimiters: &["'"],
};

const LISP: Language = Language {
    name: "lisp",
    line_comments: &[";"],
    block_comments: &[("#|", "|#")],
    string_delimiters: &["\""],
};

const HTML: Language = Language {
    name: "html",
    line_comments: &[],
    block_comments: &[("<!--", "-->")],
    string_delimiters: &["\"", "'"],
};

const MARKDOWN: Language = Language {
    name: "markdown",
    line_comments: &[],
    // No block-comment delimiters in v0.1: detecting `<!-- -->` correctly
    // requires skipping fenced-code-blocks + inline-backticks (otherwise
    // a literal `<!--` inside a code example, like in DESIGN.md, starts
    // a spurious comment range that swallows the rest of the doc until
    // the next `-->`). Markdown-aware comment parsing lands in v0.2;
    // until then, `commentsOnly: true` matches nothing in .md files —
    // the safe default per DESIGN §5.4.1.
    block_comments: &[],
    string_delimiters: &[],
};

/// Look up a language descriptor by file extension (without the leading
/// dot, case-insensitive). Returns `None` for unknown extensions; the
/// scanner treats `None` as "no detected comment regions."
#[must_use]
pub fn language_for_extension(ext: &str) -> Option<&'static Language> {
    let ext_lower = ext.to_ascii_lowercase();
    match ext_lower.as_str() {
        // C-family.
        "c" | "h" | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" | "m" | "mm" | "java" | "kt"
        | "kts" | "scala" | "swift" | "go" | "cs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
        | "dart" | "groovy" => Some(&C_FAMILY),
        "rs" => Some(&RUST),
        "py" | "pyi" | "pyw" => Some(&PYTHON),
        "rb" | "sh" | "bash" | "zsh" | "fish" | "yaml" | "yml" | "toml" | "ini" | "cfg"
        | "conf" | "pl" | "pm" | "tf" | "tfvars" | "dockerfile" | "mk" | "makefile" | "gemspec"
        | "rake" => Some(&HASH_ONLY),
        "lua" => Some(&LUA),
        "sql" => Some(&SQL),
        "lisp" | "lsp" | "el" | "clj" | "cljs" | "scm" | "ss" => Some(&LISP),
        "html" | "htm" | "xml" | "svg" | "xhtml" => Some(&HTML),
        "md" | "markdown" | "mdx" => Some(&MARKDOWN),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_for_extension_rust() {
        let lang = language_for_extension("rs").unwrap();
        assert_eq!(lang.name, "rust");
    }

    #[test]
    fn test_language_for_extension_is_case_insensitive() {
        assert!(language_for_extension("PY").is_some());
        assert!(language_for_extension("CPP").is_some());
    }

    #[test]
    fn test_language_for_extension_unknown_returns_none() {
        assert!(language_for_extension("unknownext").is_none());
    }

    #[test]
    fn test_language_for_extension_python_has_triple_quote_strings_first() {
        let lang = language_for_extension("py").unwrap();
        // Triple-quote delimiters must come before single-quote so the
        // matcher consumes them as one unit. Same for "'''" before "'".
        assert_eq!(lang.string_delimiters[0], "\"\"\"");
        assert_eq!(lang.string_delimiters[1], "'''");
    }

    #[test]
    fn test_language_for_extension_yaml_uses_hash_only() {
        let lang = language_for_extension("yaml").unwrap();
        assert_eq!(lang.line_comments, &["#"]);
        assert!(lang.block_comments.is_empty());
    }

    #[test]
    fn test_language_for_extension_markdown_has_no_block_comments_in_v0_1() {
        // Per the comment in languages.rs MARKDOWN: <!-- --> is a v0.2
        // feature; needs fenced-code-block awareness to avoid
        // false-positive comment ranges from <!-- literals inside ``` ```.
        let lang = language_for_extension("md").unwrap();
        assert!(
            lang.block_comments.is_empty(),
            "markdown block_comments must be empty in v0.1 to avoid the \
             DESIGN.md-class bug where <!-- inside backticks opens a \
             spurious comment range; v0.2 wires fenced-block-aware parsing",
        );
    }
}
