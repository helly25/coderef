//! Variable resolver. See `DESIGN.md` §8.
//!
//! Implements `${name}` and `${namespace:argument}` substitution against
//! a `Context` carrying the live values for each namespace. Strict by
//! default: any unresolved variable returns `VariableError`; callers can
//! relax this per-call via `Context::with_strict(false)`.
//!
//! v0.1 namespaces: `builtin`, `capture`, `env`, `config`, `file`, `ref`,
//! `ide`. The `git` and `blame` namespaces ship in v0.2 / v0.3.

mod syntax;

pub use self::syntax::{parse_segments, Segment, SyntaxError};

use indexmap::IndexMap;
use thiserror::Error;

/// Variable namespaces. See `DESIGN.md` §8.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Namespace {
    /// Bare names; resolved against builtins first, then captures.
    Default,
    Builtin,
    Capture,
    Env,
    Config,
    File,
    Ref,
    Ide,
    /// v0.2.
    Git,
    /// `upgrade` rewrite only (v0.3).
    Blame,
}

impl Namespace {
    /// Parse a namespace name into the enum. Unknown names produce `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "builtin" => Some(Self::Builtin),
            "capture" => Some(Self::Capture),
            "env" => Some(Self::Env),
            "config" => Some(Self::Config),
            "file" => Some(Self::File),
            "ref" => Some(Self::Ref),
            "ide" => Some(Self::Ide),
            "git" => Some(Self::Git),
            "blame" => Some(Self::Blame),
            _ => None,
        }
    }

    /// True iff this namespace is implemented in v0.1.
    #[must_use]
    pub fn is_v0_1(self) -> bool {
        matches!(
            self,
            Self::Default
                | Self::Builtin
                | Self::Capture
                | Self::Env
                | Self::Config
                | Self::File
                | Self::Ref
                | Self::Ide
        )
    }
}

/// Variable-resolution context. Holds whatever per-call data the resolver
/// needs to fill in placeholders.
///
/// `Clone` lets the scanner derive a per-match context (base context
/// plus the new captures) without rebuilding from the config every time.
/// The `env` field is `Copy` because trait-object references are `Copy`.
#[derive(Clone, Default)]
pub struct Context<'a> {
    builtins: IndexMap<String, String>,
    captures: IndexMap<String, String>,
    env: Option<&'a dyn EnvProvider>,
    config: IndexMap<String, String>,
    file: IndexMap<String, String>,
    ref_meta: IndexMap<String, String>,
    ide: IndexMap<String, String>,
    strict: bool,
}

/// Trait abstracting over the host's process environment so a WASM target
/// can substitute a deterministic in-memory map without depending on
/// `std::env`.
pub trait EnvProvider {
    fn lookup(&self, key: &str) -> Option<String>;
}

/// Default implementation that reads `std::env::var`. Available on non-
/// `wasm32` targets only.
#[cfg(not(target_arch = "wasm32"))]
pub struct StdEnv;

#[cfg(not(target_arch = "wasm32"))]
impl EnvProvider for StdEnv {
    fn lookup(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Map-backed env provider, useful in tests and on WASM targets.
pub struct MapEnv(pub IndexMap<String, String>);

impl EnvProvider for MapEnv {
    fn lookup(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

impl<'a> Context<'a> {
    /// New empty context. Strict-mode is enabled by default.
    #[must_use]
    pub fn new() -> Self {
        Self {
            strict: true,
            ..Default::default()
        }
    }

    /// Toggle strict mode. With `strict = false`, an unresolved variable
    /// expands to an empty string rather than producing `VariableError`.
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Insert a builtin (`${workspaceFolder}`, `${homeDir}`, …).
    pub fn with_builtin(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.builtins.insert(name.into(), value.into());
        self
    }

    /// Insert a named regex capture.
    pub fn with_capture(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.captures.insert(name.into(), value.into());
        self
    }

    /// Set the env provider (process env on native, map on WASM/test).
    pub fn with_env(mut self, env: &'a dyn EnvProvider) -> Self {
        self.env = Some(env);
        self
    }

    /// Insert a user-defined config variable. Callers walk the
    /// `${config:variables.x}` namespace from `Config.variables` and
    /// feed it in flattened.
    pub fn with_config(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.insert(name.into(), value.into());
        self
    }

    /// Insert a `file:` namespace entry.
    pub fn with_file(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.file.insert(name.into(), value.into());
        self
    }

    /// Insert a `ref:` namespace entry.
    pub fn with_ref(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.ref_meta.insert(name.into(), value.into());
        self
    }

    /// Insert an `ide:` namespace entry.
    pub fn with_ide(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.ide.insert(name.into(), value.into());
        self
    }

    /// Resolve a template string by expanding every `${...}` placeholder.
    pub fn resolve(&self, template: &str) -> Result<String, VariableError> {
        let segments = parse_segments(template)?;
        let mut out = String::with_capacity(template.len());
        for seg in segments {
            match seg {
                Segment::Literal(s) => out.push_str(&s),
                Segment::Variable { namespace, name } => {
                    let resolved = self.lookup(&namespace, &name)?;
                    out.push_str(&resolved);
                }
            }
        }
        Ok(out)
    }

    fn lookup(&self, ns: &Option<String>, name: &str) -> Result<String, VariableError> {
        // Bare `${name}` resolves first as capture, then as builtin —
        // matching the §8.2 fallthrough.
        let found = match ns.as_deref() {
            None => self
                .captures
                .get(name)
                .or_else(|| self.builtins.get(name))
                .cloned(),
            Some("builtin") => self.builtins.get(name).cloned(),
            Some("capture") => self.captures.get(name).cloned(),
            Some("env") => self.env.and_then(|e| e.lookup(name)),
            Some("config") => self.config.get(name).cloned(),
            Some("file") => self.file.get(name).cloned(),
            Some("ref") => self.ref_meta.get(name).cloned(),
            Some("ide") => self.ide.get(name).cloned(),
            Some("git") => {
                return Err(VariableError::NamespaceNotYetImplemented {
                    namespace: "git".into(),
                    expected_version: "v0.3".into(),
                });
            }
            Some("blame") => {
                return Err(VariableError::NamespaceNotYetImplemented {
                    namespace: "blame".into(),
                    expected_version: "v0.3".into(),
                });
            }
            Some(other) => {
                return Err(VariableError::UnknownNamespace(other.to_string()));
            }
        };
        match found {
            Some(value) => Ok(value),
            None if self.strict => Err(VariableError::Unresolved {
                namespace: ns.clone(),
                name: name.into(),
            }),
            None => Ok(String::new()),
        }
    }
}

/// Failures from variable resolution.
#[derive(Debug, Error)]
pub enum VariableError {
    /// Syntax error in a `${...}` expression.
    #[error("variable syntax error: {0}")]
    Syntax(#[from] SyntaxError),

    /// Reference to a namespace that doesn't exist.
    #[error("unknown variable namespace: `{0}`")]
    UnknownNamespace(String),

    /// Reference to a namespace not yet implemented in this engine version.
    #[error(
        "namespace `{namespace}:` is not implemented in v0.1 (expected in {expected_version})"
    )]
    NamespaceNotYetImplemented {
        namespace: String,
        expected_version: String,
    },

    /// Variable not found and strict mode is on.
    #[error(
        "unresolved variable {}{}",
        match namespace { Some(ns) => format!("${{{ns}:{name}}}"), None => format!("${{{name}}}") },
        ""
    )]
    Unresolved {
        namespace: Option<String>,
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_bare_capture_expands() {
        let ctx = Context::new().with_capture("user", "marcus");
        assert_eq!(
            ctx.resolve("https://users.example.com/${user}").unwrap(),
            "https://users.example.com/marcus"
        );
    }

    #[test]
    fn test_resolve_explicit_capture_namespace_expands() {
        let ctx = Context::new().with_capture("user", "marcus");
        assert_eq!(
            ctx.resolve("https://x/${capture:user}").unwrap(),
            "https://x/marcus"
        );
    }

    #[test]
    fn test_resolve_builtin_workspace_folder_expands() {
        let ctx = Context::new().with_builtin("workspaceFolder", "/repo");
        assert_eq!(
            ctx.resolve("${workspaceFolder}/docs").unwrap(),
            "/repo/docs"
        );
    }

    #[test]
    fn test_resolve_bare_capture_takes_precedence_over_builtin() {
        let ctx = Context::new()
            .with_builtin("user", "BUILTIN")
            .with_capture("user", "CAPTURE");
        assert_eq!(ctx.resolve("${user}").unwrap(), "CAPTURE");
    }

    #[test]
    fn test_resolve_explicit_builtin_namespace_bypasses_capture() {
        let ctx = Context::new()
            .with_builtin("user", "BUILTIN")
            .with_capture("user", "CAPTURE");
        assert_eq!(ctx.resolve("${builtin:user}").unwrap(), "BUILTIN");
    }

    #[test]
    fn test_resolve_env_lookup_via_provider() {
        let mut env = IndexMap::new();
        env.insert("JIRA_TOKEN".into(), "abc123".into());
        let env_provider = MapEnv(env);
        let ctx = Context::new().with_env(&env_provider);
        assert_eq!(
            ctx.resolve("Bearer ${env:JIRA_TOKEN}").unwrap(),
            "Bearer abc123"
        );
    }

    #[test]
    fn test_resolve_unresolved_in_strict_mode_errors() {
        let ctx = Context::new();
        let err = ctx.resolve("${missing}").unwrap_err();
        assert!(matches!(err, VariableError::Unresolved { .. }));
    }

    #[test]
    fn test_resolve_unresolved_in_non_strict_mode_yields_empty() {
        let ctx = Context::new().with_strict(false);
        assert_eq!(ctx.resolve("[${missing}]").unwrap(), "[]");
    }

    #[test]
    fn test_resolve_unknown_namespace_errors() {
        let ctx = Context::new();
        let err = ctx.resolve("${bogus:foo}").unwrap_err();
        assert!(matches!(err, VariableError::UnknownNamespace(ref ns) if ns == "bogus"));
    }

    #[test]
    fn test_resolve_git_namespace_returns_not_yet_implemented() {
        let ctx = Context::new();
        let err = ctx.resolve("${git:branch}").unwrap_err();
        assert!(matches!(
            err,
            VariableError::NamespaceNotYetImplemented { ref namespace, .. } if namespace == "git"
        ));
    }

    #[test]
    fn test_resolve_blame_namespace_returns_not_yet_implemented() {
        let ctx = Context::new();
        let err = ctx.resolve("${blame:user}").unwrap_err();
        assert!(matches!(
            err,
            VariableError::NamespaceNotYetImplemented { ref namespace, .. } if namespace == "blame"
        ));
    }

    #[test]
    fn test_resolve_escape_double_dollar_yields_literal_dollar_brace() {
        let ctx = Context::new().with_capture("user", "marcus");
        // `$${user}` is the documented escape for a literal `${user}`.
        assert_eq!(ctx.resolve("$${user}").unwrap(), "${user}");
    }

    #[test]
    fn test_resolve_multiple_placeholders_in_one_template() {
        let ctx = Context::new()
            .with_capture("user", "marcus")
            .with_capture("ticket", "PROJ-1");
        assert_eq!(
            ctx.resolve("${user} -> ${ticket}").unwrap(),
            "marcus -> PROJ-1"
        );
    }

    #[test]
    fn test_resolve_no_placeholders_passes_through() {
        let ctx = Context::new();
        assert_eq!(ctx.resolve("plain text").unwrap(), "plain text");
    }

    #[test]
    fn test_namespace_parse_recognises_all_v0_1_names() {
        for n in &["builtin", "capture", "env", "config", "file", "ref", "ide"] {
            assert!(Namespace::parse(n).is_some(), "missing: {n}");
        }
    }

    #[test]
    fn test_namespace_parse_recognises_future_names() {
        assert!(matches!(Namespace::parse("git"), Some(Namespace::Git)));
        assert!(matches!(Namespace::parse("blame"), Some(Namespace::Blame)));
    }

    #[test]
    fn test_namespace_parse_rejects_unknown() {
        assert!(Namespace::parse("bogus").is_none());
    }

    #[test]
    fn test_namespace_v0_1_check() {
        assert!(Namespace::Capture.is_v0_1());
        assert!(Namespace::Env.is_v0_1());
        assert!(!Namespace::Git.is_v0_1());
        assert!(!Namespace::Blame.is_v0_1());
    }
}
