//! Scanner.
//!
//! Runs every compiled pattern against a source buffer, applies
//! `scope.commentsOnly` filtering, resolves each match's target +
//! title against the variable context, and returns a
//! deterministically-ordered `Vec<Reference>`.
//!
//! The per-file scanner is WASM-safe: it takes a buffer + a borrowed
//! pattern set with no filesystem or process access. The host-side
//! workspace walker that feeds files into the scanner is gated off
//! on `wasm32`.

mod file;

#[cfg(not(target_arch = "wasm32"))]
mod workspace;

pub use self::file::{scan_file, ScanError, ScanOptions};

#[cfg(not(target_arch = "wasm32"))]
pub use self::workspace::{scan_workspace, WorkspaceScanError};
