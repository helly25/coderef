//! Integration tests: parse the appendix-style example configs.
//!
//! Lives in `tests/` so it exercises the public crate API only (no
//! access to crate-private items).

use coderef_core::config::{Config, PatternKind};
use coderef_core::pattern::CompiledPattern;
use coderef_core::variables::Context;

const MINIMAL: &str = include_str!("fixtures/minimal.coderef.jsonc");
const TWO_PATTERNS: &str = include_str!("fixtures/two_patterns.coderef.jsonc");

#[test]
fn integration_parse_minimal_config() {
    let cfg = Config::from_jsonc_str(MINIMAL).expect("minimal fixture parses");
    assert_eq!(cfg.patterns.len(), 2);
    assert!(cfg.patterns.contains_key("todo-user"));
    assert!(cfg.patterns.contains_key("docref"));
}

#[test]
fn integration_compile_todo_user_pattern_and_resolve_url() {
    let cfg = Config::from_jsonc_str(MINIMAL).unwrap();
    let raw = cfg.patterns.get("todo-user").unwrap();
    let compiled = CompiledPattern::compile("todo-user", raw).unwrap();
    assert_eq!(compiled.kind, PatternKind::Url);

    // The pattern's regex matches a TODO with a named capture.
    let m = compiled.regex.find("# TODO(@marcus)").unwrap().unwrap();
    assert_eq!(m.as_str(), "TODO(@marcus)");

    // Capture extraction + target resolution. The target template uses
    // `${config:usersBase}`, so the context needs that variable seeded
    // from the parsed config.
    let caps = compiled.regex.captures("# TODO(@marcus)").unwrap().unwrap();
    let user = caps.name("user").unwrap().as_str();
    let users_base = cfg
        .variables
        .get("usersBase")
        .and_then(|v| v.as_str())
        .expect("usersBase variable present in fixture");
    let ctx = Context::new()
        .with_capture("user", user)
        .with_config("usersBase", users_base);
    let target = compiled.resolve_target(&ctx).unwrap();
    assert_eq!(target, "https://users.example.com/marcus");
}

#[test]
fn integration_compile_docref_pattern_is_local_kind() {
    let cfg = Config::from_jsonc_str(MINIMAL).unwrap();
    let raw = cfg.patterns.get("docref").unwrap();
    let compiled = CompiledPattern::compile("docref", raw).unwrap();
    assert_eq!(compiled.kind, PatternKind::Local);
}

#[test]
fn integration_two_patterns_preserve_declaration_order() {
    let cfg = Config::from_jsonc_str(TWO_PATTERNS).unwrap();
    let ids: Vec<&str> = cfg.patterns.keys().map(String::as_str).collect();
    assert_eq!(ids, vec!["zzz-last", "aaa-first"]);
}

#[test]
fn integration_variables_block_round_trips() {
    let cfg = Config::from_jsonc_str(MINIMAL).unwrap();
    assert!(cfg.variables.contains_key("usersBase"));
}

#[test]
fn integration_ignore_block_preserved() {
    let cfg = Config::from_jsonc_str(MINIMAL).unwrap();
    assert!(cfg.ignore.iter().any(|s| s == "**/node_modules/**"));
}
