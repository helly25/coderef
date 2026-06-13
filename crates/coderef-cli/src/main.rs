//! coderef CLI.
//!
//! Subcommand surface as of v0.2:
//!   config show <path>           Parse + pretty-print a .coderef.jsonc
//!   list [opts] <root>           Scan + emit every Reference found
//!   check [opts] <root>          Scan + verify; exits 1 on broken refs
//!   doctor [opts] [<root>]       Static + scan-dependent integrity checks
//!   patterns [opts] [<id>]       Inspect configured patterns
//!
//! Every subcommand accepts `--help` (or `-h`) for detailed sectioned
//! help (USAGE / DESCRIPTION / ARGUMENTS / OPTIONS / EXIT CODES /
//! EXAMPLES). The top-level `coderef --help` lists subcommands; drill
//! down for specifics.

use std::process::ExitCode;

mod help;

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
        Some("patterns") => cmd_patterns(args.collect()),
        Some("explain") => cmd_explain(args.collect()),
        Some("changes") => cmd_changes(args.collect()),
        Some("help") => cmd_help(args.collect()),
        Some(other) => {
            eprintln!("coderef: unknown subcommand `{other}`");
            eprintln!();
            print_help();
            ExitCode::from(2)
        }
    }
}

fn cmd_config_show(path: Option<String>) -> ExitCode {
    if let Some("--help" | "-h") = path.as_deref() {
        print!("{}", help::CONFIG_SHOW_HELP);
        return ExitCode::SUCCESS;
    }
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
                print!("{}", help::LIST_HELP);
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
                print!("{}", help::CHECK_HELP);
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
            VerifyOutcome::BlockMarker { .. } => "BLOCK ",
            _ => "BROKEN",
        };
        let detail = match &r.outcome {
            VerifyOutcome::Ok => String::new(),
            VerifyOutcome::BrokenStatus { status } => format!("  (status {status})"),
            VerifyOutcome::BrokenNetwork { reason } => format!("  ({reason})"),
            VerifyOutcome::NotFound { path } => format!("  (no such file: {path})"),
            VerifyOutcome::BlockMarker { matched_text } => {
                format!("  (block marker present: `{matched_text}`)")
            }
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
                print!("{}", help::DOCTOR_HELP);
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
    for (i, d) in report.diagnostics.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let sev = match d.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "warn",
            Severity::Info => "info",
            Severity::Hint => "hint",
            Severity::Off => "off",
        };
        let pid = d
            .pattern_id
            .as_deref()
            .map(|p| format!(" [{p}]"))
            .unwrap_or_default();
        // Header line: severity + check id + pattern id.
        println!("{sev}  {check}{pid}", check = d.check);
        // Body: message indented two spaces; embedded newlines kept.
        println!("{}", indent_block(&d.message, "  "));
        if let Some(h) = &d.hint {
            println!();
            println!(
                "  hint: {first_line}",
                first_line = h.lines().next().unwrap_or("")
            );
            for line in h.lines().skip(1) {
                println!("        {line}");
            }
        }
    }
    if !report.diagnostics.is_empty() {
        println!();
        println!("────────");
    }
    let mut parts: Vec<String> = Vec::new();
    if report.errors > 0 {
        parts.push(format!("{n} error", n = report.errors));
    }
    if report.warnings > 0 {
        parts.push(format!("{n} warning", n = report.warnings));
    }
    if report.infos > 0 {
        parts.push(format!("{n} info", n = report.infos));
    }
    if report.hints > 0 {
        parts.push(format!("{n} hint", n = report.hints));
    }
    if parts.is_empty() {
        println!("0 diagnostics — config is clean");
    } else {
        println!(
            "{total} diagnostic{plural} — {breakdown}",
            total = report.total,
            plural = if report.total == 1 { "" } else { "s" },
            breakdown = parts.join(", "),
        );
    }
}

/// Helper: prefix every line of `text` with `indent`. Preserves
/// embedded newlines so a multi-line message (`"line one\n  line two"`)
/// renders as a coherent block.
fn indent_block(text: &str, indent: &str) -> String {
    text.lines()
        .map(|l| format!("{indent}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn print_help() {
    println!("{banner}\n", banner = coderef_core::banner());
    print!("{}", help::GLOBAL_HELP);
}

/// `coderef help [<subcommand>...]`
///
/// Universal help entry point. `coderef help` prints global help;
/// `coderef help <subcommand>` prints the detailed sectioned help
/// for that subcommand (the same text as `coderef <subcommand> --help`).
/// Two-word subcommands like `config show` are accepted: `coderef
/// help config show`.
fn cmd_help(args: Vec<String>) -> ExitCode {
    let mut it = args.into_iter();
    match it.next().as_deref() {
        None | Some("help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") => {
            // help on the help command itself — describe how it works.
            print!("{}", help::HELP_HELP);
            ExitCode::SUCCESS
        }
        Some("config") => match it.next().as_deref() {
            Some("show") => {
                print!("{}", help::CONFIG_SHOW_HELP);
                ExitCode::SUCCESS
            }
            Some(other) => {
                eprintln!("coderef help config: unknown action `{other}`");
                eprintln!("try `coderef help config show`");
                ExitCode::from(2)
            }
            None => {
                eprintln!("coderef help config: pick an action — try `coderef help config show`");
                ExitCode::from(2)
            }
        },
        Some("list") => {
            print!("{}", help::LIST_HELP);
            ExitCode::SUCCESS
        }
        Some("check") => {
            print!("{}", help::CHECK_HELP);
            ExitCode::SUCCESS
        }
        Some("doctor") => {
            print!("{}", help::DOCTOR_HELP);
            ExitCode::SUCCESS
        }
        Some("patterns") => {
            print!("{}", help::PATTERNS_HELP);
            ExitCode::SUCCESS
        }
        Some("explain") => {
            print!("{}", help::EXPLAIN_HELP);
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("coderef help: unknown subcommand `{other}`");
            eprintln!();
            print_help();
            ExitCode::from(2)
        }
    }
}

/// `coderef patterns [--config <path>] [--report text|json] [<id>]`
///
/// Inspector for configured patterns. Without <id>, prints a summary
/// of all patterns. With <id>, prints full detail for that one.
fn cmd_patterns(args: Vec<String>) -> ExitCode {
    let mut config_path: Option<String> = None;
    let mut report = Report::Text;
    let mut id: Option<String> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(path) = it.next() else {
                    eprintln!("coderef patterns: --config requires a value");
                    return ExitCode::from(2);
                };
                config_path = Some(path);
            }
            "--report" => {
                let Some(kind) = it.next() else {
                    eprintln!("coderef patterns: --report requires `text` or `json`");
                    return ExitCode::from(2);
                };
                report = match kind.as_str() {
                    "text" => Report::Text,
                    "json" => Report::Json,
                    other => {
                        eprintln!(
                            "coderef patterns: --report must be `text` or `json` (got `{other}`)"
                        );
                        return ExitCode::from(2);
                    }
                };
            }
            "--help" | "-h" => {
                print!("{}", help::PATTERNS_HELP);
                return ExitCode::SUCCESS;
            }
            _ if id.is_none() => id = Some(arg),
            _ => {
                eprintln!("coderef patterns: unexpected argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }

    let cfg_path = config_path.unwrap_or_else(|| "./.coderef.jsonc".into());
    let cfg = match coderef_core::config::Config::from_file(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("coderef patterns: failed to load config `{cfg_path}`: {e}");
            return ExitCode::from(2);
        }
    };

    match (report, id) {
        (Report::Json, None) => match serde_json::to_string_pretty(&cfg.patterns) {
            Ok(s) => {
                println!("{s}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("coderef patterns: JSON encoding failed: {e}");
                ExitCode::from(3)
            }
        },
        (Report::Json, Some(name)) => {
            let Some(pat) = cfg.patterns.get(&name) else {
                eprintln!("coderef patterns: no pattern `{name}` in {cfg_path}");
                return ExitCode::from(2);
            };
            match serde_json::to_string_pretty(pat) {
                Ok(s) => {
                    println!("{s}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("coderef patterns: JSON encoding failed: {e}");
                    ExitCode::from(3)
                }
            }
        }
        (Report::Text, None) => {
            print_patterns_summary(&cfg);
            ExitCode::SUCCESS
        }
        (Report::Text, Some(name)) => {
            let Some(pat) = cfg.patterns.get(&name) else {
                eprintln!("coderef patterns: no pattern `{name}` in {cfg_path}");
                return ExitCode::from(2);
            };
            print_pattern_detail(&name, pat);
            ExitCode::SUCCESS
        }
    }
}

/// `coderef changes [--base <ref>] [--staged] [--config <path>] [--report text|json] [<root>]`
///
/// Runs the three-pass coupled-change verifier (DESIGN §10.5). Scans
/// the workspace for IfChange/ThenChange blocks, overlays a git diff
/// (`HEAD` by default, or `<base>..HEAD` with `--base`, or staged
/// changes with `--staged`), and reports any block touched by the
/// diff whose required peers/targets weren't also touched.
#[allow(clippy::too_many_lines)] // arg parsing + dispatch is naturally long
fn cmd_changes(args: Vec<String>) -> ExitCode {
    let mut config_path: Option<String> = None;
    let mut report = Report::Text;
    let mut base: Option<String> = None;
    let mut staged = false;
    let mut root: Option<String> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(p) = it.next() else {
                    eprintln!("coderef changes: --config requires a value");
                    return ExitCode::from(2);
                };
                config_path = Some(p);
            }
            "--report" => {
                let Some(k) = it.next() else {
                    eprintln!("coderef changes: --report requires `text` or `json`");
                    return ExitCode::from(2);
                };
                report = match k.as_str() {
                    "text" => Report::Text,
                    "json" => Report::Json,
                    other => {
                        eprintln!(
                            "coderef changes: --report must be `text` or `json` (got `{other}`)"
                        );
                        return ExitCode::from(2);
                    }
                };
            }
            "--base" => {
                let Some(b) = it.next() else {
                    eprintln!("coderef changes: --base requires a value");
                    return ExitCode::from(2);
                };
                base = Some(b);
            }
            "--staged" => staged = true,
            "--help" | "-h" => {
                print!("{}", help::CHANGES_HELP);
                return ExitCode::SUCCESS;
            }
            _ if root.is_none() => root = Some(arg),
            _ => {
                eprintln!("coderef changes: unexpected argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }

    let root = root.unwrap_or_else(|| ".".to_string());
    let cfg_path =
        config_path.unwrap_or_else(|| format!("{}/.coderef.jsonc", root.trim_end_matches('/')));
    let cfg = match coderef_core::config::Config::from_file(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("coderef changes: failed to load config `{cfg_path}`: {e}");
            return ExitCode::from(2);
        }
    };

    if !coderef_core::ifchange::ifchange_enabled(&cfg) {
        eprintln!(
            "coderef changes: no `kind: \"ifchange\"` pattern in the config — \
             declare one to enable coupled-change verification"
        );
        return ExitCode::from(2);
    }

    // 1. Scan the workspace for IfChange/ThenChange blocks.
    let (blocks, parse_errors) = match coderef_core::ifchange::scan_workspace_blocks(&root, &cfg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("coderef changes: scan failed: {e}");
            return ExitCode::from(2);
        }
    };

    // 2. Run git diff and parse it.
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(&root).arg("diff").arg("-U0");
    if staged {
        git.arg("--cached");
    }
    if let Some(b) = &base {
        git.arg(format!("{b}..HEAD"));
    }
    let out = match git.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("coderef changes: failed to run `git diff`: {e}");
            return ExitCode::from(2);
        }
    };
    if !out.status.success() {
        eprintln!(
            "coderef changes: `git diff` exited {code}: {stderr}",
            code = out.status.code().unwrap_or(-1),
            stderr = String::from_utf8_lossy(&out.stderr).trim()
        );
        return ExitCode::from(2);
    }
    let diff_text = String::from_utf8_lossy(&out.stdout);
    let changed = coderef_core::ifchange::parse_unified_diff(&diff_text);

    // 3. Verify.
    let r = coderef_core::ifchange::verify_changes(&blocks, &parse_errors, &changed);

    match report {
        Report::Text => print_changes_text(&r),
        Report::Json => match serde_json::to_string_pretty(&r) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("coderef changes: JSON encoding failed: {e}");
                return ExitCode::from(3);
            }
        },
    }

    if r.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_changes_text(r: &coderef_core::ifchange::ChangesReport) {
    for v in &r.violations {
        println!("[{kind}] {msg}", kind = v.kind, msg = v.message);
    }
    for e in &r.parse_errors {
        println!("[parse-error/{kind}] {msg}", kind = e.kind, msg = e.message);
    }
    if !r.violations.is_empty() || !r.parse_errors.is_empty() {
        println!();
    }
    println!(
        "Changes report: {bc} block(s), {cc} changed, {vc} violating, {nv} no-verify. \
         {vio} violation(s), {pe} parse-error(s).",
        bc = r.block_count,
        cc = r.changed_block_count,
        vc = r.violating_block_count,
        nv = r.no_verify_block_count,
        vio = r.violations.len(),
        pe = r.parse_errors.len(),
    );
}

/// `coderef explain [--config <path>] [--report text|json] <input>`
fn cmd_explain(args: Vec<String>) -> ExitCode {
    let mut config_path: Option<String> = None;
    let mut report = Report::Text;
    let mut input: Option<String> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(path) = it.next() else {
                    eprintln!("coderef explain: --config requires a value");
                    return ExitCode::from(2);
                };
                config_path = Some(path);
            }
            "--report" => {
                let Some(kind) = it.next() else {
                    eprintln!("coderef explain: --report requires `text` or `json`");
                    return ExitCode::from(2);
                };
                report = match kind.as_str() {
                    "text" => Report::Text,
                    "json" => Report::Json,
                    other => {
                        eprintln!(
                            "coderef explain: --report must be `text` or `json` (got `{other}`)"
                        );
                        return ExitCode::from(2);
                    }
                };
            }
            "--help" | "-h" => {
                print!("{}", help::EXPLAIN_HELP);
                return ExitCode::SUCCESS;
            }
            _ if input.is_none() => input = Some(arg),
            _ => {
                eprintln!("coderef explain: unexpected argument `{arg}`");
                return ExitCode::from(2);
            }
        }
    }

    let Some(input) = input else {
        eprintln!("coderef explain: missing <input>");
        eprintln!("usage: coderef explain [--config <path>] [--report text|json] <input>");
        return ExitCode::from(2);
    };

    let cfg_path = config_path.unwrap_or_else(|| "./.coderef.jsonc".into());
    let cfg = match coderef_core::config::Config::from_file(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("coderef explain: failed to load config `{cfg_path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let report_value = coderef_core::explain::explain(&cfg, &input);

    match report {
        Report::Json => match serde_json::to_string_pretty(&report_value) {
            Ok(s) => {
                println!("{s}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("coderef explain: JSON encoding failed: {e}");
                ExitCode::from(3)
            }
        },
        Report::Text => {
            print_explain_text(&report_value);
            ExitCode::SUCCESS
        }
    }
}

fn print_explain_text(report: &coderef_core::explain::ExplainReport) {
    println!("Input: {:?}", report.input);
    println!();
    if report.matches.is_empty() {
        println!("No configured pattern matches this input.");
    } else {
        let n = report.matches.len();
        let plural = if n == 1 { "" } else { "es" };
        println!("Match{plural}: {n}");
        println!();
        for m in &report.matches {
            println!(
                "  [{id}]  ({kind:?})",
                id = m.pattern_id,
                kind = m.pattern_kind
            );
            if let Some(desc) = &m.description {
                for line in desc.lines() {
                    println!("    {line}");
                }
            }
            println!("    matched:   {text:?}", text = m.matched_text);
            if !m.captures.is_empty() {
                let caps: Vec<String> = m
                    .captures
                    .iter()
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect();
                println!("    captures:  {captures}", captures = caps.join(", "));
            }
            // Block-kind patterns have no resolved target; the match
            // *is* the diagnostic. Skip the empty `target:` line.
            if m.pattern_kind == coderef_core::config::PatternKind::Block {
                println!("    effect:    block — this match would fail `coderef check`");
            } else {
                println!("    target:    {target}", target = m.target);
            }
            if let Some(title) = &m.title {
                println!("    title:     {title}");
            }
            if m.priority != 0 {
                println!("    priority:  {p}", p = m.priority);
            }
            for note in &m.scope_notes {
                for (i, line) in note.lines().enumerate() {
                    if i == 0 {
                        println!("    scope:     {line}");
                    } else {
                        println!("               {line}");
                    }
                }
            }
            for w in &m.resolution_warnings {
                println!("    warning:   {w}");
            }
            println!();
        }
    }
    if !report.non_matching_pattern_ids.is_empty() {
        let n = report.non_matching_pattern_ids.len();
        let plural = if n == 1 { "" } else { "s" };
        println!(
            "Did NOT match {n} pattern{plural}: {}",
            report.non_matching_pattern_ids.join(", ")
        );
    }
}

fn print_patterns_summary(cfg: &coderef_core::config::Config) {
    if cfg.patterns.is_empty() {
        println!("(no patterns configured)");
        return;
    }
    println!("PATTERNS ({n})", n = cfg.patterns.len());
    println!();
    for (id, pat) in &cfg.patterns {
        println!("  {id}");
        if let Some(desc) = &pat.description {
            for line in desc.lines() {
                println!("    {line}");
            }
        } else {
            println!("    (no description — add `description: \"...\"` to document this pattern)");
        }
        println!("    regex:  {regex}", regex = pat.regex);
        println!();
    }
    println!("Use `coderef patterns <id>` for full details.");
}

fn print_pattern_detail(id: &str, pat: &coderef_core::config::Pattern) {
    println!("PATTERN: {id}");
    println!();
    if let Some(desc) = &pat.description {
        println!("  Description:");
        for line in desc.lines() {
            println!("    {line}");
        }
        println!();
    }
    println!("  Kind:     {kind:?}", kind = pat.kind);
    println!("  Regex:    {regex}", regex = pat.regex);
    if let Some(target) = &pat.target {
        println!("  Target:   {target}");
    }
    if let Some(title) = &pat.title {
        println!("  Title:    {title}");
    }
    if let Some(scope) = &pat.scope {
        println!();
        println!("  Scope:");
        println!("    commentsOnly: {b}", b = scope.comments_only);
        if !scope.include.is_empty() {
            println!("    include:");
            for g in &scope.include {
                println!("      - {g}");
            }
        }
        if !scope.exclude.is_empty() {
            println!("    exclude:");
            for g in &scope.exclude {
                println!("      - {g}");
            }
        }
    }
}
