//! coderef CLI.
//!
//! v0.0.0 is a placeholder so the workspace builds. Real subcommands land
//! per the v0.1 roadmap in `DESIGN.md` §19.

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("{}", coderef_core::banner());
        }
        Some("--help" | "-h") | None => print_help(),
        Some(other) => {
            eprintln!("coderef: unknown subcommand `{other}`");
            eprintln!();
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "{banner}

Usage:  coderef <subcommand> [options]

Subcommands (planned; not yet implemented — see DESIGN.md §19):
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
