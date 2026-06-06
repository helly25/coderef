//! `${name}` / `${namespace:argument}` parser.
//!
//! Splits an input template into alternating `Segment::Literal` and
//! `Segment::Variable` runs without performing any substitution. Resolver
//! callers walk the segments and look each variable up.
//!
//! Escape: `$${name}` produces a literal `${name}` (the leading `$$` is
//! consumed; the rest is treated as literal text).

use thiserror::Error;

/// One run of input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Segment {
    /// Literal text between (or after) variable placeholders.
    Literal(String),
    /// A `${...}` placeholder.
    Variable {
        /// `Some("env")` for `${env:NAME}`, `None` for bare `${NAME}`.
        namespace: Option<String>,
        name: String,
    },
}

/// Parser failures for `${...}` syntax.
#[derive(Debug, Error)]
pub enum SyntaxError {
    /// `${` opened without a matching `}`.
    #[error("unclosed variable expression starting at byte {0}")]
    UnclosedExpression(usize),

    /// An empty variable name (`${}` or `${ns:}`).
    #[error("empty variable name at byte {0}")]
    EmptyName(usize),

    /// Multiple colons or empty namespace (`${:foo}`, `${ns:foo:bar}`).
    #[error("malformed namespace at byte {0}: {1}")]
    MalformedNamespace(usize, String),
}

/// Parse a template string into segments.
pub fn parse_segments(template: &str) -> Result<Vec<Segment>, SyntaxError> {
    let bytes = template.as_bytes();
    let mut segments = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            // `$$` — consume one `$`, then emit the rest verbatim. If the
            // next bytes are `${name}`, that produces a literal `${name}`
            // in the buffer (the escape).
            buf.push('$');
            i += 2;
            continue;
        }
        if c == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            // Start of a `${...}` placeholder.
            if !buf.is_empty() {
                segments.push(Segment::Literal(std::mem::take(&mut buf)));
            }
            let start = i;
            let body_start = i + 2;
            let close = template[body_start..]
                .find('}')
                .ok_or(SyntaxError::UnclosedExpression(start))?;
            let body = &template[body_start..body_start + close];
            let (namespace, name) = parse_variable_body(body, start)?;
            segments.push(Segment::Variable { namespace, name });
            i = body_start + close + 1;
            continue;
        }
        // Plain literal byte. UTF-8 safe because we only branch on ASCII
        // bytes (`$`, `{`, `}`) which are single-byte in UTF-8.
        let ch_start = i;
        let ch_len = utf8_char_len(c);
        buf.push_str(&template[ch_start..ch_start + ch_len]);
        i += ch_len;
    }
    if !buf.is_empty() {
        segments.push(Segment::Literal(buf));
    }
    Ok(segments)
}

fn parse_variable_body(body: &str, start: usize) -> Result<(Option<String>, String), SyntaxError> {
    let body = body.trim();
    if body.is_empty() {
        return Err(SyntaxError::EmptyName(start));
    }
    let parts: Vec<&str> = body.splitn(2, ':').collect();
    match parts.as_slice() {
        [name] => {
            if name.is_empty() {
                Err(SyntaxError::EmptyName(start))
            } else {
                Ok((None, (*name).to_string()))
            }
        }
        [ns, name] => {
            let ns = ns.trim();
            let name = name.trim();
            if ns.is_empty() {
                Err(SyntaxError::MalformedNamespace(
                    start,
                    "empty namespace before `:`".into(),
                ))
            } else if name.is_empty() {
                Err(SyntaxError::EmptyName(start))
            } else {
                Ok((Some(ns.to_string()), name.to_string()))
            }
        }
        _ => unreachable!("splitn(2) yields at most 2 parts"),
    }
}

const fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1, // Invalid leading byte; treat as 1 to avoid panic.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Segment {
        Segment::Literal(s.to_string())
    }
    fn var(ns: Option<&str>, name: &str) -> Segment {
        Segment::Variable {
            namespace: ns.map(String::from),
            name: name.to_string(),
        }
    }

    #[test]
    fn test_parse_plain_text_yields_single_literal() {
        let s = parse_segments("hello world").unwrap();
        assert_eq!(s, vec![lit("hello world")]);
    }

    #[test]
    fn test_parse_empty_string_yields_no_segments() {
        let s = parse_segments("").unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn test_parse_bare_variable_extracts_name() {
        let s = parse_segments("${user}").unwrap();
        assert_eq!(s, vec![var(None, "user")]);
    }

    #[test]
    fn test_parse_namespaced_variable_extracts_both() {
        let s = parse_segments("${env:JIRA_TOKEN}").unwrap();
        assert_eq!(s, vec![var(Some("env"), "JIRA_TOKEN")]);
    }

    #[test]
    fn test_parse_literal_then_variable_then_literal() {
        let s = parse_segments("[${user}]").unwrap();
        assert_eq!(s, vec![lit("["), var(None, "user"), lit("]")]);
    }

    #[test]
    fn test_parse_multiple_variables_split_correctly() {
        let s = parse_segments("${a}-${b}").unwrap();
        assert_eq!(s, vec![var(None, "a"), lit("-"), var(None, "b")]);
    }

    #[test]
    fn test_parse_escape_double_dollar_yields_literal_dollar() {
        // $${user} → "${user}" literal (no variable).
        let s = parse_segments("$${user}").unwrap();
        assert_eq!(s, vec![lit("${user}")]);
    }

    #[test]
    fn test_parse_unclosed_expression_errors() {
        let err = parse_segments("${user").unwrap_err();
        assert!(matches!(err, SyntaxError::UnclosedExpression(_)));
    }

    #[test]
    fn test_parse_empty_name_errors() {
        let err = parse_segments("${}").unwrap_err();
        assert!(matches!(err, SyntaxError::EmptyName(_)));
    }

    #[test]
    fn test_parse_empty_namespace_errors() {
        let err = parse_segments("${:foo}").unwrap_err();
        assert!(matches!(err, SyntaxError::MalformedNamespace(_, _)));
    }

    #[test]
    fn test_parse_empty_name_after_namespace_errors() {
        let err = parse_segments("${env:}").unwrap_err();
        assert!(matches!(err, SyntaxError::EmptyName(_)));
    }

    #[test]
    fn test_parse_whitespace_around_name_is_trimmed() {
        let s = parse_segments("${ user }").unwrap();
        assert_eq!(s, vec![var(None, "user")]);
    }

    #[test]
    fn test_parse_whitespace_around_namespace_is_trimmed() {
        let s = parse_segments("${ env : KEY }").unwrap();
        assert_eq!(s, vec![var(Some("env"), "KEY")]);
    }

    #[test]
    fn test_parse_dollar_followed_by_non_brace_is_literal() {
        let s = parse_segments("$5.00").unwrap();
        assert_eq!(s, vec![lit("$5.00")]);
    }

    #[test]
    fn test_parse_handles_multibyte_utf8_between_placeholders() {
        let s = parse_segments("Müller ${user}").unwrap();
        assert_eq!(s, vec![lit("Müller "), var(None, "user")]);
    }
}
