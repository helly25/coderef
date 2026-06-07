//! WASM bindings for `coderef-core`.
//!
//! Exposes the WASM-safe surface of the engine to JS hosts: config
//! loading, in-buffer scanning, and static doctor checks. Host-side
//! work (workspace walking, HTTP verification, filesystem I/O) stays
//! on the host — the editor's extension is responsible for feeding
//! buffers in and dispatching the verifier (e.g. via the editor's
//! fetch API). See `DESIGN.md` §14.5.1 for the WASM module boundary.
//!
//! Function signatures take + return JSON strings (or `JsValue`) so
//! the JS side never has to construct Rust-shaped data structures.
//! Errors come back as `JsValue::String` so they show up cleanly in
//! browser/Node consoles and devtools.

#![allow(clippy::needless_pass_by_value)] // JsValue arguments are owned for convenience.

use coderef_core::comment::language_for_extension;
use coderef_core::config::Config;
use coderef_core::doctor::{DoctorReport, run_doctor};
use coderef_core::pattern::CompiledPattern;
use coderef_core::reference::Reference;
use coderef_core::scan::{ScanOptions, scan_file};
use coderef_core::variables::Context;
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Serialize Rust → `JsValue` with maps rendered as plain JS objects
/// (not `Map`), so the JS caller can `JSON.stringify` the result and
/// parse it back without losing keys. `IndexMap`'s default
/// `serde-wasm-bindgen` mapping is JS `Map`, which serialises as `{}`
/// under `JSON.stringify` — surprising, and the reason the v0.1 smoke
/// caught this on first push.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Engine version (matches `coderef-core::VERSION`).
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    coderef_core::VERSION.to_string()
}

/// Banner string used by the host to verify they're linked against the
/// expected engine.
#[wasm_bindgen]
#[must_use]
pub fn banner() -> String {
    coderef_core::banner()
}

/// Parse a JSONC config string and return the engine's `Config` as a
/// JS object. Useful for hosts that want to validate a `.coderef.jsonc`
/// without re-implementing the JSONC parser.
#[wasm_bindgen]
pub fn parse_config(jsonc: &str) -> Result<JsValue, JsValue> {
    let cfg = Config::from_jsonc_str(jsonc).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&cfg)
}

/// Scan a single file buffer. The config and language are passed by
/// the host to keep this function pure (no I/O).
///
/// Arguments:
/// - `content`       — the file's text.
/// - `config_json`   — a `Config` serialized as JSON (e.g. what
///                     `parse_config` returns, then `JSON.stringify`d).
/// - `language_ext`  — optional file extension (without dot, e.g.
///                     `"rs"`). When unknown, `commentsOnly` patterns
///                     match nothing per `DESIGN.md` §5.4.1.
/// - `file`          — the file label embedded in each returned
///                     `Reference`. The host chooses whether this is
///                     workspace-relative, absolute, or untitled.
///
/// Returns a JS array of `Reference` objects.
#[wasm_bindgen]
pub fn scan_buffer(
    content: &str,
    config_json: &str,
    language_ext: Option<String>,
    file: &str,
) -> Result<JsValue, JsValue> {
    let cfg: Config =
        serde_json::from_str(config_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Compile every pattern; surface the first failure as a JS error.
    let mut compiled = Vec::with_capacity(cfg.patterns.len());
    for (id, raw) in &cfg.patterns {
        let c = CompiledPattern::compile(id.clone(), raw)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        compiled.push((c, raw.clone()));
    }

    let lang = language_ext.as_deref().and_then(language_for_extension);

    let mut ctx = Context::new();
    for (k, v) in &cfg.variables {
        if let Some(s) = v.as_str() {
            ctx = ctx.with_config(k.clone(), s.to_string());
        }
    }

    let opts = ScanOptions {
        patterns: &compiled,
        language: lang,
        base_context: &ctx,
        file,
    };

    let refs: Vec<Reference> =
        scan_file(content, &opts).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&refs)
}

/// Run the static doctor checks against a config. The workspace-
/// dependent `pattern.unused` is unavailable from WASM (no walker);
/// hosts that want it should call the CLI for that check.
#[wasm_bindgen]
pub fn doctor_static(config_json: &str) -> Result<JsValue, JsValue> {
    let cfg: Config =
        serde_json::from_str(config_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let report: DoctorReport = run_doctor(&cfg);
    to_js(&report)
}

// ---------------------------------------------------------------------
// Host-side tests of the binding *logic* (no WASM runtime required).
//
// The actual wasm32 build is exercised by `wasm-pack build --target
// nodejs` in CI followed by a small Node smoke script (see
// .github/workflows/ci.yml). Those two together cover:
//
//   1. The crate compiles to wasm32-unknown-unknown.
//   2. wasm-pack emits a valid npm package.
//   3. Node can import the package and call the exported functions
//      end-to-end.
//
// The tests here check the *Rust* side of the boundary — that the
// conversions between Rust types and serde_wasm_bindgen / serde_json
// stay coherent. They run on the host triple (no WASM toolchain
// required), so `cargo test` continues to work everywhere.
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Host-side unit tests. The internal helpers (config compile,
    //! pattern compile, scan invocation) all live in `coderef-core`
    //! and have their own tests there; what we cover here is the
    //! glue: that our argument shapes round-trip through JSON and
    //! that error paths surface a useful message.

    use coderef_core::config::Config;

    #[test]
    fn engine_version_string_is_nonempty() {
        assert!(!coderef_core::VERSION.is_empty());
    }

    #[test]
    fn config_round_trips_through_json() {
        // Mirrors the parse_config → JSON.stringify → scan_buffer flow.
        let jsonc = r#"
        {
            "patterns": {
                "todo": {
                    "regex":  "TODO\\(@(?<user>\\w+)\\)",
                    "target": "https://x/${user}"
                }
            }
        }
        "#;
        let cfg = Config::from_jsonc_str(jsonc).unwrap();
        let json = serde_json::to_string(&cfg).unwrap();
        let round_tripped: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.patterns.len(), 1);
        assert!(round_tripped.patterns.contains_key("todo"));
    }

    #[test]
    fn invalid_jsonc_surfaces_a_message() {
        let err = Config::from_jsonc_str("{ \"patterns\": [").unwrap_err();
        let msg = err.to_string();
        assert!(!msg.is_empty());
        // The first ~80 chars should describe the parse failure.
        assert!(
            msg.to_ascii_lowercase().contains("jsonc")
                || msg.to_ascii_lowercase().contains("parse"),
            "got: {msg}"
        );
    }
}
