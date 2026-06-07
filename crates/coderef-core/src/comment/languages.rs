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
    // `<!-- -->` is the only comment delimiter. The detector special-
    // cases markdown to use the fenced-code-block-aware parser in
    // `super::markdown` instead of the generic one, so this descriptor
    // documents the language faithfully (matching what an editor host
    // would expose) without letting `<!--` inside backticks open a
    // spurious comment range.
    block_comments: &[("<!--", "-->")],
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
    fn test_language_for_extension_markdown_advertises_html_comments() {
        // v0.2 onwards: the descriptor faithfully advertises <!-- -->.
        // The detector special-cases markdown to use a fenced-block-
        // aware parser; see super::markdown::detect_markdown_comment_ranges.
        let lang = language_for_extension("md").unwrap();
        assert_eq!(lang.block_comments, &[("<!--", "-->")]);
    }
}
