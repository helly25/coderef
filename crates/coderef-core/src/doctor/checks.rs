//! Individual doctor check implementations.

use std::collections::HashSet;

use fancy_regex::Regex;

use super::Diagnostic;
use crate::config::{Config, Pattern};
use crate::severity::Severity;
use crate::variables::{parse_segments, Segment};

/// Run every per-pattern check, appending diagnostics in place.
#[allow(clippy::too_many_lines)] // every check is small; one fn keeps the order obvious
pub fn check_pattern(id: &str, p: &Pattern, cfg: &Config, out: &mut Vec<Diagnostic>) {
    // 1) target / targets validity.
    let has_target = p.target.is_some();
    let has_targets = !p.targets.is_empty();
    match (has_target, has_targets) {
        (false, false) => {
            out.push(Diagnostic {
                check: "pattern.targetMissing".into(),
                severity: Severity::Error,
                pattern_id: Some(id.into()),
                message: format!("pattern `{id}` has neither `target` nor `targets[]`"),
                hint: Some("set `target: \"...\"` (single) or `targets: [...]` (multi)".into()),
            });
        }
        (true, true) => {
            out.push(Diagnostic {
                check: "pattern.targetsBothFieldsSet".into(),
                severity: Severity::Error,
                pattern_id: Some(id.into()),
                message: format!("pattern `{id}` declares both `target` and `targets[]`"),
                hint: Some(
                    "pick one form: drop `target` to use multi-target, or empty `targets`".into(),
                ),
            });
        }
        _ => {}
    }

    // 2) regex compile.
    let compiled = match Regex::new(&p.regex) {
        Ok(r) => r,
        Err(e) => {
            out.push(Diagnostic {
                check: "pattern.regexInvalid".into(),
                severity: Severity::Error,
                pattern_id: Some(id.into()),
                message: format!("pattern `{id}` has an invalid regex: {e}"),
                hint: None,
            });
            return; // every subsequent check needs the compiled regex.
        }
    };

    let regex_caps: HashSet<String> = compiled
        .capture_names()
        .flatten()
        .map(String::from)
        .collect();

    // 3) Walk every template string (target + title + per-target urls) and
    // collect references to captures / variables.
    let mut referenced_captures: HashSet<String> = HashSet::new();
    let mut templates: Vec<(&'static str, &str)> = Vec::new();
    if let Some(t) = &p.target {
        templates.push(("target", t.as_str()));
    }
    if let Some(t) = &p.title {
        templates.push(("title", t.as_str()));
    }
    for (i, t) in p.targets.iter().enumerate() {
        // Borrow trick: rather than format!() the field label every
        // iteration, fall back to a static label noting it's a targets
        // entry. The pattern id + check id let the user find the row.
        let _ = i;
        templates.push(("targets[].url", t.url.as_str()));
    }

    for (field, template) in templates {
        let segments = match parse_segments(template) {
            Ok(s) => s,
            Err(e) => {
                out.push(Diagnostic {
                    check: "variable.invalidSyntax".into(),
                    severity: Severity::Error,
                    pattern_id: Some(id.into()),
                    message: format!(
                        "pattern `{id}` field `{field}` has invalid `${{...}}` syntax: {e}"
                    ),
                    hint: None,
                });
                continue;
            }
        };

        for seg in segments {
            let Segment::Variable { namespace, name } = seg else {
                continue;
            };
            match namespace.as_deref() {
                None => {
                    // Bare name — could be a builtin or a capture.
                    if regex_caps.contains(&name) {
                        referenced_captures.insert(name);
                    }
                    // Else: assume builtin / left for runtime. v0.2
                    // doctor will validate against a known-builtins
                    // table.
                }
                Some("capture") => {
                    if regex_caps.contains(&name) {
                        referenced_captures.insert(name);
                    } else {
                        out.push(Diagnostic {
                            check: "pattern.captureUnknown".into(),
                            severity: Severity::Error,
                            pattern_id: Some(id.into()),
                            message: format!(
                                "pattern `{id}` field `{field}` references `${{capture:{name}}}` \
                                 but the regex has no such named capture"
                            ),
                            hint: Some(format!(
                                "either add `(?<{name}>...)` to the regex, or fix the template to \
                                 reference an existing capture: {}",
                                regex_caps.iter().cloned().collect::<Vec<_>>().join(", ")
                            )),
                        });
                    }
                }
                Some("config") => {
                    if !cfg.variables.contains_key(&name) {
                        out.push(Diagnostic {
                            check: "pattern.variableConfigUnknown".into(),
                            severity: Severity::Error,
                            pattern_id: Some(id.into()),
                            message: format!(
                                "pattern `{id}` field `{field}` references `${{config:{name}}}` \
                                 but the config has no such variable"
                            ),
                            hint: Some(format!(
                                "add `variables.{name}` to the config, or fix the template"
                            )),
                        });
                    }
                }
                Some("git" | "blame") => {
                    let ns = namespace.as_deref().unwrap_or("");
                    let expected_version = if ns == "blame" { "v0.3" } else { "v0.2" };
                    out.push(Diagnostic {
                        check: "pattern.variableNamespaceFuture".into(),
                        severity: Severity::Warning,
                        pattern_id: Some(id.into()),
                        message: format!(
                            "pattern `{id}` field `{field}` references `${{{ns}:{name}}}` — \
                             namespace `{ns}:` is not implemented in v0.1 (expected in \
                             {expected_version})"
                        ),
                        hint: Some(format!(
                            "this pattern will fail at scan time until {expected_version}"
                        )),
                    });
                }
                Some(_) => {
                    // env, file, ref, ide — accepted at static time;
                    // runtime checks them.
                }
            }
        }
    }

    // 4) captureUnused: any named capture that wasn't referenced.
    for cap in &regex_caps {
        if !referenced_captures.contains(cap) {
            out.push(Diagnostic {
                check: "pattern.captureUnused".into(),
                severity: Severity::Warning,
                pattern_id: Some(id.into()),
                message: format!(
                    "pattern `{id}` captures `{cap}` but no template (target/title/targets[].url) \
                     references it"
                ),
                hint: Some(format!(
                    "remove the capture group or reference it as `${{{cap}}}`"
                )),
            });
        }
    }
}
