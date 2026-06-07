//! Help text for each subcommand. Pulled out of `main.rs` so the
//! command-dispatch logic stays readable; each `--help` arm is one
//! line that prints the matching constant.
//!
//! Format follows the standard sectioned shape:
//!   USAGE / DESCRIPTION / ARGUMENTS / OPTIONS / EXIT CODES / EXAMPLES.
//!
//! Single source of truth: editing the help text here automatically
//! flows through every consumer (the CLI itself, doc builds, future
//! man-page generation). No duplication between code + docs.

pub const CONFIG_SHOW_HELP: &str = "\
USAGE
    coderef config show <path>

DESCRIPTION
    Parse the JSONC config at <path> and pretty-print the deserialised
    `Config` as JSON. Useful for verifying that an edit doesn't break
    the schema, or for inspecting how the engine sees a particular
    config file after applying defaults.

ARGUMENTS
    <path>     Path to a .coderef.jsonc (or .coderef.json).

OPTIONS
    -h, --help     Show this help and exit.

EXIT CODES
    0  Config loaded + serialised successfully.
    2  Usage error, file missing, or parse failure.
    3  Internal serialisation failure (should never happen).

EXAMPLES
    coderef config show .coderef.jsonc
    coderef config show examples/minimal.coderef.jsonc | jq .patterns
";

pub const LIST_HELP: &str = "\
USAGE
    coderef list [OPTIONS] <root>

DESCRIPTION
    Walk the workspace rooted at <root>, find every reference that
    matches any configured pattern, and emit them. By default emits
    one line per reference in a human-readable layout; --json emits
    a JSON array of `Reference` records (the same shape the engine
    produces internally and that the conformance harness consumes).

    Honours .gitignore (transitively via the `ignore` crate),
    workspace-level `ignore[]` globs from the config, and per-pattern
    `scope.include` / `scope.exclude` globs.

ARGUMENTS
    <root>     Workspace directory to scan.

OPTIONS
    -c, --config <path>
        Path to .coderef.jsonc. Defaults to <root>/.coderef.jsonc.

    --json
        Emit JSON instead of one-line-per-reference text. JSON output
        is the contract for tools / editors / the conformance harness.

    -h, --help
        Show this help and exit.

EXIT CODES
    0  Scan completed successfully (regardless of how many refs were
       found — `list` is read-only, not a verifier).
    2  Usage / config / scan error.
    3  Output encoding error (when --json is given).

EXAMPLES
    coderef list .
    coderef list --json . | jq '.[] | select(.pattern_kind == \"local\")'
    coderef list --config /tmp/cfg.jsonc /path/to/workspace
";

pub const CHECK_HELP: &str = "\
USAGE
    coderef check [OPTIONS] <root>

DESCRIPTION
    Scan the workspace at <root> and verify every reference. URL
    targets are verified via HTTP HEAD (falling back to GET on a
    405 response); local-path targets via filesystem existence;
    coupled-change / command kinds are skipped in v0.1.

    Returns a `CheckReport` containing per-reference results and
    aggregate counts.

ARGUMENTS
    <root>     Workspace directory to scan.

OPTIONS
    -c, --config <path>
        Path to .coderef.jsonc. Defaults to <root>/.coderef.jsonc.

    --report text|json
        Output format. `text` (default) prints one line per reference;
        `json` emits the `CheckReport` for downstream tooling.

    --timeout-ms <N>
        Per-request HTTP timeout in milliseconds. Default: 10000.

    -h, --help
        Show this help and exit.

EXIT CODES
    0  All references resolved (or were skipped).
    1  At least one reference broke (a `BrokenStatus`, `BrokenNetwork`,
       or `NotFound` outcome).
    2  Usage / config / scan error.
    3  Output encoding error (when --report json is given).

EXAMPLES
    coderef check .
    coderef check --report json . | jq '.broken, .ok'
    coderef check --timeout-ms 2000 --config /tmp/cfg.jsonc /path/to/workspace
";

pub const DOCTOR_HELP: &str = "\
USAGE
    coderef doctor [OPTIONS] [<root>]

DESCRIPTION
    Run integrity checks against the config. Two passes:

      - Static checks   (don't need a workspace): regex compilation,
                        capture references, variable namespaces,
                        target shape, etc.
      - Scan-dependent  (need a workspace): pattern.unused (a pattern
                        is declared but matches nothing). Skip via
                        --no-scan if you only want the static pass.

    Output is structured as multi-line diagnostics: severity header,
    indented message and any per-pattern context, then an indented
    hint section.

ARGUMENTS
    <root>     Workspace directory to scan. Required unless --no-scan
               is set; defaults to `.` when no <root> is given but
               scanning is enabled.

OPTIONS
    -c, --config <path>
        Path to .coderef.jsonc. Defaults to <root>/.coderef.jsonc, or
        ./.coderef.jsonc if no <root> is given.

    --report text|json
        Output format. `text` (default) prints multi-line diagnostics;
        `json` emits the `DoctorReport` for tooling.

    --no-scan
        Static checks only — skip the workspace scan (so pattern.unused
        and any other scan-dependent diagnostics don't run). Useful in
        CI for validating a config without a workspace context.

    -h, --help
        Show this help and exit.

EXIT CODES
    0  No error-severity diagnostics. (Warnings, info, hints still print
       but don't fail the run; pattern.unused is Info by default —
       harmless for shared / template configs.)
    1  At least one error-severity diagnostic.
    2  Usage / config error.
    3  Output encoding error (when --report json is given).

EXAMPLES
    coderef doctor .
    coderef doctor --no-scan --config .coderef.jsonc
    coderef doctor --report json . | jq '.diagnostics[] | select(.severity == \"error\")'
";

pub const PATTERNS_HELP: &str = "\
USAGE
    coderef patterns [OPTIONS] [<id>]

DESCRIPTION
    Inspect configured patterns. Without <id>, prints a one-paragraph
    summary of every pattern. With <id>, prints the full detail for
    that one pattern (description, kind, regex, target, scope rules,
    severity overrides).

    The `description` field on each pattern is what carries the
    pattern's intent — what it's for, when to use it. Strongly
    recommended for shared / template configs; otherwise consumers
    have to reverse-engineer intent from the regex.

ARGUMENTS
    <id>     Optional pattern id (a key in `patterns`). If omitted,
             lists all patterns.

OPTIONS
    -c, --config <path>
        Path to .coderef.jsonc. Defaults to ./.coderef.jsonc.

    --report text|json
        Output format. `text` (default) is human-oriented; `json`
        emits the underlying `Config.patterns` map (or a single
        `Pattern` if <id> is given).

    -h, --help
        Show this help and exit.

EXIT CODES
    0  Pattern(s) listed successfully.
    2  Usage / config error, or <id> not found in the config.
    3  Output encoding error (when --report json is given).

EXAMPLES
    coderef patterns
    coderef patterns docref
    coderef patterns --report json | jq 'keys'
";

pub const GLOBAL_HELP: &str = "\
USAGE
    coderef <subcommand> [options]
    coderef help [<subcommand>]

SUBCOMMANDS
    config show <path>           Parse + pretty-print a .coderef.jsonc
    list [opts] <root>           Scan + emit every reference (text or JSON)
    check [opts] <root>          Scan + verify every reference; exit 1 on
                                 broken refs
    doctor [opts] [<root>]       Static + scan-dependent integrity checks
    patterns [opts] [<id>]       Inspect configured patterns
    help [<subcommand>]          Show detailed help for a subcommand

    `coderef help <subcommand>` and `coderef <subcommand> --help` produce
    the same output.

PLANNED (per DESIGN.md §20, not yet implemented)
    changes     Coupled-change verifier (v0.2)
    upgrade     Rewrite legacy markers (v0.3)
    explain     Show resolution for a single reference token
    cache       Manage the verification cache
    lsp         LSP server mode (v0.4)

OPTIONS
    -h, --help      Show this help
    -V, --version   Show version banner

For the working specification, see DESIGN.md in the repository root.
";

pub const HELP_HELP: &str = "\
USAGE
    coderef help                 Print global help (same as `coderef --help`).
    coderef help <subcommand>    Print detailed help for <subcommand>.
    coderef help config show     Print detailed help for the `config show` action.

DESCRIPTION
    Universal help entry point. The output is identical to invoking
    the subcommand with `--help` directly:

        coderef help check       ===  coderef check --help

EXAMPLES
    coderef help
    coderef help check
    coderef help patterns
    coderef help config show
";
