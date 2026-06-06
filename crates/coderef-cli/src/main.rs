//! coderef CLI.
//!
//! v0.0.0 → v0.1 transition. Foundation slice (`config show`) lets a user
//! validate a `.coderef.jsonc` end-to-end via the engine's loader. Real
//! subcommands (`check`, `changes`, `upgrade`, …) land per the v0.1
//! roadmap in `DESIGN.md` §20.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("{}", coderef_core::banner());
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("config") => match args.next().as_deref() {
            Some("show") => cmd_config_show(args.next()),
            Some(other) => {
                eprintln!("coderef config: unknown action `{other}`");
                eprintln!();
                print_help();
                ExitCode::from(2)
            }
            None => {
                eprintln!("coderef config: missing action (try `coderef config show <path>`)");
                ExitCode::from(2)
            }
        },
        Some(other) => {
            eprintln!("coderef: unknown subcommand `{other}`");
            eprintln!();
            print_help();
            ExitCode::from(2)
        }
    }
}

fn cmd_config_show(path: Option<String>) -> ExitCode {
    let Some(path) = path else {
        eprintln!("coderef config show: missing <path>");
        eprintln!("usage: coderef config show <path-to-.coderef.jsonc>");
        return ExitCode::from(2);
    };
    match coderef_core::config::Config::from_file(&path) {
        Ok(cfg) => match serde_json::to_string_pretty(&cfg) {
            Ok(s) => {
                println!("{s}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("coderef: failed to serialise parsed config: {e}");
                ExitCode::from(3)
            }
        },
        Err(e) => {
            eprintln!("coderef: {e}");
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!(
        "{banner}

Usage:  coderef <subcommand> [options]

Subcommands implemented in v0.0.x foundation:
  config show <path>   Parse and pretty-print a .coderef.jsonc

Subcommands planned per DESIGN.md §20 (not yet implemented):
  check       Verify references in source files
  changes     Coupled-change verifier (v0.2)
  upgrade     Rewrite legacy markers (v0.3)
  list        Dump all references without verifying
  explain     Show resolution for a single reference token
  doctor      Run integrity checks against the config
  cache       Manage the verification cache
  lsp         LSP server mode (v0.4)

Options:
  -h, --help      Show this help
  -V, --version   Show version banner

For the working specification, see DESIGN.md in the repository root.",
        banner = coderef_core::banner()
    );
}
