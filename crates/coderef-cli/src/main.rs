//! coderef CLI.
//!
//! v0.0.0 → v0.1 transition. Foundation slice (`config show`, `list`)
//! lets a user validate a `.coderef.jsonc` and dump all references
//! discovered in a workspace. Real verification subcommands (`check`,
//! `changes`, `upgrade`, …) land per the v0.1 roadmap in
//! `DESIGN.md` §20.

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
        Some("list") => cmd_list(args.collect()),
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

/// `coderef list [--config <path>] [--json] <root>`
///
/// Walks the workspace at `<root>`, scans every non-ignored file, and
/// prints every discovered reference. Default format:
///
/// ```text
/// <file>:<line>:<col>  [<pattern_id>]  <matched_text>  →  <target>
/// ```
fn cmd_list(args: Vec<String>) -> ExitCode {
    let mut config_path: Option<String> = None;
    let mut as_json = false;
    let mut root: Option<String> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(path) = it.next() else {
                    eprintln!("coderef list: --config requires a value");
                    return ExitCode::from(2);
                };
                config_path = Some(path);
            }
            "--json" => as_json = true,
            "--help" | "-h" => {
                println!("Usage: coderef list [--config <path>] [--json] <root>");
                println!();
                println!("  --config <path>   Path to the .coderef.jsonc (default: <root>/.coderef.jsonc)");
                println!("  --json            Emit JSON instead of one-line-per-reference text");
                return ExitCode::SUCCESS;
            }
            _ if root.is_none() => root = Some(arg),
            _ => {
                eprintln!("coderef list: unexpected argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(root) = root else {
        eprintln!("coderef list: missing <root>");
        eprintln!("usage: coderef list [--config <path>] [--json] <root>");
        return ExitCode::from(2);
    };

    let cfg_path =
        config_path.unwrap_or_else(|| format!("{}/.coderef.jsonc", root.trim_end_matches('/')));
    let cfg = match coderef_core::config::Config::from_file(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("coderef list: failed to load config `{cfg_path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let refs = match coderef_core::scan::scan_workspace(&root, &cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("coderef list: scan failed: {e}");
            return ExitCode::from(2);
        }
    };

    if as_json {
        match serde_json::to_string_pretty(&refs) {
            Ok(s) => {
                println!("{s}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("coderef list: JSON encoding failed: {e}");
                ExitCode::from(3)
            }
        }
    } else {
        for r in &refs {
            println!(
                "{file}:{line}:{col}  [{id}]  {text}  →  {target}",
                file = r.file,
                line = r.line,
                col = r.column,
                id = r.pattern_id,
                text = r.matched_text,
                target = r.target,
            );
        }
        ExitCode::SUCCESS
    }
}

fn print_help() {
    println!(
        "{banner}

Usage:  coderef <subcommand> [options]

Subcommands implemented in v0.0.x foundation:
  config show <path>           Parse and pretty-print a .coderef.jsonc
  list [opts] <root>           Walk <root> and dump every reference found
                               --config <path>  Override config location
                               --json           Emit JSON

Subcommands planned per DESIGN.md §20 (not yet implemented):
  check       Verify references in source files
  changes     Coupled-change verifier (v0.2)
  upgrade     Rewrite legacy markers (v0.3)
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
