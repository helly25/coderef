//! Top-level error types for `coderef-core`.
//!
//! Each subsystem (config, variables, pattern) defines its own `*Error`
//! and this module re-exports a `CoreError` that is the union the CLI and
//! WASM binding can match on.

use thiserror::Error;

use crate::config::ConfigError;
use crate::pattern::PatternError;
use crate::variables::VariableError;

/// Crate-level error type. Subsystem errors flatten into one of the
/// variants below; callers usually match on a specific variant rather than
/// stringify.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Failure parsing or validating a config file.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// Failure compiling or evaluating a pattern.
    #[error(transparent)]
    Pattern(#[from] PatternError),

    /// Failure resolving a `${...}` variable.
    #[error(transparent)]
    Variable(#[from] VariableError),
}

/// Result alias used by public APIs.
pub type Result<T, E = CoreError> = std::result::Result<T, E>;
