//! coderef-core
//!
//! Engine for [coderef](https://github.com/helly25/coderef). This crate is
//! deliberately host-call-free: no filesystem walking, no process spawning,
//! no HTTP. That discipline is what allows the same crate to compile to a
//! native CLI binary and to a WASM module used in-process by the `VSCode`
//! extension (see `DESIGN.md` §14.5.1 for the architectural commitment).
//!
//! v0.1 modules so far: config types + JSONC loading + variable
//! resolution + pattern compilation + comment-region detection +
//! per-file scanner + host-side workspace walker. Verifier and the
//! VSCode/WASM bindings land in subsequent PRs per `DESIGN.md` §20.

#![doc(html_root_url = "https://docs.rs/coderef-core/0.1.0")]

pub mod category;
#[cfg(not(target_arch = "wasm32"))]
pub mod anchor;
#[cfg(not(target_arch = "wasm32"))]
pub mod check;
pub mod comment;
#[cfg(not(target_arch = "wasm32"))]
pub mod commit_msg;
pub mod config;
pub mod doctor;
pub mod error;
pub mod explain;
pub mod pattern;
pub mod reference;
pub mod scan;
pub mod severity;
pub mod variables;
#[cfg(not(target_arch = "wasm32"))]
pub mod verify;

/// The crate version, exposed for the CLI's `--version` flag.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Returns a banner identifying the engine version. Used by the CLI and the
/// WASM module to verify they're linked against the same `coderef-core`.
#[must_use]
pub fn banner() -> String {
    format!("coderef-core {VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_contains_version() {
        assert!(banner().contains(VERSION));
    }
}
