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
        Some("check") => cmd_check(args.collect()),
        Some("doctor") => cmd_doctor(args.collect()),
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

/// `coderef check [--config <path>] [--report text|json] [--timeout-ms N] <root>`
///
/// Scans the workspace at `<root>` and verifies every discovered
/// reference. Exits 0 if all references resolved (or were skipped),
/// 1 if any reference broke, 2 on usage / config / scan errors.
#[allow(clippy::too_many_lines)] // arg parsing + dispatch is naturally long
fn cmd_check(args: Vec<String>) -> ExitCode {
    let mut config_path: Option<String> = None;
    let mut report: Report = Report::Text;
    let mut timeout_ms: u64 = 10_000;
    let mut root: Option<String> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(path) = it.next() else {
                    eprintln!("coderef check: --config requires a value");
                    return ExitCode::from(2);
                };
                config_path = Some(path);
            }
            "--report" => {
                let Some(kind) = it.next() else {
                    eprintln!("coderef check: --report requires `text` or `json`");
                    return ExitCode::from(2);
                };
                report = match kind.as_str() {
                    "text" => Report::Text,
                    "json" => Report::Json,
                    other => {
                        eprintln!(
                            "coderef check: --report must be `text` or `json` (got `{other}`)"
                        );
                        return ExitCode::from(2);
                    }
                };
            }
            "--timeout-ms" => {
                let Some(n) = it.next() else {
                    eprintln!("coderef check: --timeout-ms requires a value");
                    return ExitCode::from(2);
                };
                let Ok(v) = n.parse() else {
                    eprintln!("coderef check: --timeout-ms must be a positive integer");
                    return ExitCode::from(2);
                };
                timeout_ms = v;
            }
            "--help" | "-h" => {
                println!("Usage: coderef check [--config <path>] [--report text|json] [--timeout-ms N] <root>");
                println!();
                println!(
                    "  --config <path>     Path to .coderef.jsonc (default: <root>/.coderef.jsonc)"
                );
                println!("  --report text|json  Output format (default: text)");
                println!("  --timeout-ms N      Per-request timeout in ms (default: 10000)");
                println!();
                println!("Exit codes: 0 = all references resolved (or skipped); 1 = at least");
                println!("one reference broke; 2 = usage / config / scan error.");
                return ExitCode::SUCCESS;
            }
            _ if root.is_none() => root = Some(arg),
            _ => {
                eprintln!("coderef check: unexpected argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(root) = root else {
        eprintln!("coderef check: missing <root>");
        eprintln!(
            "usage: coderef check [--config <path>] [--report text|json] [--timeout-ms N] <root>"
        );
        return ExitCode::from(2);
    };

    let cfg_path =
        config_path.unwrap_or_else(|| format!("{}/.coderef.jsonc", root.trim_end_matches('/')));
    let cfg = match coderef_core::config::Config::from_file(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("coderef check: failed to load config `{cfg_path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let opts = coderef_core::verify::VerifyOptions {
        timeout: std::time::Duration::from_millis(timeout_ms),
        workspace_root: std::path::PathBuf::from(&root),
        ..Default::default()
    };

    let result = coderef_core::check::check_workspace(&root, &cfg, &opts);
    let report_value = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("coderef check: {e}");
            return ExitCode::from(2);
        }
    };

    match report {
        Report::Text => print_text_report(&report_value),
        Report::Json => match serde_json::to_string_pretty(&report_value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("coderef check: JSON encoding failed: {e}");
                return ExitCode::from(3);
            }
        },
    }

    if report_value.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

enum Report {
    Text,
    Json,
}

fn print_text_report(report: &coderef_core::check::CheckReport) {
    use coderef_core::verify::VerifyOutcome;
    for r in &report.results {
        let prefix = match &r.outcome {
            VerifyOutcome::Ok => "ok    ",
            VerifyOutcome::Skipped { .. } => "skip  ",
            _ => "BROKEN",
        };
        let detail = match &r.outcome {
            VerifyOutcome::Ok => String::new(),
            VerifyOutcome::BrokenStatus { status } => format!("  (status {status})"),
            VerifyOutcome::BrokenNetwork { reason } => format!("  ({reason})"),
            VerifyOutcome::NotFound { path } => format!("  (no such file: {path})"),
            VerifyOutcome::Skipped { reason } => format!("  ({reason})"),
        };
        println!(
            "{prefix}  {file}:{line}:{col}  [{id}]  →  {target}{detail}",
            file = r.reference.file,
            line = r.reference.line,
            col = r.reference.column,
            id = r.reference.pattern_id,
            target = r.reference.target,
        );
    }
    println!();
    println!(
        "Checked {total} reference(s): {ok} ok, {broken} broken, {skipped} skipped",
        total = report.total,
        ok = report.ok,
        broken = report.broken,
        skipped = report.skipped,
    );
}

/// `coderef doctor [--config <path>] [--report text|json] [--no-scan] [<root>]`
///
/// Runs static + (optionally) scan-dependent integrity checks against
/// the config. Exits 0 if no error-severity diagnostics, 1 if any.
fn cmd_doctor(args: Vec<String>) -> ExitCode {
    let mut config_path: Option<String> = None;
    let mut report: Report = Report::Text;
    let mut scan = true;
    let mut root: Option<String> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(path) = it.next() else {
                    eprintln!("coderef doctor: --config requires a value");
                    return ExitCode::from(2);
                };
                config_path = Some(path);
            }
            "--report" => {
                let Some(kind) = it.next() else {
                    eprintln!("coderef doctor: --report requires `text` or `json`");
                    return ExitCode::from(2);
                };
                report = match kind.as_str() {
                    "text" => Report::Text,
                    "json" => Report::Json,
                    other => {
                        eprintln!(
                            "coderef doctor: --report must be `text` or `json` (got `{other}`)"
                        );
                        return ExitCode::from(2);
                    }
                };
            }
            "--no-scan" => scan = false,
            "--help" | "-h" => {
                println!("Usage: coderef doctor [--config <path>] [--report text|json] [--no-scan] [<root>]");
                println!();
                println!("  --config <path>     Path to .coderef.jsonc (default: <root>/.coderef.jsonc, or ./.coderef.jsonc)");
                println!("  --report text|json  Output format (default: text)");
                println!("  --no-scan           Static checks only (skip pattern.unused which needs a scan)");
                println!();
                println!("Exit codes: 0 = no errors; 1 = at least one error-severity diagnostic;");
                println!("2 = usage / config error.");
                return ExitCode::SUCCESS;
            }
            _ if root.is_none() => root = Some(arg),
            _ => {
                eprintln!("coderef doctor: unexpected argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }

    let cfg_path = config_path.unwrap_or_else(|| match &root {
        Some(r) => format!("{}/.coderef.jsonc", r.trim_end_matches('/')),
        None => "./.coderef.jsonc".into(),
    });
    let cfg = match coderef_core::config::Config::from_file(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("coderef doctor: failed to load config `{cfg_path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let report_value = if scan {
        let r = root.as_deref().unwrap_or(".");
        match coderef_core::doctor::run_doctor_with_workspace(r, &cfg) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("coderef doctor: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        coderef_core::doctor::run_doctor(&cfg)
    };

    match report {
        Report::Text => print_doctor_text(&report_value),
        Report::Json => match serde_json::to_string_pretty(&report_value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("coderef doctor: JSON encoding failed: {e}");
                return ExitCode::from(3);
            }
        },
    }

    if report_value.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_doctor_text(report: &coderef_core::doctor::DoctorReport) {
    use coderef_core::severity::Severity;
    for d in &report.diagnostics {
        let sev = match d.severity {
            Severity::Error => "ERROR  ",
            Severity::Warning => "warning",
            Severity::Info => "info   ",
            Severity::Hint => "hint   ",
            Severity::Off => "off    ",
        };
        let pid = d
            .pattern_id
            .as_deref()
            .map(|p| format!("[{p}] "))
            .unwrap_or_default();
        println!(
            "{sev}  {check}  {pid}{message}",
            check = d.check,
            message = d.message
        );
        if let Some(h) = &d.hint {
            println!("           hint: {h}");
        }
    }
    if !report.diagnostics.is_empty() {
        println!();
    }
    println!(
        "{total} diagnostic(s): {errors} error, {warnings} warning, {infos} info, {hints} hint",
        total = report.total,
        errors = report.errors,
        warnings = report.warnings,
        infos = report.infos,
        hints = report.hints,
    );
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
  check [opts] <root>          Scan + verify every reference; exit 1 on failure
                               --config <path>  Override config location
                               --report json    Emit JSON report
                               --timeout-ms N   Per-request timeout (default 10000)
  doctor [opts] [<root>]       Static + scan-dependent integrity checks
                               --config <path>  Override config location
                               --report json    Emit JSON report
                               --no-scan        Skip the workspace scan (static only)

Subcommands planned per DESIGN.md §20 (not yet implemented):
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
