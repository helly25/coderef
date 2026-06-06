//! Generate `schema/coderef.schema.json` from `coderef-core`'s config types.
//!
//! Run: `cargo run --bin coderef-gen-schema` from the workspace root.
//! Writes the schema to `<workspace>/schema/coderef.schema.json`. CI
//! re-runs this binary and `git diff --exit-code`s the result to enforce
//! that the committed schema matches the Rust types (DESIGN.md §7.3,
//! §14.5.1).

use coderef_core::config::Config;
use schemars::schema_for;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schema_for!(Config);
    let json = serde_json::to_string_pretty(&schema)?;
    let path = "schema/coderef.schema.json";
    std::fs::write(path, format!("{json}\n"))?;
    println!("wrote {path}");
    Ok(())
}
