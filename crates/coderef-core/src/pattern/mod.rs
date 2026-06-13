//! Pattern compilation and reference resolution.
//!
//! Takes a `Pattern` (the config struct) and produces a `CompiledPattern`
//! (the runtime form with a compiled regex). Resolution uses the
//! `variables` module to interpolate the target template against a match's
//! captures.

use fancy_regex::Regex;
use thiserror::Error;

use crate::config::{Pattern, PatternKind};
use crate::variables::{Context, VariableError};

/// Pattern in its runtime form: the regex is compiled and the target
/// template is parsed and ready for substitution.
#[derive(Clone, Debug)]
pub struct CompiledPattern {
    /// The pattern's config id (e.g. `"todo-user"`).
    pub id: String,
    /// Compiled regex.
    pub regex: Regex,
    /// Resolved kind (url, local, ...).
    pub kind: PatternKind,
    /// Target template (raw `${...}`-containing string).
    pub target_template: String,
    /// Title template (raw).
    pub title_template: Option<String>,
    /// Priority for tie-breaking overlapping matches.
    pub priority: i32,
    /// Optional category id.
    pub category: Option<String>,
}

impl CompiledPattern {
    /// Compile a `Pattern` config into its runtime form.
    pub fn compile(id: impl Into<String>, raw: &Pattern) -> Result<Self, PatternError> {
        let id = id.into();
        let kind = raw.kind;

        // v0.1 supports url + local. v0.2 adds block + ifchange;
        // command is still a placeholder.
        match kind {
            PatternKind::Url | PatternKind::Local | PatternKind::Block | PatternKind::IfChange => {}
            PatternKind::Command => {
                return Err(PatternError::KindNotYetImplemented {
                    id,
                    kind: "command".into(),
                    expected_version: "post-v0.4".into(),
                });
            }
        }

        // Target requirements differ by kind:
        // - url / local: single-target required (v0.1 contract; multi-target is v0.3).
        // - block / ifchange: target template is irrelevant — the
        //   match itself (block) or peer/target lookup at diff-time
        //   (ifchange) is what verifies. Accept `target: null` and
        //   ignore `target` if set. `targets[]` is rejected for
        //   these kinds to keep the schema unambiguous.
        let target_template = match kind {
            PatternKind::Block | PatternKind::IfChange => {
                if !raw.targets.is_empty() {
                    return Err(PatternError::BlockKindCannotHaveTargets { id });
                }
                String::new()
            }
            _ => match (&raw.target, raw.targets.as_slice()) {
                (Some(t), []) => t.clone(),
                (None, []) => return Err(PatternError::NoTarget { id }),
                (None, _targets) => return Err(PatternError::MultiTargetNotYetImplemented { id }),
                (Some(_), _) => return Err(PatternError::TargetAndTargetsBothSet { id }),
            },
        };

        let regex = Regex::new(&raw.regex).map_err(|e| PatternError::InvalidRegex {
            id: id.clone(),
            message: e.to_string(),
        })?;

        Ok(Self {
            id,
            regex,
            kind,
            target_template,
            title_template: raw.title.clone(),
            priority: raw.priority,
            category: raw.category.clone(),
        })
    }

    /// Resolve this pattern's target against a captured-values context.
    /// The context should already have `${capture:<name>}` populated for
    /// every named capture in `self.regex` that the call site cares about.
    pub fn resolve_target(&self, ctx: &Context) -> Result<String, VariableError> {
        ctx.resolve(&self.target_template)
    }

    /// Resolve the title template if present.
    pub fn resolve_title(&self, ctx: &Context) -> Result<Option<String>, VariableError> {
        match &self.title_template {
            Some(t) => Ok(Some(ctx.resolve(t)?)),
            None => Ok(None),
        }
    }
}

/// Failures from pattern compilation.
#[derive(Debug, Error)]
pub enum PatternError {
    /// Regex did not compile under `fancy-regex`.
    #[error("pattern `{id}` has an invalid regex: {message}")]
    InvalidRegex { id: String, message: String },

    /// Neither `target` nor `targets` was set.
    #[error("pattern `{id}` is missing a target")]
    NoTarget { id: String },

    /// Both `target` and `targets[]` were set — doctor would flag this
    /// as `targets.bothFieldsSet` (DESIGN §5.3.1, §9.1).
    #[error("pattern `{id}` declares both `target` and `targets[]`; pick one")]
    TargetAndTargetsBothSet { id: String },

    /// Multi-target patterns are v0.3+.
    #[error("pattern `{id}` uses `targets[]`, which is a v0.3 feature; v0.1 supports single-target only")]
    MultiTargetNotYetImplemented { id: String },

    /// Pattern kind isn't supported by this engine version.
    #[error("pattern `{id}` uses `kind: \"{kind}\"`, which is not yet implemented (expected in {expected_version})")]
    KindNotYetImplemented {
        id: String,
        kind: String,
        expected_version: String,
    },

    /// `kind: "block"` patterns don't resolve to a target; `targets[]`
    /// would be silently ignored, so we reject it loudly instead.
    #[error("pattern `{id}` has `kind: \"block\"` and may not declare `targets[]` (the matched text is the diagnostic; no target to resolve)")]
    BlockKindCannotHaveTargets { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p_with(regex: &str) -> Pattern {
        Pattern {
            regex: regex.to_string(),
            target: Some("https://x/${user}".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_compile_simple_url_pattern_succeeds() {
        let p = p_with(r"TODO\(@(?<user>[\w.-]+)\)");
        let c = CompiledPattern::compile("todo-user", &p).unwrap();
        assert_eq!(c.id, "todo-user");
        assert_eq!(c.kind, PatternKind::Url);
        assert_eq!(c.target_template, "https://x/${user}");
    }

    #[test]
    fn test_compile_invalid_regex_returns_error() {
        let p = p_with(r"TODO\(@(?<user>[\w.-]+\)"); // unbalanced
        let err = CompiledPattern::compile("bad", &p).unwrap_err();
        assert!(matches!(err, PatternError::InvalidRegex { .. }));
    }

    #[test]
    fn test_compile_missing_target_returns_error() {
        let p = Pattern {
            regex: "X".into(),
            ..Default::default()
        };
        let err = CompiledPattern::compile("no-target", &p).unwrap_err();
        assert!(matches!(err, PatternError::NoTarget { .. }));
    }

    #[test]
    fn test_compile_target_and_targets_both_set_returns_error() {
        use crate::config::TargetSpec;
        let p = Pattern {
            regex: "X".into(),
            target: Some("a".into()),
            targets: vec![TargetSpec {
                url: "b".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let err = CompiledPattern::compile("both", &p).unwrap_err();
        assert!(matches!(err, PatternError::TargetAndTargetsBothSet { .. }));
    }

    #[test]
    fn test_compile_multi_target_returns_not_yet_implemented() {
        use crate::config::TargetSpec;
        let p = Pattern {
            regex: "X".into(),
            targets: vec![TargetSpec {
                url: "a".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let err = CompiledPattern::compile("multi", &p).unwrap_err();
        assert!(matches!(
            err,
            PatternError::MultiTargetNotYetImplemented { .. }
        ));
    }

    #[test]
    fn test_compile_ifchange_kind_succeeds_without_target_and_ignores_stray_target() {
        // ifchange compiles like block — no target needed; a stray
        // `target` field is accepted but does nothing at scan time.
        let p = Pattern {
            regex: "X".into(),
            target: Some("ignored".into()),
            kind: PatternKind::IfChange,
            ..Default::default()
        };
        let c = CompiledPattern::compile("ifc", &p).unwrap();
        assert_eq!(c.kind, PatternKind::IfChange);
        assert_eq!(c.target_template, "");
    }

    #[test]
    fn test_compile_command_kind_returns_not_yet_implemented() {
        let p = Pattern {
            regex: "X".into(),
            target: Some("a".into()),
            kind: PatternKind::Command,
            ..Default::default()
        };
        let err = CompiledPattern::compile("cmd", &p).unwrap_err();
        assert!(matches!(
            err,
            PatternError::KindNotYetImplemented { ref kind, .. } if kind == "command"
        ));
    }

    #[test]
    fn test_compile_block_kind_without_target_succeeds() {
        let p = Pattern {
            regex: r"\bDONOTMERGE\b".into(),
            kind: PatternKind::Block,
            ..Default::default()
        };
        let c = CompiledPattern::compile("block-default", &p).unwrap();
        assert_eq!(c.kind, PatternKind::Block);
        assert_eq!(c.target_template, "");
    }

    #[test]
    fn test_compile_block_kind_ignores_target_field() {
        // Block patterns may carry a stray `target` (e.g. leftover from
        // copy-pasted config). We accept it and ignore it; the match
        // itself is the diagnostic.
        let p = Pattern {
            regex: "X".into(),
            target: Some("https://example.org/ignored".into()),
            kind: PatternKind::Block,
            ..Default::default()
        };
        let c = CompiledPattern::compile("blk", &p).unwrap();
        assert_eq!(c.kind, PatternKind::Block);
        assert_eq!(c.target_template, "");
    }

    #[test]
    fn test_compile_block_kind_rejects_targets_array() {
        use crate::config::TargetSpec;
        let p = Pattern {
            regex: "X".into(),
            kind: PatternKind::Block,
            targets: vec![TargetSpec {
                url: "https://x".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let err = CompiledPattern::compile("blk-multi", &p).unwrap_err();
        assert!(matches!(
            err,
            PatternError::BlockKindCannotHaveTargets { .. }
        ));
    }

    #[test]
    fn test_resolve_target_interpolates_capture() {
        let p = p_with(r"TODO\(@(?<user>[\w.-]+)\)");
        let c = CompiledPattern::compile("todo-user", &p).unwrap();
        let ctx = Context::new().with_capture("user", "marcus");
        assert_eq!(c.resolve_target(&ctx).unwrap(), "https://x/marcus");
    }

    #[test]
    fn test_resolve_title_returns_some_when_template_set() {
        let mut p = p_with("X");
        p.title = Some("User: ${user}".into());
        let c = CompiledPattern::compile("t", &p).unwrap();
        let ctx = Context::new().with_capture("user", "sara");
        assert_eq!(
            c.resolve_title(&ctx).unwrap(),
            Some("User: sara".to_string())
        );
    }

    #[test]
    fn test_resolve_title_returns_none_when_template_unset() {
        let p = p_with("X");
        let c = CompiledPattern::compile("t", &p).unwrap();
        let ctx = Context::new();
        assert!(c.resolve_title(&ctx).unwrap().is_none());
    }

    #[test]
    fn test_fancy_regex_lookaround_compiles() {
        // Sanity-check that fancy-regex is wired in: a regex using
        // negative-lookahead should compile.
        let p = p_with(r"TODO\((?!b/)(?<user>[\w.-]+)\)");
        let c = CompiledPattern::compile("user-not-bug", &p).unwrap();
        // And it should match the right thing.
        assert!(c.regex.is_match("TODO(marcus)").unwrap());
        assert!(!c.regex.is_match("TODO(b/123)").unwrap());
    }
}
