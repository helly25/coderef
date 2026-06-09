# coderef — Design Document

Status: **Draft v0.6** — 2026-06-06
Owner: Marcus Boerger (`helly25`)
License of this document and code: Apache-2.0

---

## 1. Overview

`coderef` formalises the well-known but ad-hoc pattern of embedding cross-references
inside source code — `TODO(@user)`, `TODO(b/123)`, `JIRA(PROJ-123)`,
`DOCREF(/docs/x.md)`, `RFC(8259)`, `CVE-2024-1234`, and so on — and makes them
first-class:

- **One declaration** of each pattern in a project-level JSONC config.
- **Click-to-open** and **hover preview** in editors (VSCode first, LSP later).
- **CLI verifier** that resolves every reference and reports breakage, suitable
  for `pre-commit`, `lefthook`, `husky`, and CI.
- **Verification modes**: full-repo (greenfield) and changed-lines (incremental,
  for retrofits to large existing codebases).
- **Pattern-system integrity checks**: detect ambiguous/overlapping patterns
  before they cause silent mis-routing.
- **Language-aware marker placement** *(v0.2)*: patterns automatically
  require the right comment prefix per language — `//` in C/C++/TS/Go/Rust,
  `#` in Python/Ruby/Shell/YAML, `--` in SQL/Haskell, `<!--` in
  HTML/Markdown — and can optionally require markers to be on a line by
  themselves (§5.4.2, §7.5). v0.1 ships with the coarser `commentsOnly`
  filter only.
- **Coupled-change enforcement** *(v0.2)* (`IfChange/ThenChange`):
  code/docs that must change together are linked by markers — explicit
  targets with optional line ranges, id-anchored groups, refactor-stable
  named labels, or *(v0.4)* composable ids that reference other
  patterns. Verified in the same pre-commit hook (§10).
- **Auto-upgrade of legacy markers** *(v0.3)* (`coderef upgrade`):
  bulk-converts plain `TODO ` / `TODO:` comments into canonical
  `TODO(@user)` form, using `git blame` to fill in the author when no
  marker is present and cross-pattern resolution when the comment
  already contains a URL or ticket id (§11).
- **Multi-target references** *(v0.3)* with priority and per-target
  verification policy — one `@user` can resolve to a primary user page
  plus alternate locations (epitaph page, external partner profile, ...),
  all surfaced through the editor; the verifier requires only the
  targets marked `required` to pass (§5.3.1).
- **Unverified-reference marker (`?`)** *(v0.2)* so during the
  adoption phase or for in-flight tickets, authors can write
  `TODO(?@user)` to mean "this exists but skip verification for now."
  Doctor counts them and warns when they linger past a configurable
  age threshold (§5.6).
- **Visual config editor in the VSCode extension** *(v0.3)*: a
  block-based form UI with style templates (user reference, ticket
  reference, local doc, coupled-change, …), live regex preview,
  drag-reorder for multi-target lists, and source-pane round-trip for
  everything the form can't cover (§14.6).

Version tags in brackets show the slot in §20's roadmap. v0.1 — the
minimum viable release — keeps to **`url`/`local` single-target
patterns, basic HTTP verification, click-to-open in VSCode, and
pre-commit integration**; everything else above is part of v0.2–v0.4.
- **Network-profile aware**: distinguishes internal vs external hosts, applies
  proxy settings, and degrades gracefully off-VPN.
- **Single variable system**: one substitution layer (`${...}`) shared by
  editor and CLI, so configs behave identically everywhere.

Implementation: **Rust** for the CLI/core (single static binary, fast cold
start, fearless concurrency, WASM-ready for future editor embedding) and
**TypeScript** for the VSCode extension (native API access; shells out to the
Rust binary for scanning/verification). See §4.

This is *not* a TODO tracker, *not* a tag-uniqueness enforcer (that's
[`tagref`](https://github.com/stepchowfun/tagref)), *not* a generic markdown
link checker (that's [`lychee`](https://github.com/lycheeverse/lychee)). It's the
glue between a regex you put in source and the thing it points at — plus the
linter that keeps groups of those references in sync as you change them.

### 1.1 Why now

Several adjacent tools cover slices:

| Tool                           | Editor | CLI | Custom regex | URL templates | Local-file shortcuts | Anchor verify | Diagnostics | Verifier | Network profiles | Coupled-change | Codemod |
| ------------------------------ | :----: | :-: | :----------: | :-----------: | :------------------: | :-----------: | :---------: | :------: | :--------------: | :------------: | :-----: |
| VSCode Regex Robin             |   ✓    |     |      ✓       |       ✓       |                      |               |             |          |                  |                |         |
| VSCode Linker (sharten)        |   ✓    |     |      ✓       |       ✓       |          ~           |               |             |          |                  |                |         |
| Comment Anchors                |   ✓    |     |      ~       |               |          ~           |               |             |          |                  |                |         |
| tagref                         |        |  ✓  |              |               |                      |               |             |    ✓     |                  |                |         |
| lychee                         |        |  ✓  |              |               |                      |       ✓ HTML  |             |    ✓     |        ~         |                |         |
| muffet                         |        |  ✓  |              |               |                      |       ✓ HTML  |             |    ✓     |                  |                |         |
| markdown-link-check            |        |  ✓  |              |               |                      |               |             |    ✓     |                  |                |         |
| todocheck                      |        |  ✓  |              |     fixed     |                      |               |             |    ✓     |                  |                |         |
| JetBrains Issue Nav.           |   ✓    |     |      ✓       |       ✓       |                      |               |             |          |                  |                |         |
| ifttt-lint (simonepri/ebrevdo) |        |  ✓  |              |               |                      |               |             |    ✓     |                  |       A,B      |         |
| checksync (Khan lineage)       |        |  ✓  |              |               |                      |               |             |    ✓     |                  |       A,B      |         |
| **coderef** (this design)      | **✓**  | **✓** | **✓**      | **✓**         | **✓**                | **✓ HTML + Markdown via slugifier** | **✓**       | **✓**    | **✓**            | **A,B,C + labels** | **✓**   |

Nothing unifies *editor + CLI verifier + custom patterns + local-file shortcuts +
network awareness + integrity checks + coupled-change enforcement with line ranges
and composable IDs + auto-upgrade codemods*. Internally at large companies
(Google's `TODO(user)` / `TODO(b/1234567)` + `LINT.IfChange/ThenChange`) this
exists but is not portable. `coderef` ports the idea.

---

## 2. Goals & Non-goals

### Goals

1. A single JSONC config drives editor and CLI behaviour identically.
2. Reference patterns are user-defined via regex with named captures.
3. Targets are produced by URL templates that interpolate captures and
   environment-derived values through one variable system.
4. Local-file references support ergonomic shortcuts (omit extension, directory
   → index file, anchor support); leading `/` makes the workspace-root anchor
   explicit and obvious.
5. Verification works in two modes: **full** and **changed-lines** (git-diff
   driven).
6. Verifier classifies failures by severity and is configurable (e.g. accept
   `200/301/302/307/308`; warn on `403`; fail on `404`).
7. The **pattern system itself** is verifiable: declared patterns are checked
   for overlap/ambiguity so two patterns never silently fight over the same
   match.
8. **Coupled-change references** (`IfChange/ThenChange`) are first-class
   patterns, supporting explicit targets with optional line ranges, id-anchored
   groups, and composable ids that reference other patterns.
9. **Comment-prefix matching is language-aware**. Patterns are written without
   language-specific prefix syntax; the scanner composes the appropriate
   prefix per file (Python `#`, C/C++ `//`, etc.), enforces own-line-only
   placement when configured, and ships a built-in language table that is
   user-overridable.
10. **Adoption is painless.** A bulk-conversion subcommand (`coderef upgrade`)
    turns legacy `TODO ` and `TODO:` comments into the canonical
    `TODO(@user)` form, using `git blame` for missing usernames and the
    existing pattern set to resolve URLs / ticket ids when present.
11. Network profiles let the *same* config behave correctly on a corporate
    network, on VPN, and on a contractor laptop with no internal access.
12. The editor experience uses native primitives only — `DocumentLinkProvider`,
    `HoverProvider`, `DiagnosticCollection`, `CodeActionProvider` — so reference
    handling composes with every other extension.
13. **One reference can point at several targets** with declared priority and
    per-target verification policy, so the common case ("@user has a home
    page, an epitaph page, and an external profile, and we want all three
    discoverable from the editor") is first-class rather than encoded across
    many duplicated patterns.
14. **An unverified-reference marker** lets authors flag references they want
    to keep in the source without the verifier failing on them — useful for
    bulk-conversion phases and for refs whose targets aren't ready yet.
15. Distribution is boring: a pre-built static binary per platform, wrapped by
    an npm package, so `pre-commit` consumers and `npx`-style invocations Just
    Work; VSCode Marketplace for the extension; `pre-commit-hooks.yaml` shipped
    in this repo.

### Non-goals (initially)

- Replacing dedicated tools: not a JIRA client, not a code search, not a
  documentation generator.
- AST-aware parsing. We scan text and trust the regex. (Optional comment-only
  scoping is a config knob, see §5.4.)
- Cross-repo resolution graphs *or* cross-repo coupled-change graphs. Each
  repo has its own config.
- Anything resembling an LLM step. Patterns are deterministic.

---

## 3. Glossary

| Term                        | Meaning                                                                                                                                                                                                                                                                                                                                                               |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Pattern**                 | A named regex with named captures. Match results become a *reference*.                                                                                                                                                                                                                                                                                                |
| **Reference**               | A concrete instance found in source: `{pattern, captures, file, range}`.                                                                                                                                                                                                                                                                                              |
| **Resolver**                | A function/strategy that turns a reference into a *target* (URL, file path, etc).                                                                                                                                                                                                                                                                                     |
| **Target**                  | The thing a reference points at: a URL, a local file path (optionally with anchor), or a custom command.                                                                                                                                                                                                                                                              |
| **Action**                  | What to do with a target when the user invokes it: `open`, `preview`, `verify`.                                                                                                                                                                                                                                                                                       |
| **Profile**                 | A named network configuration: which hosts are internal, what proxy to use, what to skip.                                                                                                                                                                                                                                                                             |
| **Verifier**                | The component that asks "does this target exist?" for each reference, with optional caching and concurrency.                                                                                                                                                                                                                                                          |
| **Workspace**               | The repository root, detected via `.git`/`.hg` or explicit `coderef.workspaceRoot`.                                                                                                                                                                                                                                                                                   |
| **Variable**                | A `${...}`-syntax placeholder resolved by the shared variable system (see §8).                                                                                                                                                                                                                                                                                        |
| **Doctor**                  | The integrity-check subcommand that audits patterns for overlap/ambiguity (see §9).                                                                                                                                                                                                                                                                                   |
| **Coupled-change block**    | A pair of `IfChange`/`ThenChange` markers and the source lines between them, tracked together by the verifier (§10).                                                                                                                                                                                                                                                  |
| **NoVerify**                | An opt-out marker that bypasses a specific verifier check for one block or one commit, with a required reason (§10.6).                                                                                                                                                                                                                                                |
| **Prefix policy**           | Per-pattern rules controlling which comment context (`lineComment`/`blockComment`/`any`) a match may appear in, and whether the marker must be on its own line (§5.4.2).                                                                                                                                                                                              |
| **Language table**          | The mapping from file extensions / language ids to comment-syntax definitions used by the prefix-policy scanner (§7.5).                                                                                                                                                                                                                                               |
| **Codemod**                 | A safe, idempotent transformation of source text — in coderef, the `upgrade` subcommand that rewrites legacy markers (§11).                                                                                                                                                                                                                                           |
| **Upgrade rule**            | A `{match, rewrite, ...}` entry under a pattern's `upgrade.rules` that defines one legacy-form transformation (§11.2).                                                                                                                                                                                                                                                |
| **Blame mapping**           | The `blame.userMapping` table that resolves a `git blame` author email to a project username (§11.4).                                                                                                                                                                                                                                                                 |
| **Multi-target pattern**    | A pattern whose `targets[]` lists more than one URL, each with a priority and verification policy (§5.3.1).                                                                                                                                                                                                                                                           |
| **Primary target**          | The highest-priority entry in `targets[]`; what click-to-open opens by default. Ties broken by declaration order.                                                                                                                                                                                                                                                     |
| **Unverified marker**       | A captured prefix (default `?`) inside a reference like `TODO(?@user)` that tells the verifier to skip this match (§5.6).                                                                                                                                                                                                                                             |
| **Category**                | A declared semantic grouping (`files` / `people` / `tickets` / `standards` / `urls` / `coupled-change` / `other` + user-defined) used by the references browser and category-aware doctor checks (§5.7).                                                                                                                                                              |
| **Label / EndLabel**        | *Optional compat form.* A named region inside a file (`Label('name') ... EndLabel`) addressable by name (`path:label-name`). The recommended primary form for refactor-stable targeting puts the name on the `IfChange` itself (`IfChange('name') ... ThenChange(path:name)`); the Label-bracketed variant exists for codebases mirroring `ebrevdo/ifttt-lint` (§10). |
| **Checksum entry**          | A row in `.coderef-checksums.json` pairing a `path:N-M` target with a stored content hash for drift detection (§10.14, v0.4).                                                                                                                                                                                                                                         |
| **Submodule pass-through**  | Treating a `git submodule` directory as part of the workspace for scanning and coupled-change resolution; the *only* cross-repo mechanism supported (§6.4, v0.4; non-goal of linked-repo manifests §23.1).                                                                                                                                                            |
| **Profile-scoped variable** | A `${config:variables.x}` whose resolved value depends on the active network profile (§12.2.1).                                                                                                                                                                                                                                                                       |
| **Workspace lock**          | The advisory `flock` on `<workspace>/.coderef/lock` that serialises write-mode subcommands (`upgrade --apply`, `checksum {add,update,remove}`, `cache clear`) — §11.10.                                                                                                                                                                                               |

---

## 4. High-Level Architecture

```
                       ┌───────────────────────────────────────────┐
                       │   .coderef.jsonc / .config/coderef.jsonc  │
                       └────────────────────┬──────────────────────┘
                                            │
                                  ┌─────────▼─────────┐
                                  │  coderef-core     │
                                  │      (Rust)       │
                                  │  - config loader  │
                                  │  - var resolver   │
                                  │  - regex engine   │
                                  │  - scanner (rayon)│
                                  │  - doctor (lint)  │
                                  │  - resolvers      │
                                  │  - verifier (HTTP)│
                                  │  - changes (ICTC) │
                                  │  - upgrade (mod)  │
                                  │  - profile detect │
                                  └─────────┬─────────┘
                                            │
                                ┌───────────┴───────────┐
                                │   coderef CLI bin     │
                                │   (Rust, clap)        │
                                └─────┬──────────┬──────┘
                                      │          │
              ┌───────────────────────┘          └────────────────────────────┐
              │                                                               │
   ┌──────────▼──────────┐                                     ┌──────────────▼──────────────┐
   │ @helly25/coderef    │                                     │     helly25.coderef         │
   │ (npm wrapper, TS)   │                                     │     VSCode extension        │
   │ downloads bin at    │                                     │     (TypeScript)            │
   │ install for the     │                                     │  - DocumentLinkProvider     │
   │ host platform       │                                     │  - HoverProvider            │
   └──────────┬──────────┘                                     │  - DiagnosticCollection     │
              │                                                │  - CodeActionProvider (§11) │
   ┌──────────▼──────────┐                                     │  - Commands                 │
   │ pre-commit / hooks  │                                     │  shells out to coderef bin  │
   │ husky / lefthook /  │                                     │  via JSON I/O               │
   │ CI runners          │                                     └─────────────────────────────┘
   └─────────────────────┘
```

### 4.1 Language choice: Rust CLI + TypeScript extension

The CLI and core library are **Rust**; the VSCode extension is **TypeScript**.
Decision rationale, in order of weight for this project:

1. **Cold start matters every commit.** Pre-commit hooks run on every git
   commit and on every CI job. A Rust binary starts in 2–8 ms; a Node CLI with
   our planned deps starts in 100–200 ms. The difference is the gap between
   "invisible" and "users notice and disable the hook."
2. **Throughput matters for adoption-phase workloads.** Initial bulk-conversion
   of `TODO ` comments via `coderef upgrade`, and the modern reality of
   AI-driven refactor commits touching hundreds of files, both push the
   scanner past the point where Node's per-file overhead is comfortable. Rust
   regex throughput is ~5–10× Node's; memory is 2–3× lower; both compound
   under load.
3. **WASM-to-editor path stays open.** Today the extension shells out to the
   CLI binary. Tomorrow we can compile `coderef-core` to WASM and embed it
   in-process inside VSCode for guaranteed regex-engine parity — no
   "documented subset" caveat, just one engine in both hosts. Rust → WASM is
   the only sensible source language for this; Go → WASM ships a multi-MB
   runtime and immature goroutines.
4. **Regex engine consistency.** `regex` (RE2-style, fast) and `fancy-regex`
   (lookaround, backtracking) are the same Rust crate ecosystem with the same
   syntax — `fancy-regex` adds features without changing the surface. Go would
   have forced us to juggle stdlib `regexp` + the separately-maintained
   `regexp2` package with different syntax and a ~10× perf gap between them.
5. **Type system & error handling.** This tool has many fallible code paths
   (config parse, regex compile, file I/O, HTTP, git, blame). Rust's
   `Result<T, E>` + `?` and exhaustive matching catch whole classes of bugs
   at compile time that Go would surface only at runtime.

Trade-offs accepted:

- **Build time.** Clean cargo build is 30–90s; incremental builds are 1–5s.
  Mitigated by `Swatinem/rust-cache` in CI, `cargo check` (~1s) during
  inner-loop dev, and `mold` linker if needed.
- **Cross-compile matrix.** Five-platform release CI takes ~15 minutes vs Go's
  ~30 seconds. Run on tag only; not a daily cost.
- **Contributor onboarding.** Rust has a real learning curve, but: (a) the
  current cohort of CLI-tool contributors in 2026 leans Rust (ripgrep, fd,
  eza, bat, hyperfine, just, mdbook, gitui, tokei, delta, dust, bottom...),
  and (b) the maintainer comes from C++ — the C++→Rust transition is hours,
  not weeks.

Alternatives considered:

| Alternative                | Why rejected                                                                                                                                                                       |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript everywhere      | Cold start ~150 ms felt on every commit. Scan throughput limits adoption-phase bulk operations. No native WASM-shareable-engine story.                                             |
| Go CLI + TS extension      | RE2 lacks lookaround; `regexp2` is a separate engine with ~10× perf gap. Go → WASM is poor. C++→Go feels like stepping sideways (no RAII, weaker types).                           |
| Python CLI + TS extension  | CPython is *slower* than Node V8 for this workload (1.5–3×) plus heavier import startup (50–150 ms). Distribution via pyenv/venv is messy. VSCode parity requires LSP indirection. |

#### 4.1.1 Source-of-truth contract

Two distinct meanings of "source of truth" matter here:

- **Implementation source of truth: `coderef-core`** (the Rust crate).
  All semantics — regex, variable resolution, resolver, scanner,
  coupled-change algorithm, doctor checks — live in one place and one
  place only. The CLI binary, the WASM module, the LSP server, and
  any future plugin call into this crate. There is no parallel
  implementation anywhere else.

- **Behavioural source of truth: the CLI binary.** Everything a user
  or a plugin observes as "what coderef does" is defined by
  `coderef <subcommand> --report json` on a given input. The WASM
  module used by the VSCode extension, the LSP server in v0.4, and
  every future editor plugin (JetBrains, Neovim/Helix via LSP, …) are
  **conformance-tested against the CLI**. When a plugin's behaviour
  diverges from the CLI's for the same input, the plugin is wrong
  by definition; the CLI's output is the reference.

This is what makes "multiple plugins for multiple IDEs" tractable.
Every plugin developer has the same artefact to validate against —
`cargo install coderef` + run the CLI — without arguing about which
host is canonical.

The CI conformance job (§16, post-v0.1) feeds a curated input corpus
through the CLI and through the WASM module, diffs the JSON outputs,
and fails on any divergence. Same shape extends to the LSP server when
it ships in v0.4: the LSP responses for a corpus of fixture documents
are golden-tested against `coderef --report json` for the same inputs.

### 4.2 Components and key crates / packages

| Component                    | Package                       | Notes                                                                                                                                                                                                                                                                          |
| ---------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Core library                 | `coderef-core` (Rust crate)   | Config, variables, regex engine driver, scanner, resolvers, verifier, changes, upgrade, doctor. No editor deps.                                                                                                                                                                |
| CLI binary                   | `coderef` (Rust crate)        | `clap`-based subcommand router. Cross-compiled for darwin/{amd64,arm64}, linux/{amd64,arm64,musl}, windows/amd64.                                                                                                                                                              |
| npm wrapper                  | `@helly25/coderef`            | Tiny TS shim. `postinstall` script picks the correct prebuilt binary by platform/arch and exposes the `coderef` bin. Lets `pre-commit` `language: node` and `npx -y @helly25/coderef …` Just Work.                                                                             |
| **WASM module (v0.1)**       | `@helly25/coderef-core-wasm`  | `coderef-core` compiled via `wasm-bindgen`. Editor imports in-process for scan/hover/document-link hot paths. Hard cap: 1.5 MB gzipped; target ~600 KB. **No** I/O (file walker stays host-side via `vscode.workspace.findFiles`); **no** HTTP (verifier stays in the binary). |
| VSCode extension             | `helly25.coderef` (TS)        | Imports `@helly25/coderef-core-wasm` from v0.1 for hot-path scanning; spawns the bin for verify/upgrade/changes/doctor/LSP (§14.5.1).                                                                                                                                          |
| LSP server (v0.4)            | same Rust bin in `--lsp` mode | Implements `documentLink`, `hover`, `publishDiagnostics`, `codeAction`. Neovim/Helix/JetBrains plug in via standard LSP. WASM build of `coderef-core` is also available to LSP clients that prefer in-process embedding.                                                       |
| JetBrains plugin (post-v0.4) | thin Kotlin LSP-client        | Mostly `plugin.xml` + LSP plumbing. Backlog (§20.5).                                                                                                                                                                                                                           |

Key Rust dependencies for `coderef-core` / `coderef-cli`:

| Crate                     | Used for                                                                                            |
| ------------------------- | --------------------------------------------------------------------------------------------------- |
| `serde` + `serde_json`    | Config (de)serialisation.                                                                           |
| `jsonc-parser` or `json5` | Tolerant JSONC reader (preserves comments, accepts trailing commas).                                |
| `regex`                   | Fast RE2-style engine for hot paths (internal patterns, scanner).                                   |
| `fancy-regex`             | User-facing patterns — adds lookaround, backreferences, same syntax superset.                       |
| `regex-syntax`            | AST analysis for doctor's synthetic-match overlap check (§9.4).                                     |
| `clap`                    | CLI subcommands, args, help.                                                                        |
| `tokio` + `reqwest`       | Async HTTP client for the verifier; supports proxy via env (`HTTPS_PROXY`) or config (§12).         |
| `globset` / `ignore`      | Gitignore-style globs for `ignore`, `scope.include/exclude`, target globs.                          |
| `rayon`                   | Data-parallel file scanning.                                                                        |
| `tower-lsp`               | LSP server (v0.4).                                                                                  |
| (shell out to `git`)      | Diff parsing (`git diff -U0 --cached`), blame (`git blame --porcelain`), rename detection.          |

Tests: `cargo test` for unit + integration; `insta` for snapshot tests of
CLI output; `@vscode/test-electron` for the extension end-to-end.
Build/lint: `cargo build --release`, `cargo clippy -- -D warnings`,
`cargo fmt --check`. For the extension: `tsc` strict mode + ESLint v9 flat
config (mirrors `vscode-iwyu`). Packaging: `cargo dist` for binary releases,
`vsce` for the extension, `npm publish` for the wrapper.

### 4.3 Repository layout

```
coderef/
├── crates/
│   ├── coderef-core/    # the library crate
│   └── coderef-cli/     # the binary crate (depends on -core)
├── npm/
│   └── coderef/         # @helly25/coderef wrapper (TS, downloads bin)
├── extension/           # helly25.coderef VSCode extension (TS)
├── examples/            # sample .coderef.jsonc, .pre-commit-config.yaml,
│                        # ifchange-lint-compat.jsonc (§10.12),
│                        # blame-mapping.jsonc (§11.4)
├── schema/              # JSON Schema for the config (publishable)
├── docs/                # extended docs beyond this DESIGN.md
├── .pre-commit-hooks.yaml
├── Cargo.toml           # workspace
└── Cargo.lock
```

---

## 5. Reference Model

### 5.1 Patterns

A *pattern* is a named regex with named captures plus instructions on how to
turn captures into a target. Patterns live in the config under `patterns.<id>`.

Minimum required fields: `regex` and `target`. Everything else is optional.

```jsonc
{
  "patterns": {
    "todo-user": {
      "regex": "TODO\\(@(?<user>[a-zA-Z][a-zA-Z0-9._-]{0,63})\\)",
      "target": "https://users.example.com/${user}",
      "title": "User: ${user}",
      "verify": { "enabled": true, "profile": "internal" }
    }
  }
}
```

All substitution uses the variable system described in §8 with `${...}` syntax.
Named regex captures are exposed both as bare names (`${user}`) and explicitly
(`${capture:user}`). The explicit form wins on namespace conflicts.

### 5.2 Reference kinds

Each pattern is classified by `kind`:

- `url` (default) — `target` is treated as a URL. The engine does not
  parse or validate URL syntax beyond what the verifier (§13) needs; any
  URL form is acceptable, including:
  - Standard `http(s)://example.com/path` URLs.
  - **Internal shorteners / "go links"** such as `OPTIMIZE(http://go/here)`
    — organisation-internal hostnames are fine; the verifier resolves
    them through whichever network profile applies (§12), and the
    pre-commit hook / CLI verifier checks them just like any other URL.
  - **Scheme-less internal forms** such as `OPTIMIZE(go/here)` once the
    pattern's `target` template prepends a scheme
    (`"target": "http://${short}"`).
  - **Custom schemes** like `mailto:`, `file://`, `slack://`,
    `obsidian://`, `vscode://` — opened by the OS / VSCode via
    `vscode.open`. For non-HTTP schemes the verifier defaults to
    "scheme-skipped" with a per-pattern override (`verify.enabled:
    false`, or `verify.command` for a custom probe).
- `local` — `target` is a workspace-relative path; resolved via §6. The
  leading-`/` convention (§6.1) makes the workspace-root anchor explicit.
- `block` — *"must not be present"*. No target; every match is a
  failure surfaced by `coderef check` (and refused by the pre-commit
  hook). Intended for `DO NOT COMMIT` / `DO NOT MERGE` /
  `DONOTMERGE` / `NOCOMMIT` source guards. Typically used with
  `scope.commentsOnly: true` so docs that *describe* the marker
  don't self-match. Severity-overridable like any other pattern; a
  user who wants the marker as a warning instead of an error sets
  `severity: { default: "warning" }`. Shipped preset:
  `examples/block-markers.coderef.jsonc`.
- `ifchange` — a coupled-change pattern; not a single resolver but a marker
  pair (`IfChange`/`ThenChange`) with its own grammar and verifier (§10).
- `command` — `target` is a custom command name to dispatch (advanced; the
  extension binds it to a `vscode.commands.executeCommand` call).

Resolvers are pluggable; the v0.1 + v0.2 ship `url`, `local`, `block`,
and `ifchange`. `command` is reserved for v0.3.

**Every shape is verified end-to-end by the hook / CLI**, not only by the
editor. `coderef check` and `coderef check --staged` run the verifier
against whatever `target` evaluates to — `http://go/here`, an absolute
URL, a local path, or a scheme-less form prepended to a URL by a `target`
template. The editor reuses the same resolver + verifier results, so what
fails in CI also fails on hover; what passes in CI is what the
DocumentLink opens.

#### 5.2.1 Polymorphic patterns via multiple definitions

A single keyword (`OPTIMIZE`, `SEE`, `REF`) often needs to accept several
reference shapes — a local file in some cases, a URL in others, an
internal go-link or a ticket id in still others. The idiomatic v0.1
approach is **one pattern per shape**, distinguished by regex, with
`priority` resolving overlaps. Doctor's static checks (§9.1) detect
accidental ambiguity at config load.

```jsonc
{
  "patterns": {
    "optimize-local": {
      "regex":    "OPTIMIZE\\((?<path>/[^)\\s]+)\\)",
      "kind":     "local",
      "target":   "${path}",
      "priority": 100,
      "resolve":  { "root": "${workspaceFolder}",
                    "extensions": [".md"], "indexFiles": ["README.md"] }
    },
    "optimize-url": {
      "regex":    "OPTIMIZE\\((?<url>https?://[^)\\s]+)\\)",
      "kind":     "url",
      "target":   "${url}",
      "priority": 90
    },
    "optimize-go": {
      "regex":    "OPTIMIZE\\((?<short>go/[\\w./-]+)\\)",
      "kind":     "url",
      "target":   "http://${short}",
      "priority": 80
    },
    "optimize-ticket": {
      "regex":    "OPTIMIZE\\((?<ticket>[A-Z][A-Z0-9_]+-\\d+)\\)",
      "kind":     "url",
      "target":   "${config:variables.jiraBase}/browse/${ticket}",
      "priority": 70
    }
  }
}
```

Each pattern's regex anchors the dispatch on the shape of the captured
argument:

| Author writes                   | Matched pattern   | Effective target                      | Verifier behaviour                  |
| ------------------------------- | ----------------- | ------------------------------------- | ----------------------------------- |
| `OPTIMIZE(/docs/perf.md)`       | `optimize-local`  | local file at `<repo>/docs/perf.md`   | filesystem stat                     |
| `OPTIMIZE(http://go/here)`      | `optimize-url`    | `http://go/here`                      | HEAD via internal profile           |
| `OPTIMIZE(https://example.com)` | `optimize-url`    | `https://example.com`                 | HEAD via external profile           |
| `OPTIMIZE(go/here)`             | `optimize-go`     | `http://go/here` (scheme prepended)   | HEAD via internal profile           |
| `OPTIMIZE(PROJ-123)`            | `optimize-ticket` | `<jiraBase>/browse/PROJ-123`          | HEAD via internal profile           |

Higher `priority` wins when two regexes can both match the same input.
All five shapes go through the same hook/CLI verifier and the same
editor open/hover path — the only thing that differs is which dispatch
resolved the input.

**Multiple patterns is the polymorphism story.** An earlier design draft
floated a `kind: "auto"` with nested `dispatch[]` arms inside a single
pattern; it was cut because the "one pattern per shape" idiom is *more*
discoverable, not less — each shape carries its own
`verify`/`preview`/`upgrade`/`severity` as a flat block instead of
buried inside per-arm overrides, and the visual config editor (§14.6)
makes the per-shape blocks visually obvious. We don't plan to revisit
the nested-dispatch shape; if the design's verbose for the multi-shape
case, the visual editor's style templates (§14.6.2) absorb the
verbosity at the UX layer.

### 5.3 Actions

Each pattern can declare three actions; defaults are sensible if omitted:

```jsonc
{
  "patterns": {
    "jira": {
      "regex": "JIRA\\((?<ticket>[A-Z][A-Z0-9_]+-\\d+)\\)",
      "target": "https://jira.example.com/browse/${ticket}",
      "actions": {
        "open":    { "kind": "url" },                                  // default
        "preview": { "kind": "http",
                     "url":     "https://jira.example.com/rest/api/2/issue/${ticket}?fields=summary,status",
                     "headers": { "Accept": "application/json",
                                  "Authorization": "Bearer ${env:JIRA_TOKEN}" },
                     "render":  "**{fields.summary}** — *{fields.status.name}*" },
        "verify":  { "kind": "http",
                     "url":     "https://jira.example.com/rest/api/2/issue/${ticket}",
                     "method":  "HEAD",
                     "acceptStatus": [200] }
      }
    }
  }
}
```

- **`open`** — primary click action (URL → browser, local → editor open).
- **`preview`** — what the hover/quickview shows. Defaults to a snippet for
  `local`, the URL itself for `url`. Configurable: `file` (read the file),
  `http` (fetch + render with a template), or `static` (literal text/markdown).
- **`verify`** — how the CLI checks the reference resolves. Defaults to a
  `HEAD` to the target for `url`, `fs.exists` for `local`. Per-pattern
  `acceptStatus`, `method`, `timeoutMs`, `headers` overrides.

In `preview.render` the `{field.path}` syntax (no `$`) is a response-field
accessor for HTTP previews, drawn from the JSON body — distinct from the
variable system. This keeps the two concepts separable.

`kind: "ifchange"` patterns have their own action structure (§10.3); the
`actions` block above does not apply.

#### 5.3.1 Multiple targets per pattern (v0.3)

One reference often resolves to several places that are all worth surfacing.
The canonical example is a user mention: `@marcus` might link to the
internal homepage *and* an epitaph page *and* an external partner profile —
all useful, with a clear "primary" target most users want by default. Rather
than encoding this as several near-duplicate patterns, a pattern can declare
a **`targets[]`** array. The single `target` string is shorthand for a
one-entry list.

```jsonc
{
  "patterns": {
    "todo-user": {
      "regex":  "TODO\\((?<unverified>\\?)?@(?<user>[\\w.-]+)\\)",
      "title":  "@${user}",
      "targets": [
        { "label": "User home",        "url": "https://company.local/users?${user}",
          "priority": 100, "verify": { "required": true  } },
        { "label": "Epitaph",          "url": "https://company.local/epitaphs?${user}",
          "priority":  50, "verify": { "required": false } },
        { "label": "External profile", "url": "https://company.local/external?${user}",
          "priority":  30, "verify": { "required": false, "profile": "external-only" } },
        { "label": "Partner profile",  "url": "https://external.partner.com/users/${user}",
          "priority":  20, "verify": { "required": false, "profile": "external-only" } }
      ]
    }
  }
}
```

**Target spec**

```ts
type TargetSpec = {
  label?: string;                  // shown in hover & code-action menu
  url: string;                     // URL template; supports ${...}
  priority?: number;               // default 0; higher wins for primary
  verify?: {
    enabled?: boolean;             // default true
    required?: boolean;            // default true for primary, false for others
    profile?: string;              // override network profile per target
    method?: string;
    acceptStatus?: number[];
    timeoutMs?: number;
  };
};
```

**Semantic rules**

- **Primary target** = the entry with the highest `priority`. Ties are broken
  by declaration order. The primary is what `coderef.openReference` opens
  by default; `Cmd/Ctrl-click` follows it; the DocumentLink emitted to
  VSCode points at it.
- **Hover** lists every target, ranked by priority, with its label as the
  link text. Each entry is clickable.
- **Code action** `Open with…` exposes every non-primary target so the user
  can pick. Available from the right-click menu and palette.
- **Verifier:** each target is checked according to its own `verify`
  configuration. A **broken reference** is one where any `verify.required:
  true` target fails OR every target fails (regardless of `required`).
  Failures on non-required alternates are surfaced in the report as
  "alternate unavailable" with severity `info`, not counted toward the
  exit-code failure budget.
- **Caching, profile selection, proxying** apply per-target as if each were
  its own single-target reference.

**Mutual exclusion with `target`**

`target` (string) and `targets` (array) are mutually exclusive on the same
pattern. Doctor (§9) errors if both are set. `target` is preserved as the
zero-friction form for refs that genuinely have only one home; new patterns
that need any of the above features use `targets[]`.

**Default `required` behaviour**

When `verify.required` is omitted on a target, the default is:

- `required: true` for the *primary* target.
- `required: false` for every other target.

Overridable per target. Override globally for a pattern via
`pattern.targetsDefaults.verify.required`.

**Doctor checks for multi-target**

| Check                            | Default severity       | Description                                                                              |
| -------------------------------- | ---------------------- | ---------------------------------------------------------------------------------------- |
| `targets.bothFieldsSet`          | error                  | A pattern sets both `target` and `targets`.                                              |
| `targets.duplicatePriority`      | warning                | Two targets share the same `priority`; order resolution falls back to declaration order. |
| `targets.allFail`                | (per-pattern severity) | At runtime, every target for a given reference fails verification.                       |
| `targets.alternateUnavailable`   | info                   | A non-required alternate target fails — kept visible but not breaking.                   |
| `targets.unreachableLabel`       | warning                | Two targets in the same pattern share a label.                                           |

### 5.4 Scoping the scan

By default `coderef` scans every text file it can read (subject to `ignore`).
Patterns can restrict where they apply:

```jsonc
{
  "patterns": {
    "todo-user": {
      "regex": "TODO\\(@(?<user>[\\w.-]+)\\)",
      "target": "https://users.example.com/${user}",
      "scope": {
        "include":       ["**/*.{ts,tsx,js,jsx,py,go,rs,cpp,h,hpp}"],
        "exclude":       ["**/*.min.*"],
        "commentsOnly":  true,         // see §5.4.1
        "commitMessage": true          // see §5.4.3 — scan commit messages too
      }
    }
  }
}
```

Subsections cover the available scoping knobs in detail:

- §5.4.1 — `commentsOnly` (coarse comment-region filter)
- §5.4.2 — `prefix` (language-aware comment-prefix policy, layered on top)
- §5.4.3 — `commitMessage` (apply pattern to commit-message linting too)

#### 5.4.1 Comments-only scoping

`commentsOnly: true` enables a lightweight tokeniser per language family
(`//`, `#`, `--`, `/* ... */`, `<!-- ... -->`) and ignores matches that fall
outside comment regions. This is intentionally heuristic, not a full parser.
The language table (§7.5) drives detection; languages without an entry fall
through to a permissive default (§5.4.2 *Unknown languages*).

`commentsOnly` is a *coarse-grained* gate: it only filters by detected comment
regions. To additionally require a specific comment *prefix* on the marker's
line, or to require the marker to be on a line by itself, use `prefix` (§5.4.2).
When `prefix.require` is set, it supersedes `commentsOnly` for that pattern.

#### 5.4.2 Comment prefixes and line context

Most reference patterns (TODO markers, `IfChange`/`ThenChange`, doc refs,
JIRA keys) are intended to appear *inside* source-code comments — almost
never in strings, never in identifiers. The `commentsOnly` flag filters by
comment region but does not require the match to follow a comment prefix on
its line, and does not let an `IfChange` block demand "must be on a line by
itself, after the language's comment prefix, with optional `*` decoration
inside a Javadoc block."

`scope.prefix` adds that level of control. It is *language-aware*: the
scanner looks up each file's language (extension or VSCode `languageId`) in
the language table (§7.5) and assembles an effective per-file regex by
prepending the appropriate comment-prefix regex. Authors never write
language-specific prefixes themselves; the system composes them.

```jsonc
{
  "scope": {
    "prefix": {
      "require":  "comment",          // "comment" | "lineComment" | "blockComment" | "any"
      "ownLine":  false,              // marker must be alone on its line (whitespace OK)
      "leadingWhitespace": "any",     // "none" | "any" | <regex>
      "trailingContent":   "any",     // "none" | "any" — text allowed after the match
      "blockComment": {
        "allowMidLine":      false,   // permit /* MARK */ next to code on the same line
        "leadingDecoration": true     // permit " * MARK" inside Javadoc-style blocks
      },
      "onUnknownLanguage":  "lenient" // "lenient" | "strict" | "none"
    }
  }
}
```

**Semantics**

- **`require: "lineComment"`** — the match must appear on a line whose first
  non-whitespace content is the language's *line-comment* token (e.g.
  `//`, `#`, `--`), optionally followed by whitespace, then the match.
- **`require: "blockComment"`** — the match must appear inside an open
  *block-comment* region (e.g. `/* ... */`, `<!-- ... -->`, `(* ... *)`,
  `{- ... -}`). If `ownLine: true`, the marker must be the only content on
  its line within the block; leading whitespace and the Javadoc-style
  `*`-decoration are tolerated when `leadingDecoration: true`.
- **`require: "comment"`** (default) — either line or block comment is fine.
- **`require: "any"`** — no prefix requirement; revert to scanning the raw
  text (subject to `commentsOnly` / `scope.include`).

- **`ownLine: true`** — even when matched on a line that has comment-prefixed
  content, the marker must be the only non-whitespace content on its line
  after the prefix. Recommended for `IfChange`/`ThenChange` (otherwise a
  developer could write `code(); // IfChange(x)` and accidentally bound a
  one-line block).

- **`trailingContent: "any"`** (default) — text after the match is allowed
  (e.g. `# TODO(@marcus): swap in the new KDF`). Set to `"none"` to require
  the marker to be the literal last content on its line.

**Composition**

The effective per-file scan regex is built lazily once per (pattern, language)
pair. Authors do not see these composed regexes — they live in the engine.

| Pattern regex (author writes)        | Language   | Policy                                | Composed (engine)                                       |
| ------------------------------------ | ---------- | ------------------------------------- | ------------------------------------------------------- |
| `IfChange(?:\((?<id>[^)]*)\))?`      | python     | `require: lineComment, ownLine: true` | `^[ \t]*#[ \t]+IfChange(?:\((?<id>[^)]*)\))?[ \t]*$`    |
| `IfChange(?:\((?<id>[^)]*)\))?`      | cpp        | `require: comment, ownLine: true`     | `^[ \t]*(?://[ \t]+                                     |
| `TODO\(@(?<user>[\w.-]+)\)`          | typescript | `require: comment, ownLine: false`    | `(?://[ \t]+                                            |

**Unknown languages**

If a file's language has no entry in the language table:

- `onUnknownLanguage: "lenient"` (default) — fall back to a permissive
  prefix regex that recognises any of `//`, `#`, `--`, `;`, `%`, `/*`,
  `<!--` as candidate prefixes. Useful for catch-all conventions in
  mixed-language repos.
- `"strict"` — the pattern does not run on unknown-language files.
- `"none"` — drop the prefix requirement entirely; match the raw user regex.

Doctor (§9) reports any file that triggered the `lenient` fallback during
the most recent scan so users can add a proper language entry.

**Per-pattern override and global defaults**

A pattern's `scope.prefix` wins over any default. Set a default for the whole
config under `defaults.prefix` (applies to every pattern that doesn't declare
its own):

```jsonc
{
  "defaults": {
    "prefix": { "require": "comment", "ownLine": false }
  },
  "patterns": {
    "todo-user": {
      "regex": "TODO\\(@(?<user>[\\w.-]+)\\)",
      "target": "${...}"
      // inherits defaults.prefix
    },
    "ifchange-default": {
      "kind": "ifchange",
      "ifChange":  { "regex": "IfChange(?:\\((?<id>[^)]*)\\))?" },
      "thenChange": { "regex": "ThenChange(?:\\((?<targets>[^)]*)\\))?" },
      "scope": {
        "prefix": {
          "require": "comment",
          "ownLine": true,                        // stricter for marker pairs
          "blockComment": { "leadingDecoration": true }
        }
      }
    }
  }
}
```

**Relationship to `commentsOnly`**

If a pattern sets both `commentsOnly: true` and `prefix.require`, the prefix
policy takes effect and `commentsOnly` becomes a no-op (doctor warns,
because the flag is redundant). Existing configs that use only
`commentsOnly` continue to work without change; `prefix` is purely additive.

#### 5.4.3 Commit-message scope

The `scope.commitMessage` field controls whether the pattern is applied
to commit-message files when `coderef check --commit-msg` is invoked
(§16.1.1). Three values:

| Value        | Behaviour                                                                                                                                                                                     | Default for `kind`                                                                                     |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `true`       | Pattern scans commit messages just like source files.                                                                                                                                         | `url`, `local` (the common case — JIRA/TODO refs naturally appear in commits)                          |
| `false`      | Pattern does NOT scan commit messages.                                                                                                                                                        | `ifchange`, `command` (block markers and command-dispatchers don't translate to single-message inputs) |
| `"required"` | Every commit message MUST contain at least one match of this pattern; missing matches are reported with `severity.commitMessageMissing` (per-pattern, default `error` when this mode is set). | (never default; opt-in only)                                                                           |

Together with the `--commit-msg` CLI flag (§13.1) and the
`coderef-commit-msg` opt-in pre-commit hook (§16.1.1), this turns the
same pattern engine into a commit-message linter — no separate grammar
or subsystem.

The `severity` keys on a pattern can include `commitMessageMissing`
alongside `broken` (and any other feature-specific keys); see the
`Pattern` schema in §7.3, which now uses the open `Record<string,
Severity>` shape on `severity` so feature subsections can name their
own checks consistently.

### 5.5 Priority

`pattern.priority` (integer, default `0`) determines which pattern wins when
multiple match the same byte range. Higher wins; ties are a conflict (§9).
Priority is intentionally explicit, not derived from declaration order, so
config merges and refactors stay deterministic.

(Pattern priority and per-target priority in §5.3.1 are distinct knobs:
pattern priority resolves "which pattern claims this match," target priority
resolves "which URL does the open action go to.")

### 5.6 Unverified references (v0.2)

Adoption-phase realism: when retrofitting `coderef` onto an existing repo, or
when adding a reference to something that doesn't exist yet, authors want to
write the marker *without* having the verifier fail on it. A
language-agnostic in-line opt-out solves this: a designated prefix character
inside the parentheses marks the reference as **unverified**.

```python
# TODO(?@marcus)               — exists, skip verification for now
# TODO(@marcus)                — verified normally
```

The default prefix is `?`. Authors and the editor both find it obvious; the
character is rare inside usernames and ticket ids, so collisions are
unlikely.

#### 5.6.1 Pattern declaration

A pattern opts in by nominating a regex capture as the unverified marker:

```jsonc
{
  "patterns": {
    "todo-user": {
      "regex":  "TODO\\((?<unverified>\\?)?@(?<user>[\\w.-]+)\\)",
      "targets": [ /* §5.3.1 */ ],
      "unverified": {
        "capture":     "unverified",     // the named capture above
        "maxAge":      "90 days",        // doctor warns if older than this
        "diagnostics": "info"            // editor severity: "off"|"info"|"warning"
      }
    }
  }
}
```

When a match has a **non-empty** `${capture:unverified}`, the engine treats
the match as unverified for the rest of the pipeline. The capture itself is
not required to use any specific character — any pattern can pick its own
syntax — but the `?` convention is recommended for cross-pattern
consistency.

#### 5.6.2 Semantics

- **Verifier:** skip all `verify` calls for unverified matches. The match is
  still indexed, still appears in `coderef list`, and still appears as a
  document link / hover in the editor.
- **Doctor (§9) checks:**
  - `unverified.present` (default `info`) — total count per pattern.
  - `unverified.tooOld` (default `warning`) — reference still marked
    unverified after `unverified.maxAge`, measured via `git blame` of the
    line. Goes red so unverified markers don't become permanent.
  - `unverified.captureMissing` (`error` at config load) — `unverified.capture`
    names a capture not present in the pattern's regex.
- **Editor (VSCode):** unverified refs are rendered with a distinct
  decoration (subtle italic / muted color), and the hover annotation reads
  "unverified — verification skipped." A code action **"Mark verified"** is
  offered, which removes the `?` (and triggers normal verification on next
  scan).
- **`coderef changes` (coupled-change verifier, §10):** an unverified marker
  in an `IfChange` id is *still* tracked for change coherence — unverified
  is about target verification, not block tracking.

#### 5.6.3 Upgrade integration

The upgrade engine (§11) recognises a special tag for "promote to verified":

```jsonc
{
  "patterns": {
    "todo-user": {
      "regex": "TODO\\((?<unverified>\\?)?@(?<user>[\\w.-]+)\\)",
      "upgrade": {
        "rules": [
          // ... earlier rules ...
          { "match":   "TODO\\(\\?@(?<user>[\\w.-]+)\\)",
            "rewrite": "TODO(@${user})",
            "tag":     "verify-now" }
        ]
      }
    }
  }
}
```

`coderef upgrade --tag verify-now --apply` rewrites all unverified refs of
that pattern to their verified form in one sweep. Useful when migrating away
from a batch of legacy unverified markers.

#### 5.6.4 Reporting

```
src/server/auth.ts
  L13  TODO(?@former-employee)         → unverified (skipped)
  L57  TODO(?@aged-marker)             → unverified for 142 days (limit 90)  [warning]

Refs: 142 checked, 3 broken, 0 skipped, 119 cached, 27 unverified
```

### 5.7 Pattern categories (v0.2)

Pattern ids (`todo-user`, `todo-bug`, `jira`, `docref`, ...) are
implementation detail. Authors recognise *visible cues* in the source
— `/path`, `@user`, `PROJ-123`, `RFC(8259)` — and group references in
their heads by those cues, not by config ids. The **category** field
mirrors that mental model and drives the references browser (§14.7),
the visual config editor's style templates (§14.6), and category-aware
doctor checks.

#### 5.7.1 Field

Each pattern declares an optional `category` string. Built-in names and
their conventions:

| Category | Icon | Visible cue | Default secondary grouping in the browser |
| ---------------- | :--: | ----------------------------- | ---------------------------------------- |
| `files` | 📁 | `/docs/x.md`, `/src/auth.ts` | by directory |
| `people` | 👤 | `@marcus`, `@channel`, `@team` | by username |
| `tickets` | 🎫 | `PROJ-123`, `b/12345`, `#42` | by project prefix (regex sub-capture) |
| `standards` | 📜 | `RFC(8259)`, `CVE-2024-1234` | by series / year |
| `urls` | 🔗 | `http://...`, `go/here` | by host |
| `coupled-change` | 🔄 | `IfChange(...)` blocks | by block id |
| `other` | ❓ | fallback | by file |

Categories are open: any user-defined string is allowed (e.g.
`"category": "slack-channels"`, `"category": "rfcs"`, `"category":
"compliance"`). User categories use a default 🏷 icon and sub-group by
file; both can be overridden under `integrity.categories.<name>`.

#### 5.7.2 Defaults and inference

If a pattern omits `category`, it is inferred from `kind`:

| `kind`      | Inferred `category`                                                                 |
| ----------- | ----------------------------------------------------------------------------------- |
| `local`     | `files`                                                                             |
| `ifchange`  | `coupled-change`                                                                    |
| `url`       | `other` — doctor warns: "looks like `people`/`tickets`/`urls`; declare explicitly." |

The visual editor's style templates (§14.6) set the right category by
default — `"User reference (multi-target)"` → `people`, `"Bug / ticket
reference"` → `tickets`, `"Local doc reference"` → `files`, etc. — so
in practice the explicit declaration only matters for free-form
patterns.

#### 5.7.3 Display order

Categories have a designed display order in the references browser
(§14.7), not config-declaration order:

```
📁 Files → 👤 People → 🎫 Tickets → 📜 Standards → 🔗 URLs → 🔄 Coupled-change → (user categories) → ❓ Other
```

Concrete first (files exist on disk), then well-typed external
references (people, tickets, standards), then the looser categories
(URLs are generic, coupled-change is structural, other is catchall).
User-defined categories slot between `coupled-change` and `other` in
alphabetical order; the position is configurable per category via
`integrity.categories.<name>.displayOrder` (integer; lower = earlier).

#### 5.7.4 Doctor checks

| Check                     | Severity | Trigger                                                                                                    |
| ------------------------- | -------- | ---------------------------------------------------------------------------------------------------------- |
| `category.unset`          | info     | `kind: "url"` pattern omits `category`. Doctor guesses from regex shape and suggests.                      |
| `category.mismatch`       | warning  | Captured values consistently start with a sigil (`@`, `/`, `http`) that contradicts the declared category. |
| `category.tooBroadOther`  | info     | More than `integrity.maxOtherPatterns` (default 5) patterns share `category: "other"`.                     |

### 5.8 Per-pattern editor styling (v0.3)

A pattern can declare an optional `style` block that the VSCode
extension applies as a `TextEditorDecorationType` over each match.
Useful when category-level icons (§5.7) aren't enough — e.g. legacy
`TODO`s tinted red, internal-domain refs underlined dotted, deprecated
patterns struck through. Regex Robin does this; we adopt the same
DecorationRenderOptions surface, kept minimal.

```jsonc
{
  "style": {
    "color":           "#cccccc",
    "backgroundColor": "#001122aa",            // 8-digit hex allows alpha
    "fontStyle":       "italic",
    "fontWeight":      "bold",
    "textDecoration":  "underline dotted",
    "border":          "1px solid #555",
    "borderRadius":    "3px",
    "cursor":          "pointer",
    "themeColor":      "editorWarning.foreground"   // VSCode theme color id; overrides `color`
  }
}
```

Fields map 1-1 to `vscode.DecorationRenderOptions`. Recognised keys:
`color`, `backgroundColor`, `fontStyle`, `fontWeight`,
`textDecoration`, `border`, `borderColor`, `borderRadius`,
`borderStyle`, `borderWidth`, `cursor`, `opacity`, `letterSpacing`,
plus the `themeColor` escape hatch for any field that needs a VSCode
workbench color id (`editorError.foreground` etc.). Unknown keys are
rejected at config load.

**Performance.** The extension creates **one decoration type per
pattern**, not one per match — VSCode handles thousands of ranges per
type with negligible cost, but hundreds of types start to slow editor
scroll. Doctor warns when total declared `style`-using patterns
exceeds `integrity.maxStyledPatterns` (default 100).

The CLI ignores `style` entirely; it's an editor-only concern.

Doctor checks: `style.unknownKey` (error), `style.tooManyPatterns`
(warning), `style.themeColorUnknown` (warning — color id not in
VSCode's documented set).

### 5.9 Uniqueness constraints (design only; post-v0.4 backlog)

Some patterns are *labels*, not pointers — they identify a thing, and
the system should reject duplicates. tagref enforces this rule for
`[tag:name]`: every tag must be unique workspace-wide. coderef offers
the same as an opt-in pattern feature, complementary to tagref rather
than a port of it.

```jsonc
{
  "uniqueness": {
    "scope":    "workspace",          // "workspace" | "file"
    "by":       "capture:slug",       // which capture must be unique; "match" for full text
    "ignoreCase": false,
    "severity": "error"
  }
}
```

Semantics: after the scan, the engine groups matches by the resolved
value of `uniqueness.by`. Any group with more than one entry is a
violation; the report lists every offending location.

Example use case: a `SPEC(<slug>)` pattern that names a
specification anchor. Each `<slug>` must appear exactly once across
the workspace.

```jsonc
{
  "patterns": {
    "spec": {
      "regex":      "SPEC\\((?<slug>[a-z][a-z0-9-]+)\\)",
      "category":   "standards",
      "target":     "${workspaceFolder}/spec/${slug}.md",
      "kind":       "local",
      "uniqueness": { "scope": "workspace", "by": "capture:slug",
                      "severity": "error" }
    }
  }
}
```

Doctor checks: `unique.duplicates` (default error) — surfaces every
collision; `unique.captureMissing` (error at load) — `uniqueness.by`
names a capture not present in the regex.

**Relation to tagref.** A `kind: "url"`/`"local"` pattern with
`uniqueness` set behaves like tagref's `[tag:name]`. References to
those tags (the verifier checking that a captured value matches a
declared tag) is the inverse direction — `coderef`'s standard
verification (target file/URL exists) covers it when the unique
pattern produces a local-file target. For pure
intra-codebase tag-ref enforcement without target resolution,
**tagref** remains the recommended tool (see §23.1's compose-don't-port
principle).

---

## 6. Local Path Resolution

`kind: local` patterns produce a workspace-relative path. Resolution applies
*shortcuts* before checking existence, so the reference doesn't have to spell
out the full filename.

```jsonc
{
  "patterns": {
    "docref": {
      "regex": "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
      "kind": "local",
      "target": "${path}",
      "resolve": {
        "root": "${workspaceFolder}",      // search root; variables allowed
        "anchorMode": "workspace",         // "workspace" | "file" | "rootedOrFile"
        "extensions": [".md", ".mdx", ".rst"],
        "indexFiles": ["README.md", "index.md", "README.rst"],
        "caseSensitive": "fs",             // "always" | "never" | "fs"
        "anchor": "${anchor}"              // appended as #anchor on open
      }
    }
  }
}
```

### 6.1 Leading-slash semantics

Authors write paths with a leading `/` to make the workspace root explicit:

- `DOCREF(/docs/architecture)` — unambiguously workspace-rooted.
- `DOCREF(docs/architecture)`  — semantics depend on `resolve.anchorMode`.

`anchorMode` values:

- `"workspace"` (default) — both forms anchor at the workspace root. The
  leading `/` is decorative but encouraged for readability. *This is the
  recommended default and matches the user-facing examples in this doc.*
- `"file"` — paths without leading `/` are resolved relative to the file
  containing the reference. Leading-`/` paths are still workspace-rooted.
- `"rootedOrFile"` — same as `"file"`, but a leading `./` forces file-relative
  even if other settings would workspace-anchor it.

This is independent of the OS: a literal absolute path on disk
(`/Users/marcus/...`) is *never* opened by a local reference unless
`resolve.root` is set to `/`, which is rejected with a load-time error.

### 6.2 Resolution algorithm

Given input `path` (already with leading `/` stripped if present):

1. Compose absolute candidate: `join(resolve.root, path)`.
2. If candidate is a regular file: done.
3. For each extension in `resolve.extensions`: if `candidate + ext` is a file: done.
4. If candidate is a directory: for each name in `resolve.indexFiles`, if it
   exists inside: done.
5. Otherwise: unresolved (error in verifier, broken link in editor).

`caseSensitive: "fs"` means honour the filesystem (case-insensitive on macOS
default + Windows, case-sensitive on Linux). `"always"` and `"never"` force
the behaviour for cross-platform consistency.

Anchor handling is precise for in-repo Markdown (see §6.3); for HTML and
remote URL targets the verifier fetches and parses the page (see §13.3).

### 6.3 Anchor verification (local references)

For in-repo files, `coderef` owns the content and can verify that the
`#anchor` portion of a reference resolves precisely. There is no
opt-out flag: **omitting the anchor in the reference itself is the
opt-out** (`DOCREF(/docs/x.md)` skips anchor checking; only
`DOCREF(/docs/x.md#hashing)` triggers it).

#### 6.3.1 Per-pattern configuration

```jsonc
{
  "resolve": {
    "anchor":       "${anchor}",
    "anchorVerify": "ifPresent",      // "ifPresent" (default) | "always" | "never"
    "slugifier":    "github"           // "github" | "pandoc" | "gitlab" | "hugo" | "mkdocs-material" | { custom: ... }
  }
}
```

| `anchorVerify` | Behaviour                                                                                                               |
| -------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `ifPresent`    | (default) Verify only when the captured `${anchor}` is non-empty. Maps to the "user removes the anchor" opt-out.        |
| `always`       | The anchor is required. A reference without an anchor is broken. For docs that demand specificity.                      |
| `never`        | Ignore anchors entirely. Useful in adoption-phase migrations or when the target is known to render anchors dynamically. |

#### 6.3.2 Slug derivation

`slugifier` controls how headings in the target file are converted into
anchor slugs. The supported algorithms map 1-1 to the major
markdown-rendering platforms, so the algorithm a project picks should
match how its docs are *served*:

| Slugifier          | Example: `## My Heading & v2.0!` →                          |
| ------------------ | ----------------------------------------------------------- |
| `github` (default) | `my-heading--v20`                                           |
| `pandoc`           | `my-heading-v20`                                            |
| `gitlab`           | `my-heading-v2-0`                                           |
| `hugo`             | `my-heading-v2-0`                                           |
| `mkdocs-material`  | `my-heading-v20`                                            |

Pandoc-style explicit IDs (`## My Heading {#explicit-id}`) are always
honoured ahead of the derived slug, matching lychee's behaviour. Custom
slugifiers may be declared inline:

```jsonc
{
  "slugifier": {
    "custom": [
      { "lowercase": true },
      { "replace": { "from": "[^a-z0-9]+", "to": "-" } },
      { "trim": "-" }
    ]
  }
}
```

#### 6.3.3 Algorithm

Given a resolved local target file and a captured `${anchor}`:

1. If `anchorVerify` is `never`, or the captured anchor is empty under
   `ifPresent`, return *resolved*.
2. Read the target file (cache-keyed by `mtime`).
3. Parse with `pulldown-cmark` (or `comrak` for GitHub-flavoured
   markdown by default).
4. Walk all `Heading` events and any explicit `{#id}` attributes;
   derive the slug set via the configured `slugifier`.
5. Test the captured anchor against the slug set, case-sensitive.
6. If absent, the reference is **broken with severity from the pattern**,
   and the report includes a Levenshtein-1 suggestion when one exists
   ("did you mean `#hashing`?").

For non-Markdown targets (`.html`, `.adoc`, `.rst`), the same algorithm
applies with format-specific parsers; the slugifier table extends in
v0.2 (open question §21). For unknown extensions, anchor verification
is skipped with an `info` diagnostic so the file resolves but the
anchor doesn't gate the run.

#### 6.3.4 Doctor checks

| Check                      | Severity                      | Trigger                                                                                                      |
| -------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `anchor.unknown`           | per-pattern `severity.broken` | Captured anchor absent from target's slug set.                                                               |
| `anchor.styleMismatch`     | warning                       | Target contains Pandoc-style `{#id}` attributes but pattern's slugifier is `github` (or similar mismatches). |
| `anchor.skippedExt`        | info                          | Target's extension has no anchor parser; anchor checking was skipped.                                        |

#### 6.3.5 Why precise local checking matters

No surveyed OSS link checker validates anchors against the *actual
parsed headings* of in-repo Markdown:

| Tool                | Anchor support                                     |
| ------------------- | -------------------------------------------------- |
| lychee              | External URLs only (`--include-fragments`), opt-in |
| muffet              | External URLs, default on                          |
| htmltest            | Internal HTML only (`CheckInternalHash`)           |
| markdown-link-check | None (open issue since 2018)                       |
| linkchecker         | External URLs only, opt-in                         |

For docs-heavy projects, this is the gap that bites most often:
heading renamed, the link still resolves to the file, the section is
gone, broken reading experience. `coderef`'s `kind: "local"` resolver
closes it.

### 6.4 Submodule pass-through (v0.4)

A repo with `git submodule`-checked-out dependencies (vendored libs,
bazel `external/`, monorepo subprojects) has the dependency content
already on disk. `coderef` can simply treat each submodule as more
workspace — the scanner walks in, the diff parser sees changes inside,
coupled-change can span the boundary — without any fetch, auth, or
shadow-manifest machinery. Submodules are the *one* cross-repo
mechanism we support; see §23 for the deliberate rejection of
linked-repo manifests.

```jsonc
{
  "submodules": {
    "follow":              true,           // example value; default is false (opt-in)
    "perSubmoduleConfig":  "use-parent"    // "use-parent" | "discover" | "ignore"
  }
}
```

| Setting                            | Effect                                                                                                                                                                                              |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `follow: true`                     | The scanner walks into submodule directories; targets like `/vendor/auth-lib/src/hash.py:42-80` resolve through the submodule's checked-out tree.                                                   |
| `perSubmoduleConfig: "use-parent"` | (default) The parent's patterns/categories/profiles apply uniformly across the workspace. One config rules all.                                                                                     |
| `perSubmoduleConfig: "discover"`   | Each submodule's own `.coderef.jsonc` is loaded for matches *inside* that submodule; profile and integrity settings stay parent-scoped (preserving the global block table and verification policy). |
| `perSubmoduleConfig: "ignore"`     | The submodule is not scanned at all — useful for vendored deps with their own toolchain.                                                                                                            |

**Coupled-change across submodule boundaries.** Works the same as
within one repo: the scanner has all files, the diff overlay sees
changes on both sides, an `IfChange` in the parent can reference a
range inside a submodule and vice versa.

**Limitation worth knowing.** The pre-commit hook runs in the
containing repo's commit context. If a coupled change touches both the
parent and a submodule but the submodule's commit hash isn't bumped in
the parent commit, doctor surfaces `submodule.unsynchronizedBlock`
(warning by default; configurable). This is a real and common gotcha,
not a defect — git's submodule model means "the parent commit pins a
submodule sha"; an uncommitted change inside a submodule isn't visible
to the parent until that bump happens.

**Doctor checks specific to submodules:**

| Check                              | Severity | Trigger                                                                                                         |
| ---------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------- |
| `submodule.followNotRegistered`    | warning  | `submodules.follow: true` but the workspace has no `git submodule` entries.                                     |
| `submodule.unsynchronizedBlock`    | warning  | A coupled-change block spans a submodule boundary; the submodule sha in the parent index hasn't been bumped.    |
| `submodule.configMissing`          | info     | `perSubmoduleConfig: "discover"` is set but a submodule has no `.coderef.jsonc`. Falls back to parent's config. |
| `submodule.crossesScopeIgnore`     | warning  | A target points into a submodule with `"ignore"` policy — likely a config gap.                                  |

---

## 7. Configuration File

### 7.1 Discovery order

The same order applies to both the CLI and the extension:

1. Explicit override:
   - CLI: `--config <path>` flag, or `$CODEREF_CONFIG` env var.
   - Extension: `coderef.configFile` setting (supports `${workspaceFolder}`).
2. `<workspace>/.config/coderef.jsonc`
3. `<workspace>/.config/coderef.json`
4. `<workspace>/.coderef.jsonc`
5. `<workspace>/.coderef.json`

The first existing path wins. The chosen path is logged on startup (CLI to
stderr, extension to its output channel).

Workspace root is detected by walking up from the file/CWD looking for `.git/`
or `.hg/`; overridable via `coderef.workspaceRoot` or `--workspace`.

### 7.2 Format

JSONC (JSON with `//` and `/* */` comments and trailing commas). Schema URL
in `$schema` is recommended:

```jsonc
{
  "$schema": "https://helly25.github.io/coderef/schema/v1.json",
  "variables":        { /* user-defined values, see §8.3 */ },
  "defaults":         { /* §5.4.2 — config-wide defaults applied when a pattern omits its own */ },
  "patterns":         { /* see §5, §10 */ },
  "languages":        { /* §7.5 user overrides for the built-in language table */ },
  "blame":            { /* §11.4 user-mapping for git-blame author lookups */ },
  "verification":     { /* see §13 */ },
  "networkProfiles":  { /* see §12 */ },
  "profileSelection": { /* see §12.3 */ },
  "submodules":       { /* §6.4 — git-submodule pass-through, v0.4 */ },
  "concurrency":      { /* §11.10 — write-mode lock + git-clean policy */ },
  "integrity":        { /* see §9 — how strict the doctor is */ },
  "ignore": ["**/node_modules/**", "**/dist/**", "**/.git/**"]
}
```

### 7.3 Top-level schema (informative)

The shape below is an **informative subset** of the full JSON Schema
shipped in `schema/coderef.schema.json` (draft 2020-12). It exists to
orient readers; the JSON Schema is the authoritative reference. Fields
whose semantics depend on a feature still landing in v0.2+ are marked
with the relevant section reference.

```ts
type Config = {
  $schema?: string;
  variables?: Variables;                  // §8
  defaults?: ConfigDefaults;              // §5.4.2 — { prefix?: PrefixPolicy; ... }
  patterns: Record<string, Pattern>;
  languages?: Record<string, LanguageDef>; // §7.5
  blame?: BlameConfig;                    // §11.4
  verification?: VerificationDefaults;
  networkProfiles?: Record<string, NetworkProfile>;
  profileSelection?: ProfileSelection;
  submodules?: SubmodulesConfig;          // §6.4 — { follow?: boolean; perSubmoduleConfig?: ... }
  concurrency?: ConcurrencyConfig;        // §11.10 — { lockPath; lockTimeoutMs; allowDirty }
  integrity?: IntegrityConfig;
  ignore?: string[];                      // gitignore-style globs, applied repo-wide
  workspaceRoot?: string;                 // override; supports variables
};

type Severity = "error" | "warning" | "info" | "hint" | "off";

type IntegrityConfig = {
  onConflict?: "error" | "warn" | "first-wins";         // §9.2
  maxStyledPatterns?: number;                           // §5.8 — default 100
  maxOtherPatterns?: number;                            // §5.7.4 — default 5
  categories?: Record<string, CategoryOverride>;        // §5.7.3
  coupled?: { maxAllGlob?: number };                    // §10.9
  checks?: Record<string, Severity>;                    // §9.1 — keys are check names; values override default severity
};

type CategoryOverride = {
  icon?: string;
  displayOrder?: number;
  defaultSubGrouping?: string;
};

type Pattern =
  | UrlPattern
  | LocalPattern
  | IfChangePattern                       // see §10
  | CommandPattern;

type UrlPattern = {
  kind?: "url";                           // default
  regex: string;                          // ECMAScript-compatible flavour
  flags?: string;                         // default "g" (always added if absent)
  target?: string;                        // single-target shorthand
  targets?: TargetSpec[];                 // multi-target form (§5.3.1)
  targetsDefaults?: Partial<TargetSpec>;  // §5.3.1 — shared defaults merged into each entry of targets[]
  canonicalForm?: string;                 // §11.5 — explicit canonical form for cross-pattern resolution; "JIRA(${ticket})"
  title?: string;
  priority?: number;
  category?: string;                      // §5.7
  style?: StyleSpec;                      // §5.8 — VSCode DecorationRenderOptions subset
  uniqueness?: UniquenessSpec;            // §5.9
  scope?: ScopeConfig;
  actions?: { open?: ActionConfig; preview?: ActionConfig; verify?: ActionConfig };
  verify?: VerifyToggle;
  upgrade?: UpgradeConfig;                // §11
  unverified?: UnverifiedConfig;          // §5.6
  checksums?: { trackByDefault?: boolean }; // §10.14 — opt every range-target in this pattern into tracking
  severity?: { broken?: Severity; [k: string]: Severity };
};

type LocalPattern = UrlPattern & {
  kind: "local";
  resolve?: LocalResolveConfig;
};

// IfChangePattern is detailed in §10.3.
```

The full JSON Schema lives in `schema/coderef.schema.json` (draft 2020-12)
and ships with the npm wrapper so editors that respect `$schema` (VSCode
included) get autocomplete.

**Generation, not maintenance.** From v0.1 onward, the schema is
*derived* from `coderef-core`'s Rust config types via the
[`schemars`](https://docs.rs/schemars/) crate. The committed
`schema/coderef.schema.json` is the output of a `cargo run --bin
gen-schema` step that runs as part of the build; CI re-runs the
generator and `git diff --exit-code`s the result, failing if the
schema in the tree drifts from what the Rust types say. This
eliminates "schema disagrees with the engine" as a class of bug — the
Rust types are the only authoring surface, the JSON Schema is a build
artefact.

The hand-authored v0.0.0 schema currently in the tree is a placeholder
intended to be regenerated the first time real config types land.

### 7.4 Pointing the hook at a non-standard config

Pre-commit users whose config lives outside the default search path pass
`--config`:

```yaml
repos:
  - repo: https://github.com/helly25/coderef
    rev: v0.1.0
    hooks:
      - id: coderef-check
        args: ["--config", "tools/coderef.jsonc"]
```

Same flag for `lefthook`/`husky` invocations.

### 7.5 Language definitions (v0.2)

The prefix policy (§5.4.2) needs to know each file's comment syntax.
`coderef` ships a built-in language table covering the common cases; users
can extend or override it under the top-level `languages` block.

```jsonc
{
  "languages": {
    // === C family — all extend the "c" baseline ===
    "c":            { "extensions": [".c", ".h"],
                       "lineComment": "//", "blockComment": ["/*", "*/"] },
    "cpp":          { "extensions": [".cpp", ".cc", ".cxx", ".hpp", ".hh", ".hxx"], "extends": "c" },
    "objective-c":  { "extensions": [".m", ".mm"],            "extends": "c" },
    "java":         { "extensions": [".java"],                "extends": "c" },
    "javascript":   { "extensions": [".js", ".mjs", ".cjs"],  "extends": "c" },
    "typescript":   { "extensions": [".ts", ".tsx"],          "extends": "c" },
    "go":           { "extensions": [".go"],                  "extends": "c" },
    "rust":         { "extensions": [".rs"],                  "extends": "c" },
    "kotlin":       { "extensions": [".kt", ".kts"],          "extends": "c" },
    "swift":        { "extensions": [".swift"],               "extends": "c" },
    "scala":        { "extensions": [".scala"],               "extends": "c" },
    "csharp":       { "extensions": [".cs"],                  "extends": "c" },
    "dart":         { "extensions": [".dart"],                "extends": "c" },
    "fsharp":       { "extensions": [".fs", ".fsi", ".fsx"],  "lineComment": "//", "blockComment": ["(*", "*)"] },
    "groovy":       { "extensions": [".groovy", ".gradle"],   "extends": "c" },
    "zig":          { "extensions": [".zig"],                 "lineComment": "//" },
    "v":            { "extensions": [".v", ".vsh"],           "extends": "c" },
    "protobuf":     { "extensions": [".proto"],               "extends": "c" },

    // === Hash-comment family ===
    "python":       { "extensions": [".py", ".pyi"],          "lineComment": "#" },
    "ruby":         { "extensions": [".rb"],                  "lineComment": "#",
                       "blockComment": ["=begin", "=end"] },
    "shell":        { "extensions": [".sh", ".bash", ".zsh", ".fish"], "lineComment": "#" },
    "yaml":         { "extensions": [".yaml", ".yml"],        "lineComment": "#" },
    "toml":         { "extensions": [".toml"],                "lineComment": "#" },
    "ini":          { "extensions": [".ini", ".cfg"],         "lineComment": ["#", ";"] },
    "elixir":       { "extensions": [".ex", ".exs"],          "lineComment": "#" },
    "r":            { "extensions": [".r", ".R"],             "lineComment": "#" },
    "starlark":     { "extensions": [".bzl", ".star"],
                       "filenames":  ["BUILD", "BUILD.bazel", "WORKSPACE", "MODULE.bazel"],
                       "lineComment": "#" },
    "make":         { "extensions": [".mk"],
                       "filenames":  ["Makefile", "GNUmakefile", "makefile"],
                       "lineComment": "#" },
    "perl":         { "extensions": [".pl", ".pm", ".t"],     "lineComment": "#",
                       "blockComment": ["=pod", "=cut"] },
    "graphql":      { "extensions": [".graphql", ".gql"],     "lineComment": "#" },
    "julia":        { "extensions": [".jl"],                  "lineComment": "#",
                       "blockComment": ["#=", "=#"] },
    "nim":          { "extensions": [".nim", ".nims"],        "lineComment": "#",
                       "blockComment": ["#[", "]#"] },
    "tcl":          { "extensions": [".tcl"],                 "lineComment": "#" },
    "powershell":   { "extensions": [".ps1", ".psm1"],        "lineComment": "#",
                       "blockComment": ["<#", "#>"] },
    "terraform":    { "extensions": [".tf", ".tfvars", ".hcl"],
                       "lineComment": ["#", "//"], "blockComment": ["/*", "*/"] },
    "dockerfile":   { "filenames":  ["Dockerfile", "Containerfile"],
                       "extensions": [".dockerfile"],
                       "lineComment": "#" },

    // === Dash / other line-comment families ===
    "sql":          { "extensions": [".sql"],                 "lineComment": "--",
                       "blockComment": ["/*", "*/"] },
    "haskell":      { "extensions": [".hs"],                  "lineComment": "--",
                       "blockComment": ["{-", "-}"] },
    "lua":          { "extensions": [".lua"],                 "lineComment": "--",
                       "blockComment": ["--[[", "]]"] },
    "ocaml":        { "extensions": [".ml", ".mli"],          "blockComment": ["(*", "*)"] },
    "erlang":       { "extensions": [".erl"],                 "lineComment": "%" },
    "lisp":         { "extensions": [".el", ".lisp", ".clj", ".cljs"], "lineComment": ";" },
    "scheme":       { "extensions": [".scm", ".ss"],          "lineComment": ";",
                       "blockComment": ["#|", "|#"] },
    "matlab":       { "extensions": [".m"],                   "lineComment": "%",
                       "blockComment": ["%{", "%}"] },
    "fortran":      { "extensions": [".f", ".f90", ".f95"],   "lineComment": "!" },
    "vim":          { "extensions": [".vim"],
                       "filenames":  [".vimrc"],
                       "lineComment": "\"" },

    // === Block-only families ===
    "html":         { "extensions": [".html", ".htm"],        "blockComment": ["<!--", "-->"] },
    "xml":          { "extensions": [".xml", ".xsd", ".xsl"], "extends": "html" },
    "markdown":     { "extensions": [".md", ".mdx"],          "blockComment": ["<!--", "-->"] },
    "css":          { "extensions": [".css", ".scss", ".sass", ".less"],
                                                              "blockComment": ["/*", "*/"] }
  }
}
```

**Schema**

```ts
type LanguageDef = {
  extensions?: string[];       // case-insensitive on Windows/macOS, exact on Linux
  filenames?: string[];        // exact basename matches (e.g. "Makefile", "BUILD")
  shebangs?: string[];         // regexes to match the first line of the file
  lineComment?: string | string[];
  blockComment?: [string, string];   // [open, close]
  extends?: string;            // another language id to inherit from
};
```

A language entry without `lineComment` simply doesn't support a line-comment
prefix policy (e.g. HTML); same for `blockComment`. A language without
either is illegal (config error). `extends` performs a shallow merge with
the parent, child values winning.

**Detection order** (first match wins):

1. VSCode's `languageId` if running inside the extension.
2. Exact filename match (`filenames`).
3. Shebang line match (`shebangs`, regex against the file's first line).
4. Extension match (`extensions`, case-insensitive on case-insensitive
   filesystems).
5. The `unknown` language entry, if defined.

**User overrides** are merged into the built-in table by id. User entries win
on conflict. Setting an entry to `null` deletes the built-in (rare).

```jsonc
{
  "languages": {
    "starlark":  { "extensions": [".bzl", ".star"],
                    "filenames": ["BUILD", "WORKSPACE", "MODULE.bazel"],
                    "lineComment": "#" },
    "jsonc":     { "extensions": [".jsonc"],          "lineComment": "//",
                    "blockComment": ["/*", "*/"] },
    "java":      null                                  // disable built-in (rare)
  }
}
```

**Edge cases**

- **JSX/TSX block comments inside JSX** (`{/* ... */}`) — the inner `/* */`
  is recognised; the `{` `}` braces are ignored by the prefix matcher.
  Authors may need `prefix.blockComment.allowMidLine: true` for JSX-style
  markers that share a line with rendered tags.
- **Markdown with fenced code blocks** — the prefix is markdown's `<!-- -->`
  even inside a fenced code block. Detecting the embedded language inside
  fenced blocks is deferred (v0.3); for now, code-block markers are treated
  by the host file's language (markdown).
- **Mixed-language files** (`.vue`, `.svelte`, `.astro`) — single-language
  rules apply. v0.3 may add region-aware detection.
- **Shebang-overrides** — `#!/usr/bin/env python` makes a `.txt` file scan
  as Python if shebang detection is enabled (default on for executables).

### 7.6 Multi-config monorepos via `extends:` (v0.4)

A monorepo with multiple services often wants *different* coderef
config per subtree — the API service points its `JIRA` pattern at one
project key, the data service at another; the docs subtree uses local
`DOCREF` shortcuts the rest of the monorepo doesn't need. v0.1–v0.3
support one config per repo; v0.4 adds an `extends:` mechanism that
keeps the workspace-rooted config as the *base* and lets subdirectories
override specific blocks.

```jsonc
// /services/data/.coderef.jsonc
{
  "$schema": "../../schema/coderef.schema.json",
  "extends": "../../.coderef.jsonc",
  "variables": {
    "jiraProject": "DATA"      // overrides the workspace default
  },
  "patterns": {
    "data-internal": {
      "regex":  "DATAREF\\((?<id>[A-Z0-9_]+)\\)",
      "target": "${config:variables.dataBase}/${id}",
      "category": "tickets"
    }
  }
}
```

#### 7.6.1 Discovery

When the scanner enters a directory, it walks *upward* looking for the
nearest `.coderef.jsonc` (or `.config/coderef.jsonc`). If that file
contains `extends`, the parent config is loaded recursively until a
config with no `extends` is found (the *base*). The scanner caches the
resolved chain per directory.

The workspace-root config is always loadable; if a subdirectory has
no own config, it inherits the workspace root's by default.

#### 7.6.2 Merge semantics

| Block                  | Merge strategy                                                                                                |
| ---------------------- | ------------------------------------------------------------------------------------------------------------- |
| `variables`            | Shallow merge by key; child wins. References to `${config:variables.x}` resolve through the merged set.       |
| `defaults`             | Deep merge of `prefix` etc.                                                                                   |
| `patterns`             | Shallow merge by pattern id; child entries replace parent entries with the same id.                           |
| `languages`            | Shallow merge by language id; child entries replace parent entries.                                           |
| `blame`                | Deep merge: `userMapping` and `ignoreAuthors` concatenate (child entries take precedence on duplicate keys).  |
| `verification`         | Deep merge.                                                                                                   |
| `networkProfiles`      | Shallow merge by profile name.                                                                                |
| `profileSelection`     | Child replaces parent if present.                                                                             |
| `submodules`           | Child replaces parent if present.                                                                             |
| `concurrency`          | Single workspace-level lock; child `lockPath` ignored — concurrency is workspace-wide, not per-subdirectory.  |
| `integrity`            | Shallow merge by key; `checks.*` deep-merged.                                                                 |
| `ignore`               | Concatenate parent + child globs.                                                                             |

References that match a pattern declared in the *child* config but the
file lives in a *grand-child* directory: resolved via the child's
config (the nearest config in the walk-up).

#### 7.6.3 Doctor checks

| Check                       | Severity | Trigger                                                                                                |
| --------------------------- | -------- | ------------------------------------------------------------------------------------------------------ |
| `extends.cyclic`            | error    | The `extends` chain forms a cycle.                                                                     |
| `extends.unresolved`        | error    | The referenced file does not exist.                                                                    |
| `extends.outsideWorkspace`  | warning  | The referenced file is outside `${workspaceFolder}`; allowed but flagged as a portability concern.     |
| `extends.shadowedPattern`   | info     | A child config redeclares a pattern id from the parent — usually intentional, surfaced for visibility. |

#### 7.6.4 Limitations

- The `extends` chain is *resolved at config load*, not lazily. A
  subdirectory config that extends a sibling's config will load both
  even if no file in the subdirectory ever matches the sibling's
  patterns.
- Doctor runs **once per resolved config**, not once per directory.
  Overlapping pattern definitions across two unrelated child configs
  are not cross-checked. The reasoning: those configs serve different
  subtrees by design.
- The `extends` value is a *workspace-relative or relative* path, not
  a URL. Remote inheritance ("extends from a package") is not supported
  and is unlikely to be — it leads to the same shadow-manifest failure
  modes §23.1 rejects for repo coupling.

---

## 8. Variable System

References, targets, headers, paths, profile URLs and cache locations all need
substitution. Because `coderef` runs in multiple environments — VSCode, future
LSP-based editors, the CLI in `pre-commit`, and CI — every variable must be
defined in a single place and resolvable identically by all of them. We do not
inherit VSCode's `${workspaceFolder}` etc. directly because the CLI has no host
to ask. Both hosts share one implementation in `coderef-core`.

### 8.1 Syntax

`${name}` and `${namespace:argument}`. The braces are required (bare `$name`
is treated as literal text). This matches VSCode's variable syntax users
already know and avoids collision with the named-capture syntax `(?<name>...)`
inside regexes.

Escape: `$${name}` produces the literal `${name}`.

### 8.2 Namespaces

| Namespace        | Example                            | Source                                                                | Available in |
| ---------------- | ---------------------------------- | --------------------------------------------------------------------- | ------------ |
| (none, builtin)  | `${workspaceFolder}`               | Workspace/repo root.                                                  | everywhere   |
| `capture:`       | `${capture:user}`                  | Named capture from the pattern's regex match.                         | per-ref ctx  |
| (bare → capture) | `${user}`                          | Shortcut for `${capture:user}` if `user` is a capture, else builtin   | per-ref ctx  |
| `env:`           | `${env:JIRA_TOKEN}`                | Process env at resolution time. Empty if unset (or fail; see §8.5).   | CLI + editor |
| `git:`           | `${git:branch}`, `${git:sha}`      | Computed via `git` at resolution time. Cached per invocation.         | CLI + editor |
| `config:`        | `${config:variables.companyHost}`  | User-defined values from the `variables` block.                       | everywhere   |
| `file:`          | `${file:relativePath}`             | The file being scanned (or active editor file).                       | per-ref ctx  |
| `ref:`           | `${ref:line}`, `${ref:column}`     | The reference's match location.                                       | per-ref ctx  |
| `ide:`           | `${ide:name}` (`vscode`/`cli`/...) | Identity of the host; lets configs branch carefully.                  | everywhere   |
| `blame:`         | `${blame:user}`, `${blame:email}`  | `git blame` for the current line; mapped through `blame.userMapping`. | upgrade only |

#### 8.2.1 Builtin variables

| Name                          | Value                                                                       |
| ----------------------------- | --------------------------------------------------------------------------- |
| `${workspaceFolder}`          | Absolute path to the workspace root.                                        |
| `${workspaceRoot}`            | Alias of `${workspaceFolder}` (compat with older VSCode configs).           |
| `${workspaceFolderBasename}`  | Last segment of the workspace path.                                         |
| `${workspaceUri}`             | `file://` URI of the workspace.                                             |
| `${pathSeparator}` / `${/}`   | Platform path separator.                                                    |
| `${homeDir}`                  | User home directory.                                                        |

#### 8.2.2 `file:` namespace

| Name                                 | Value                                                          |
| ------------------------------------ | -------------------------------------------------------------- |
| `${file:absolutePath}`               | Absolute path of the file being scanned.                       |
| `${file:relativePath}`               | Path relative to `${workspaceFolder}`.                         |
| `${file:dirname}`                    | Absolute directory containing the file.                        |
| `${file:relativeDirname}`            | Directory relative to workspace.                               |
| `${file:basename}`                   | `foo.ts`.                                                      |
| `${file:basenameNoExtension}`        | `foo`.                                                         |
| `${file:extension}`                  | `.ts`.                                                         |

These map 1-1 to VSCode's predefined variables; in the CLI they are derived
from the scan loop, which has the file at hand. Outside a per-file resolution
context (e.g. profile selection), they are unavailable and produce an error.

#### 8.2.3 `ref:` namespace

| Name             | Value                                                              |
| ---------------- | ------------------------------------------------------------------ |
| `${ref:line}`    | 1-based line number of the match start.                            |
| `${ref:column}`  | 1-based column of the match start.                                 |
| `${ref:match}`   | The full matched text.                                             |
| `${ref:offset}`  | 0-based byte offset of the match start in the file.                |

#### 8.2.4 `blame:` namespace

Resolved by running `git blame -L <line>,<line> --porcelain -- <file>` for the
current reference. Available **only** inside `upgrade.rules.*.rewrite`
(§11.2). Using `${blame:*}` in `target`, `verify.url`, or any other field is
a config-load error.

| Name                       | Source                                                            |
| -------------------------- | ----------------------------------------------------------------- |
| `${blame:user}`            | The mapped project username (see §11.4).                          |
| `${blame:name}`            | The git author name (e.g. `Marcus Boerger`).                      |
| `${blame:email}`           | The git author email.                                             |
| `${blame:sha}`             | The commit SHA.                                                   |
| `${blame:shortSha}`        | First 8 chars of the commit SHA.                                  |
| `${blame:date}`            | The author date (ISO 8601).                                       |
| `${blame:committerEmail}`  | The committer email (vs author email — different for some flows). |

Bot authors listed in `blame.ignoreAuthors` produce a "missing" result and
trigger `blame.fallback` (§11.4).

### 8.3 User-defined variables

The config can declare its own variables under top-level `variables`:

```jsonc
{
  "variables": {
    "companyHost": "example.com",
    "usersBase":   "https://users.${config:variables.companyHost}",
    "jiraBase":    "https://jira.${config:variables.companyHost}"
  },
  "patterns": {
    "todo-user": {
      "regex":  "TODO\\(@(?<user>[\\w.-]+)\\)",
      "target": "${config:variables.usersBase}/${user}"
    }
  }
}
```

User-defined variables are resolved recursively at config load. Cycles are a
load-time error.

### 8.4 Where variables can appear

| Field                                | Allowed namespaces                                                 |
| ------------------------------------ | ------------------------------------------------------------------ |
| `pattern.target`                     | all except `blame:`                                                |
| `pattern.title`                      | all except `blame:`                                                |
| `pattern.resolve.root`               | builtin + `config:` + `env:` + `git:` (NOT capture/file/ref/blame) |
| `pattern.resolve.anchor`             | all except `blame:`                                                |
| `pattern.actions.preview.url`        | all except `blame:`                                                |
| `pattern.actions.preview.headers.*`  | builtin + `env:` + `config:` + `git:`                              |
| `pattern.actions.preview.render`     | response-fields only (`{fields.summary}` etc.) — not vars          |
| `pattern.actions.verify.url`         | all except `blame:`                                                |
| `pattern.actions.verify.headers.*`   | same as preview headers                                            |
| `pattern.upgrade.rules.*.rewrite`    | all including `blame:` and `capture:`                              |
| `ifchange.thenChange.targets` items  | builtin + `config:` + `env:` + `capture:` (see §10.2)              |
| `verification.cache.path`            | builtin + `env:` + `config:`                                       |
| `networkProfiles.*`                  | builtin + `env:` + `config:`                                       |
| `profileSelection.canary.url`        | builtin + `env:` + `config:`                                       |

Using a disallowed namespace is a load-time error.

### 8.5 Missing values and strict mode

By default, unresolved variables produce an error at the relevant action time.
Per-namespace `defaults` may provide fallbacks:

```jsonc
{
  "variables": {
    "defaults": {
      "env:JIRA_TOKEN": "",            // empty if unset; auth header omitted
      "git:branch":     "HEAD"
    }
  }
}
```

A header whose interpolation produces an empty `Bearer ` is automatically
dropped rather than sent.

Strict mode (`--strict` CLI flag, `coderef.strict` setting) treats every
missing required variable as an error, even those with declared defaults.

### 8.6 Resolution order and caching

1. Static (`builtin`, `config:`, `ide:`) variables resolved at config load,
   **after merging the active network profile's `variables` override**
   (§12.2.1) onto the top-level `variables` table. Switching profile
   re-runs this step (lazy; only on next access).
2. Per-invocation values (`git:`) resolved once per CLI run / once per
   extension session and refreshed on `coderef.reloadConfig`.
3. Per-file values (`file:`) resolved per file scanned.
4. Per-reference values (`capture:`, `ref:`) resolved per reference.
5. Per-line values (`blame:`) resolved on demand during `coderef upgrade`,
   memoised by `(file, line)`.

The variable resolver is the single chokepoint that both the editor and CLI
call; this keeps behaviour identical across hosts.

---

## 9. Pattern System Integrity

A regex-driven system without overlap detection is a footgun. Two patterns
that both match `TODO(b/123)` — for example `TODO\(b/(?<id>\d+)\)` and
`TODO\((?<user>[^)]+)\)` — will silently fight for it. `coderef` treats the
pattern set itself as something that must validate, with two complementary
mechanisms: static checks at load and runtime detection during scans.

### 9.1 Static checks (config-load and `coderef doctor`)

Run on every config load and as `coderef doctor`. Fast and cheap; they can be
strict because they don't depend on the corpus.

| Check                          | Behaviour                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Duplicate names**            | Two patterns with the same `id`. Error.                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **Regex compile**              | Each `regex` compiles under the configured engine (`regex` or `fancy-regex`). Error on failure.                                                                                                                                                                                                                                                                                                                                                                |
| **Missing variables**          | All `${...}` references resolve against the captures + declared namespaces. Error.                                                                                                                                                                                                                                                                                                                                                                             |
| **Unused captures**            | A named capture that is referenced nowhere (target/preview/verify/upgrade) is a *warning*.                                                                                                                                                                                                                                                                                                                                                                     |
| **Greedy captures**            | A capture with no lower bound that adjoins another pattern's prefix (string-static analysis on regex literals). Warning.                                                                                                                                                                                                                                                                                                                                       |
| **Shared literal prefix**      | When two patterns share a long literal prefix (e.g. both start with `TODO\(`), `coderef` runs the synthetic-match check below.                                                                                                                                                                                                                                                                                                                                 |
| **Synthetic-match check**      | For each pattern A in a prefix-overlap group, generate a representative input from A's regex (using a derivative-based string builder, see §9.4) and test every other pattern in the group against it. Report conflicts.                                                                                                                                                                                                                                       |
| **Anchor-mode coherence**      | `kind: "local"` patterns whose capture can produce a string starting with `/Users/...`, `C:\`, etc. without `resolve.root` set are warned.                                                                                                                                                                                                                                                                                                                     |
| **Disallowed variable scope**  | E.g. `${capture:foo}` used in `networkProfiles.*`, or `${blame:user}` outside `upgrade.rules.*.rewrite`. Error.                                                                                                                                                                                                                                                                                                                                                |
| **Coupled-change integrity**   | See §10.9. Orphan markers, unresolved targets, solo ids, block overlap.                                                                                                                                                                                                                                                                                                                                                                                        |
| **Upgrade integrity**          | See §11.8. Unreachable rules, invalid rewrites that don't match the canonical pattern, missing blame mapping.                                                                                                                                                                                                                                                                                                                                                  |
| **Multi-target integrity**     | See §5.3.1. Both `target` and `targets` set; duplicate priorities; all-fail runtime; duplicate labels.                                                                                                                                                                                                                                                                                                                                                         |
| **Unverified integrity**       | See §5.6.2. `unverified.capture` names a missing capture; references unverified for longer than `maxAge`.                                                                                                                                                                                                                                                                                                                                                      |
| **Anchor integrity**           | See §6.3.4. Captured anchor absent from a local target's slug set; slugifier-style mismatch with `{#id}` attributes; anchor-style skipped for unknown extension.                                                                                                                                                                                                                                                                                               |
| **Category integrity**         | See §5.7.4. `category.unset` (info); `category.mismatch` (warning, sigil contradicts declared category); `category.tooBroadOther` (info, too many patterns share `other`).                                                                                                                                                                                                                                                                                     |
| **Per-feature check families** | The feature sections each enumerate their own check names — see §5.8 `style.*`, §5.9 `unique.*`, §6.4 `submodule.*`, §10.9 `label.*` and `checksum.*`, §11.8 `upgrade.*`, §11.10 `concurrency.*`, §12.2.1 `profileVar.*`, §13.3.3 `responseFilter.*` and `backoff.*`, §14.5.1 WASM bundle-size, §14.7.9 `references.*`, §16.1.1 `commitMessage.*`. All are addressable via the same `integrity.checks.<name>` severity override (§7.3 IntegrityConfig schema). |

Severity per check is configurable. Allowed values: `"error"`,
`"warning"`, `"info"`, `"hint"`, `"off"` (the same `Severity` enum
used throughout §7.3, §14.4, and per-pattern `severity` blocks).

```jsonc
{
  "integrity": {
    "checks": {
      "unusedCapture":      "warning",
      "greedyCapture":      "warning",
      "syntheticOverlap":   "error",
      "anchorModeMismatch": "warning"
    }
  }
}
```

### 9.2 Runtime conflict detection

During every scan (editor live, CLI verifier), the scanner records the byte
range of every match. If two patterns claim overlapping ranges in the same
file, the engine consults each pattern's `priority`:

1. Strictly higher priority wins; the loser is recorded as a *suppressed
   conflict* (visible in `coderef doctor` and verbose output).
2. Equal priority is a **conflict**. Behaviour controlled by
   `integrity.onConflict`:
   - `"error"` (default) — emit an error and skip both. The reference is not
     resolvable.
   - `"warn"` — warn, prefer the first by declaration order (deterministic
     within a single config file).
   - `"first-wins"` — silently take the first by declaration order.

Runtime conflicts are reported with both file:line and both pattern ids.
They become part of the verifier exit status.

### 9.3 `coderef doctor`

```
coderef doctor [--report json|sarif|text] [--corpus <glob>...]
```

What it does:

1. Loads the config and runs every static check from §9.1.
2. Reports `${var}` references that resolve to declared defaults (might hide
   a missing env var).
3. Optionally scans `--corpus` (default: the workspace) and reports every
   runtime conflict found.
4. For overlap groups, emits **suggested guards** — e.g. "add `(?!b/)` look-
   ahead in `todo-user.regex` to disambiguate from `todo-bug`".
5. Runs coupled-change integrity checks (§10.9) and upgrade-rule integrity
   checks (§11.8).

Exit codes: `0` clean, `1` integrity warnings, `2` integrity errors. Pairs
with `pre-commit` for fast project-bootstrap checks.

### 9.4 Synthetic-match generation (technical note)

For the synthetic-match check we walk the regex AST and produce one or more
canonical strings the regex would match. We use `regex-syntax` (the parser
behind the `regex` crate) and implement a small concretiser: character
classes pick a deterministic member, repetitions pick the minimum
cardinality, alternations enumerate. The result is *not* exhaustive —
pathological regexes can defeat it — but it catches the common-case overlaps
that motivate this section, and the runtime check (§9.2) is the ultimate
safety net.

---

## 10. Coupled-Change Enforcement (v0.2; Shape C composable IDs v0.4)

Some code, data, and docs *must change together* but live in different files.
The canonical example is a comment-marker convention that says "if you edit
the code inside this block, you must also edit these other places in the same
commit." Google's internal `LINT.IfChange/ThenChange` linter is the best-known
instance — Chromium has ~1,100 live directives, Fuchsia and TensorFlow use it.
There is no widely-adopted OSS equivalent: the closest neighbours are
[`simonepri/ifttt-lint`](https://github.com/simonepri/ifttt-lint),
[`ebrevdo/ifttt-lint`](https://github.com/ebrevdo/ifttt-lint), and
[`checksync`](https://github.com/somewhatabstract/checksync). None of them
support numeric line-range targets, composable ids, or a configurable marker
syntax, and each hard-codes its own convention.

`coderef` treats coupled-change references as a first-class pattern kind
(`kind: "ifchange"`) so they reuse the regex engine, variable system,
scanner, doctor (§9), and pre-commit hook. Marker syntax is configurable, so
projects can adopt `coderef`'s defaults or keep an existing convention.

### 10.1 Shapes

Three shapes are supported, and a single pattern definition accepts all three.

**Shape A — explicit targets.** The author lists the files (and optionally
line ranges) that must change with this block:

```python
# IfChange
def hash_password(pw): ...
# ThenChange(/docs/security.md#hashing, /tests/test_auth.py:120-180)
```

**Shape B — id-anchored group.** The `IfChange` marker carries an id. All
`IfChange` blocks across the repo with the same id are linked. When one
changes, every peer must change too:

```python
# IfChange(auth-format-v3)
HASH_FORMAT = "argon2id$..."
# ThenChange
```

**Shape C — composable id.** The id may itself be a reference token produced
by any other `coderef` pattern. The id is *resolved* through the reference
engine, and grouping happens on the resolved value, so an
`IfChange(JIRA(PROJ-123))` in `src/a.py` is linked with one in `docs/b.md`:

```python
# IfChange(JIRA(PROJ-123))
def feature_x(): ...
# ThenChange
```

Within one block, mixing shapes is allowed: an id-anchored `IfChange(my-id)`
*and* explicit ThenChange targets means "all peers with `my-id` must change
AND these explicit targets must change."

### 10.2 Target grammar

The `ThenChange` argument is a comma-separated list of *target tokens*.
Whitespace and intra-`()` newlines are tolerated. A target token is one of:

| Form                                  | Meaning                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `/path/to/file`                       | A workspace-rooted file. Some line in it must change.                                                                                                                                                                                                                                                                                                                                                  |
| `path/to/file` (no leading `/`)       | Same; leading `/` is convention, not requirement (see §6.1).                                                                                                                                                                                                                                                                                                                                           |
| `/path/to/file:N`                     | Line N must be inside a changed hunk.                                                                                                                                                                                                                                                                                                                                                                  |
| `/path/to/file:N-M`                   | At least one line in [N, M] must be inside a changed hunk. Drift protection via an external `.coderef-checksums.json` lock file is designed in §10.14 but parked in the post-v0.4 backlog; no inline hash.                                                                                                                                                                                             |
| `/path/to/file:label-name`            | **Named region**. By default, resolves to the block opened by `IfChange('label-name')` in the target file (the recommended form). Also resolves to a `Label('label-name') ... EndLabel` region if `patterns.<id>.label` is configured (optional compat for `ebrevdo/ifttt-lint`-style codebases — §10.3). Disambiguator: `:` followed by digits or `N-M` is a line/range; anything else is a label/id. |
| `:label-name`                         | **Same-file shortcut** — resolves to a label/id in the *referencing* file. Useful for in-module coupled blocks.                                                                                                                                                                                                                                                                                        |
| `/path/to/file#anchor`                | A named anchor (markdown heading slug; future: ctags-style for code).                                                                                                                                                                                                                                                                                                                                  |
| `/path/to/dir/`                       | A directory. Some line of some file in it must change.                                                                                                                                                                                                                                                                                                                                                 |
| `/path/glob/*.md` (with `*` or `**`)  | A glob. See `{any}`/`{all}` flags below.                                                                                                                                                                                                                                                                                                                                                               |
| `JIRA(PROJ-123)`                      | A composable reference — resolved via the matching pattern, then treated as that pattern's target.                                                                                                                                                                                                                                                                                                     |

Each target may carry one suffix flag in `{...}`:

- `{any}` — at least one matched element must change (default for globs).
- `{all}` — every matched element must change.
- `{soft}` — emit a warning instead of an error if the constraint isn't met.

Example: `/docs/api/*.md{all}` requires *every* matching file to be touched.

Variable interpolation is allowed in target tokens with the namespaces listed
in §8.4 (notably *no* `file:` or `ref:` — targets are computed per-block, not
per-match-site, to keep the scan-time index small).

### 10.3 Pattern definition

```jsonc
{
  "patterns": {
    "ifchange-default": {
      "kind": "ifchange",
      "ifChange":  {
        "regex":     "IfChange(?:\\((?<id>[^)]*)\\))?",
        "idCapture": "id"
      },
      "thenChange": {
        "regex":          "ThenChange(?:\\((?<targets>[^)]*)\\))?",
        "targetsCapture": "targets",
        "targetGrammar":  "csv",          // "csv" | "json-array" | "yaml"
        "labelSeparator": ":"             // ":" (default; matches path:line) | "#" (Google/Chromium dialect)
      },
      "label": {
        "open":  { "regex": "Label\\('(?<name>[^']+)'\\)", "nameCapture": "name" },
        "close": { "regex": "EndLabel" }
      },
      "block": {
        "bounding":     "paired",         // §10.4
        "allowNesting": true
      },
      "scope": {
        "prefix": {
          "require":  "comment",          // §5.4.2 — must follow a comment prefix
          "ownLine":  true,               // marker must be alone on its line
          "blockComment": { "leadingDecoration": true }
        }
      },
      "composable": true,                 // permit Shape C
      "severity": {
        "missingChange":     "error",
        "orphanIfChange":    "error",
        "orphanThenChange":  "error",
        "soloId":            "warning",
        "malformedTarget":   "error",
        "unresolvedTarget":  "error",
        "noVerifyWithoutReason": "error"
      }
    }
  }
}
```

`scope.prefix` ensures the marker is recognised in Python (`# IfChange(id)`),
C/C++/TS/Go/Rust (`// IfChange(id)`), SQL/Haskell (`-- IfChange(id)`),
HTML/Markdown (`<!-- IfChange(id) -->` on its own line), etc., without the
author writing per-language regex variants (§5.4.2, §7.5).

For Chromium-style codebases that use `LINT.IfChange/ThenChange`, ship a
preset documented at `examples/ifchange-lint-compat.jsonc` (§10.12). Mix and
match: multiple `kind: ifchange` patterns can coexist and all their blocks
feed the same global index.

### 10.4 Block bounding

Default `bounding: "paired"`: each `IfChange` marker pairs with the **next**
matching `ThenChange` marker in the same file. The block is the inclusive
line range `[ifChange.line, thenChange.line]`.

Strict rules in `"paired"` mode:

- Every `IfChange` must be paired with exactly one `ThenChange`. An unmatched
  marker is an `orphanIfChange` / `orphanThenChange` error.
- A `ThenChange` not preceded by a (still-open) `IfChange` is an
  `orphanThenChange` error.
- `allowNesting: true` lets `IfChange` markers nest; the inner block is
  tracked independently.
- Non-nested overlap of two blocks is always an error (`coupled.blockOverlap`).

`bounding: "multipleThenChange"` (deferred to v0.3) would let one IfChange
have several ThenChange markers that collectively define the targets.

### 10.5 Change detection — three-pass algorithm

Adapted from `ebrevdo/ifttt-lint`'s pass design.

**Pass 1 — Scan.** For every in-scope file, find IfChange / ThenChange
markers, pair them into blocks, and resolve ids (Shape B + C). Each
`IfChange('name')` block already contributes a named region; an
optional second sweep finds `Label('name') ... EndLabel` pairs (only
when `patterns.<id>.label` is configured — §10.3) and merges them
into the same label index. Build:

- `blocks: Block[]` indexed by file
- `blocksById: Map<resolvedId, Block[]>` for Shape B / C
- `labelsByFile: Map<file, Map<labelName, [lineStart, lineEnd]>>`
  (populated from both `IfChange('name')` and, if configured,
  `Label('name')...EndLabel`)
- For each block: `(patternId, file, lineStart, lineEnd, resolvedId?, targets[])`

**Pass 2 — Diff overlay.** Run `git diff -U0 <base>..HEAD` (or `--cached` for
`--staged`). Build `diff: Map<file, IntervalSet<line>>` of changed lines.
For each block, set `block.changed = (block.range ∩ diff[block.file]) ≠ ∅`.

**Pass 3 — Verify co-change.** For each block where `changed === true`:

- *Shape A*: for each target `t`, check whether the diff touches `t`
  according to its grammar (file / range / anchor / glob / composable-ref /
  **named-region label**). Label-form targets (`path:nonNumeric`,
  `:label-name`) are looked up in `labelsByFile` and resolved to their
  `[lineStart, lineEnd]` range before the diff check. A missing target →
  violation.
- *Shape B / C*: for each peer in `blocksById[block.resolvedId]` other than
  this one, require either (a) peer is in `changed` set, or (b) a relaxed
  rule applies (`peerMustChangeAtFileLevel` config knob — relaxes "peer
  block range changed" to "peer's file changed somewhere"). A missing peer →
  violation listing all unchanged peers with their locations.
- **(design-only, post-v0.4) Checksum mode (§10.14):** for every
  target whose `path:N-M` appears in `.coderef-checksums.json`,
  recompute the hash of the normalised range (independent of diff
  coverage) and compare to the stored hash. A mismatch → `checksum.drift`
  violation. Not committed for v0.1–v0.4; checksync covers this niche
  for teams that need it today.

Violations honour `NoVerify` markers (§10.6) before emission.

### 10.6 Escape hatch (`NoVerify`)

Coupled-change checks can be bypassed *explicitly*:

- **Per-block inline.** Place a marker on the line immediately above the
  `IfChange`, or on the same line:
  `# NoVerify(coderef:ifchange) reason: refactor — peer block intentionally lagging`.
  The reason text is required by default (`severity.noVerifyWithoutReason: "error"`).
- **Per-commit message.** Add a line `NoVerify(coderef:ifchange): <reason>`
  anywhere in the commit message. The CLI reads `$PRE_COMMIT_COMMIT_MSG_SOURCE`
  or `.git/COMMIT_EDITMSG` when run at the `commit-msg` stage.

All `NoVerify` usages are logged in the verifier report (`file:line` and
reason). Easy to grep for in audits.

The token `NoVerify(coderef:ifchange)` is namespaced so future coderef
features each get their own opt-out (`NoVerify(coderef:check)`,
`NoVerify(coderef:doctor)`) without colliding.

### 10.7 Id resolution and composability

When `composable: true`, the `id` capture is passed through the reference
engine before grouping:

1. Try each pattern with `kind: "url"` or `kind: "local"` against the literal
   id text.
2. If one matches, compute the *resolved target* (the URL string for `url`;
   the absolute path for `local`) and use it as the canonical group key.
3. If none matches, the literal id text is used (effectively Shape B).

This makes `IfChange(JIRA(PROJ-123))` and `IfChange(JIRA(PROJ-123))` in two
different files group together, and a future migration from
`IfChange(some-string)` to `IfChange(JIRA(PROJ-123))` is non-disruptive
provided both resolve to the same group key.

Doctor fires `coupled.composableTypo` when an id text *almost* matches a
pattern's regex (Levenshtein within a small threshold) but fails to resolve
— usually a typo.

### 10.8 CLI

```
coderef changes [--base <ref>] [--staged]
                [--report json|sarif|text]
                [--profile <name>]
                [--allow-noverify-no-reason]   # for migration sweeps
```

`coderef check --staged` invokes the changes verifier automatically as part
of its pre-commit pass. The standalone `coderef changes` subcommand is useful
when authoring or auditing — `-v` prints every block touched and the peer
check decisions.

Exit codes follow the project convention (§13.1).

### 10.9 Doctor integration

The integrity checker (§9) extends to coupled-change patterns:

| Check                        | Default severity | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `coupled.orphanIfChange`     | error            | IfChange marker with no matching ThenChange.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `coupled.orphanThenChange`   | error            | ThenChange marker with no preceding IfChange.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `coupled.soloId`             | warning          | Only one block uses a given id — the coupling is meaningless.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `coupled.malformedTarget`    | error            | A target token cannot be parsed under §10.2 grammar.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `coupled.unresolvedTarget`   | error            | A target's file or line range does not exist in the working tree.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `coupled.blockOverlap`       | error            | Two non-nested blocks overlap.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `coupled.composableTypo`     | warning          | Id text *almost* matches a pattern's regex but fails to resolve.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `coupled.renameSuspected`    | warning          | A target path doesn't exist but `git log --follow --diff-filter=R` suggests a recent rename.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `coupled.unboundedGlobAll`   | warning          | `{all}` glob matches more than `integrity.coupled.maxAllGlob` files (default 50).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `coupled.cycle`              | warning          | A directed cycle exists in the block→target graph. The check runs Tarjan's strongly-connected-components algorithm over the directed graph whose nodes are coupled-change blocks and whose edges are Shape A target arrows. Target resolution honours `extends:` (§7.6) so cycles spanning subdirectory configs are detected. Glob targets expand to the union of matched files before SCC; multi-target patterns (v0.3) contribute one edge per `targets[]` entry. Cycles are *often intentional* (mutually-coupled blocks where each side must change with the other); the check surfaces them so the author confirms or breaks. Use `NoVerify(coderef:ifchange)` on any edge to declare intent. |
| `checksum.drift`             | error            | Stored hash in `.coderef-checksums.json` differs from current content hash (§10.14). v0.2.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `checksum.untrackedRange`    | info             | A `path:N-M` target appears in source but isn't tracked in the management file. v0.2.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `checksum.staleEntry`        | warning          | Management file has an entry whose `target` no longer resolves (file deleted, out-of-range). v0.2.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `checksum.normalizationOnly` | info             | Drift is purely whitespace / line-ending; safe to `coderef checksum --update --apply`. v0.2.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `checksum.malformedEntry`    | error            | `.coderef-checksums.json` fails JSON parse or schema validation. v0.2.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `label.orphanOpen`           | error            | `Label('name')` without matching `EndLabel`. *Compat-only — emitted only when `patterns.<id>.label` is configured.*                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `label.orphanClose`          | error            | `EndLabel` not preceded by a still-open `Label(...)`. *Compat-only.*                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `label.duplicateInFile`      | error            | Two named regions in one file collide on the same name — whether both are `IfChange('name')`, both are `Label('name')`, or one of each. The check runs against the merged label index built in §10.5 Pass 1.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `label.unknownReference`     | error            | `ThenChange(/path:foo)` where `foo` isn't a named region in `/path` (neither an `IfChange('foo')` id nor, if configured, a `Label('foo')`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `label.unused`               | info             | A named region (`IfChange('name')` block, or compat `Label('name')`) that no `ThenChange` ever references.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `label.ambiguousName`        | error            | Name is purely numeric or matches `N-M` (collides with line-range parsing). Applies to both `IfChange('name')` ids and compat `Label('name')` names.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `label.nesting`              | warning          | A `Label` overlaps another non-nested `Label` in the same file. *Compat-only.*                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

### 10.10 Editor UX

In VSCode (and any future LSP host):

- **Gutter glyph** on every `IfChange` / `ThenChange` line — distinct from
  the reference glyph.
- **Hover over `IfChange`.** Lists every paired ThenChange target and every
  peer block (Shape B / C), each clickable to jump.
- **Hover over `ThenChange`.** Lists each target with last-modified mtime,
  the block's line range, and a "Reveal IfChange" link.
- **Save-time soft warning.** If the user saves a file with a modified
  block but the rest of the working set doesn't include any peer / target,
  surface a non-modal notification ("`auth/hash.py` block changed;
  `tests/test_auth.py` hasn't. Open it?"). Opt-out via
  `coderef.coupled.saveWarning`.
- **Status bar.** Live count of "unsynced coupled blocks" in the current
  changeset.

Pre-commit / CI remains the authoritative gate; the editor is a hint.

### 10.11 Renames, line drift, glob targets

- **Renames.** The diff parser uses `git diff --find-renames`. If a target
  `/old/path.py` no longer exists in the working tree but a rename is
  detected, the verifier emits a *fixable* warning with the suggested new
  path.
- **Line-range drift.** A target `/foo.py:42-80` becomes stale as `foo.py`
  grows. Doctor flags ranges whose upper bound is within 5 lines of EOF or
  whose interval has been heavily edited since the marker was added (signal:
  blame age of the block content). An optional **checksum mode**
  (§10.14, designed but parked in the post-v0.4 backlog) stores
  content hashes for tracked ranges in a separate committed
  `.coderef-checksums.json` management file (Cargo.lock-style); drift
  becomes a fast hash comparison without polluting source markers.
  Inspired by `checksync`'s mechanism but kept out-of-band. Not
  shipping in v0.1–v0.4; checksync covers the niche today.
- **Glob targets.** Always tied to a `{any}` or `{all}` qualifier; default
  for globs is `{any}`. Mixing globs with `{all}` is powerful but easy to
  misuse — doctor warns when an `{all}` glob matches above a configurable
  count.

### 10.12 Migration from `LINT.IfChange/ThenChange`

Shipped preset in `examples/ifchange-lint-compat.jsonc`:

```jsonc
{
  "patterns": {
    "lint-ifchange": {
      "kind": "ifchange",
      "ifChange":   { "regex": "LINT\\.IfChange(?:\\((?<id>[^)]+)\\))?", "idCapture": "id" },
      "thenChange": { "regex":          "LINT\\.ThenChange\\((?<targets>[^)]*)\\)",
                      "targetsCapture": "targets",
                      "targetGrammar":  "csv",
                      "labelSeparator": "#" },        // Chromium uses `#` for label refs
      "label": {
        "open":  { "regex": "LINT\\.Label\\('(?<name>[^']+)'\\)", "nameCapture": "name" },
        "close": { "regex": "LINT\\.EndLabel" }
      },
      "block":      { "bounding": "paired", "scope": { "commentsOnly": true } },
      "composable": false
    }
  }
}
```

Mix-and-match with `ifchange-default` is fine; both pattern ids contribute to
the same global block table. The Chromium-compat preset uses `LINT.Label` /
`LINT.EndLabel` with `#` as the label separator (matching Google's
internal dialect); the default `ifchange-default` uses bare `Label` /
`EndLabel` with `:` (matching the `path:line` convention).

### 10.13 Limitations

- **Heuristic block detection.** When `commentsOnly: true`, the comment
  tokeniser is heuristic for unusual languages — misclassification can
  cause spurious orphans.
- **Cross-repo coupling.** Out of scope. Each repo has its own ifchange
  graph. (Submodules are followed only at the repo boundary they declare.)
- **Generated files.** Targets pointing at generated files (covered by
  `.gitignore`) are usually a smell. Doctor warns.
- **Stash and rebase.** `coderef changes --staged` reflects the index only;
  it cannot detect "I'm about to push without having actually rebuilt the
  test." Pre-push hooks complement.

### 10.14 Checksum-mode drift detection (design only; post-v0.4 backlog)

Range targets (`/foo.py:42-80`) are brittle: someone edits the referenced
range, the diff doesn't touch the *referencing* block, and the line numbers
still resolve — so the reference looks fine but the content the author
remembered is gone. v0.2 closes the loop by tracking content hashes for
opt-in targets in a separate committed management file. Source markers
stay clean; the engine compares stored hash to current content on every
scan.

This was inspired by `checksync`'s "hash in the marker" mechanism but
moved out-of-band: keeping hashes in their own file keeps source markers
readable, lets the CLI auto-update without touching source, and matches
familiar lock-file ergonomics (`Cargo.lock`, `package-lock.json`).

#### 10.14.1 Storage

The management file lives at the workspace root, alongside `.coderef.jsonc`:

```
<workspace>/.coderef-checksums.json    # default location (committed)
```

Configurable via `verification.checksums.path` (variables allowed).

Format — versioned, append-friendly JSON; `coderef` is the sole writer:

```jsonc
{
  "version":   1,
  "algorithm": "sha256-8",
  "normalize": {
    "lineEndings":      "lf",
    "trailingWhitespace": "strip",
    "trailingNewline":  "ensure"
  },
  "entries": [
    {
      "target":    "/tests/test_auth.py:120-180",
      "hash":      "a1b2c3d4",
      "updatedAt": "2026-06-06T12:34:56Z",
      "updatedBy": "marcus.boerger@example.com",
      "note":      "auth-format-v3 expectations"
    },
    {
      "target":    "/docs/security.md:42-58",
      "hash":      "e5f6a7b8",
      "updatedAt": "2026-06-04T09:12:00Z",
      "updatedBy": "sara.miller@example.com"
    }
  ]
}
```

Entry key is the *target string* (path + range; anchor forms are
disallowed for now — anchors get their own treatment via §6.3). Multiple
`IfChange` blocks pointing at the same `(file, range)` share one entry.

#### 10.14.2 Hashing

| Property                  | Default                                   | Rationale                                                                          |
| ------------------------- | ----------------------------------------- | ---------------------------------------------------------------------------------- |
| Algorithm                 | **SHA-256 truncated to 8 hex chars**      | Standard, ~32 bits collision space (1-in-4-billion at our scale), perf irrelevant. |
| Lower-bound               | 6 hex chars                               | Smaller suffix = readable; below 6 risks collisions in big repos.                  |
| Upper-bound               | 16 hex chars                              | Beyond this is just noise.                                                         |
| Line-ending normalization | `lf`                                      | Editor swaps don't trigger spurious drift. `crlf` or `preserve` available.         |
| Trailing whitespace       | `strip`                                   | Same rationale.                                                                    |
| Trailing newline          | `ensure`                                  | Same.                                                                              |
| Encoding                  | Lowercase hex                             | Matches git-blob conventions.                                                      |

The hash input is `normalize(file_lines[N-1..M])` (1-based, inclusive).

#### 10.14.3 Workflow

| Action                   | Command                                         | Effect                                                             |
| ------------------------ | ----------------------------------------------- | ------------------------------------------------------------------ |
| Track new target         | `coderef checksum add <target>...`              | Adds entries with current content hash. Repeatable; merges.        |
| Track everything visible | `coderef checksum add --all-untracked`          | Opt every range-target in the workspace into tracking.             |
| List tracked             | `coderef checksum list [--drift                 | --clean]`                                                          |
| Verify only              | `coderef checksum verify`                       | Recompute, report drift, exit non-zero on mismatch, no writes.     |
| Accept current           | `coderef checksum update [--target <T>...]`     | Refresh stored hashes to current content. After intentional edits. |
| Stop tracking            | `coderef checksum remove <target>...`           | Delete entries.                                                    |

Default mode is verify (dry-run). `add` / `update` / `remove` write, and
all writes go through the same atomic temp+rename used by `coderef upgrade`
(§11.9), so concurrent invocations don't corrupt the file. A workspace
advisory lock (`flock` via `fs4`) serialises competing writers and prints
a clear message on contention.

`coderef check --staged` and `coderef changes --staged` automatically
include `checksum verify` over targets touched by the diff. The full-repo
`coderef check` runs checksum verification too (it's cheap — recomputing
~1000 ranges is sub-second on modern hardware).

#### 10.14.4 Editor UX

Two new code actions on coupled-change targets:

| Range-target state          | Code action                                                                   |
| --------------------------- | ----------------------------------------------------------------------------- |
| Untracked range             | **"Track checksum"** — inserts an entry with current hash.                    |
| Drift detected              | **"Update stored hash"** (accept change) / **"Reveal target"** (investigate). |

The DocumentLink hover on a tracked range shows last-updated metadata
(time + author) when present.

#### 10.14.5 Per-pattern configuration

```jsonc
{
  "verification": {
    "checksums": {
      "path":      "${workspaceFolder}/.coderef-checksums.json",
      "algorithm": "sha256-8",
      "normalize": { "lineEndings": "lf", "trailingWhitespace": "strip",
                     "trailingNewline": "ensure" },
      "severity":  {
        "drift":            "error",
        "untrackedRange":   "info",      // info|warning|error|off
        "staleEntry":       "warning",
        "malformedEntry":   "error"
      }
    }
  }
}
```

A pattern may opt every range-target it produces into tracking by default
via `pattern.checksums.trackByDefault: true`. The `coderef checksum
add --all-untracked` command does the bulk version.

#### 10.14.6 Limitations

- **Tracks ranges, not single tokens.** An untracked `path:N` (no range)
  is fine but doesn't get drift protection — there's no meaningful
  content to hash. Doctor warns if a pattern requests checksum tracking
  on a non-range token.
- **Mass renames are still painful.** If a tracked file is renamed,
  every entry referencing it goes stale. `coderef checksum migrate <old>
  <new>` (v0.2) rewrites the targets in one pass; `coderef checksum
  detect-renames` (v0.3) uses `git log --follow --diff-filter=R` to
  suggest migrations.
- **Refactoring the *range bounds* is normal drift** — moving lines 42–80
  to 50–88 changes the content's location and almost certainly its hash;
  doctor's `checksum.normalizationOnly` distinguishes whitespace-only
  cases that are safe to auto-accept.
- **Not a substitute for full content tracking.** This catches changes
  inside the tracked range; it does not catch changes immediately
  outside (e.g. a new function inserted just before line 42 shifts
  context). For tighter coupling, prefer Shape B / C (id-anchored
  blocks) over Shape A line ranges.

---

## 11. Auto-Upgrade (Codemods) (v0.3)

Most projects don't start with canonical markers. They start with thousands of
`TODO `, `TODO:`, `// FIXME`, `# XXX` comments accumulated over years. For
`coderef` to be useful in such repos, the *adoption phase* — converting all
those legacy markers to the canonical pattern form — has to be a single
command, not a multi-week refactor. `coderef upgrade` is that command.

### 11.1 What it does

Per pattern, declare an ordered list of `upgrade.rules`. Each rule has:

- `match` — a regex (compiled by the same engine as `pattern.regex`).
- `rewrite` — a template that produces the canonical form; uses the variable
  system (§8) plus the rule's own named captures.
- `skip?` — explicit "this is already canonical, do nothing" marker.
- `uses?` — list of resolver namespaces required (currently only `"blame"`).

For each scanned file, rules are tried in order on every byte range that
matches *any* rule's `match`. The first matching rule wins; its `rewrite`
replaces the matched range. Rules with `uses: ["blame"]` only fire when
running under `coderef upgrade` (where blame data can be fetched); they are
silent in normal `coderef check` runs.

### 11.2 Example: full TODO upgrade

```jsonc
{
  "patterns": {
    "todo-user": {
      "regex":  "TODO\\(@(?<user>[\\w.-]+)\\)",
      "target": "https://users.example.com/${user}",
      "upgrade": {
        "rules": [
          // 0. Already canonical — never rewrite.
          { "match": "TODO\\(@[\\w.-]+\\)",                  "skip": true },

          // 1. TODO @marcus  /  TODO: @marcus  →  TODO(@marcus)
          { "match":   "TODO[: ]\\s*@(?<user>[\\w.-]+)\\b",
            "rewrite": "TODO(@${user})" },

          // 2. TODO https://...  /  TODO: https://...  →  TODO(<url>)
          { "match":   "TODO[: ]\\s*(?<url>https?://[^\\s,)]+)",
            "rewrite": "TODO(${url})" },

          // 3. TODO PROJ-123  →  TODO(JIRA(PROJ-123)) via cross-pattern probe.
          //    See §11.7 — the engine tries every other pattern's regex on
          //    `${token}` and rewrites to the composable form when one matches.
          { "match":   "TODO[: ]\\s*(?<token>\\S+)\\b",
            "rewrite": "TODO(${crossPattern:token})",
            "uses":    ["crossPattern"] },

          // 4. Bare "TODO " or "TODO:" with nothing useful following → blame.
          { "match":   "TODO[: ](?!\\()",
            "rewrite": "TODO(@${blame:user})",
            "uses":    ["blame"] }
        ]
      }
    }
  }
}
```

Running `coderef upgrade --apply` on a file containing:

```python
# TODO refactor this once @sara is back
# TODO: PROJ-123
# TODO @marcus: swap the KDF
# TODO drop this once the cache is wired up
```

produces (assuming the line was last touched by `marcus.boerger@…`, and the
`jira` pattern is defined):

```python
# TODO(@sara) refactor this once is back        # rule 1 matched @sara
# TODO(JIRA(PROJ-123))                          # rule 3, cross-pattern
# TODO(@marcus): swap the KDF                   # rule 1 matched @marcus
# TODO(@marcus.boerger) drop this once the cache is wired up   # rule 4, blame
```

Trailing comment text is preserved automatically — rules only replace the
matched range, not the rest of the line. (For rule 1's first example,
"`once @sara is back`" stays put; only `TODO refactor this once @sara` is
rewritten, capturing `@sara` and producing `TODO(@sara) refactor this once`
— actually the regex captures less than that. Authors can sharpen their
`match` regex to control what gets rewritten vs preserved.)

### 11.3 The `blame:` namespace

A new variable namespace (§8.2.4), available **only** in
`upgrade.rules.*.rewrite` (rejected elsewhere as a load-time error).
Resolution per (file, line):

1. Run `git blame -L <line>,<line> --porcelain -- <file>`.
2. Parse author email, name, sha, dates from the porcelain output.
3. If the author email matches `blame.ignoreAuthors` (bots), treat as missing
   and fall through to `blame.fallback`.
4. Otherwise look up `blame.userMapping[author_email]`. If present, that's
   `${blame:user}`. If not, apply `blame.fallback` (§11.4).
5. Memoise the result by `(file, line)` for the duration of the upgrade run.

For uncommitted lines (`git blame` returns the special "Not Committed Yet"
author), apply `blame.fallback` directly.

### 11.4 User mapping

```jsonc
{
  "blame": {
    "userMapping": {
      "marcus.boerger@gresearch.co.uk": "marcus",
      "marcus.boerger@example.com":     "marcus",
      "sara.miller@example.com":        "sara",
      "Marcus Boerger <noreply@example.com>": "marcus"
    },
    "ignoreAuthors": [
      "dependabot[bot]@*",
      "renovate-bot@*",
      "*[bot]@*"
    ],
    "fallback": "emailLocalPart"
  }
}
```

`userMapping` keys are matched against (in order) `author_email`,
`Name <author_email>`. Patterns support `*` for glob matching.

`fallback` resolves when no mapping exists for the author:

- `"emailLocalPart"` (**default**) — take the part before `@`.
  `marcus.boerger@example.com` → `marcus.boerger`. Predictable, never empty,
  gives a usable handle.
- `"gitConfigUserName"` — use `git config user.name`. Useful for solo repos.
- `"literal"` — use the raw `Name <email>` form. Usually not what you want.
- `{ "value": "TBD" }` — static placeholder so the upgrade succeeds and a
  human reviewer picks it up later. Useful when you want to grep for
  unresolved attributions before merging.

Whichever fallback fires, doctor emits `upgrade.blameMissingMapping` so the
mapping table can grow organically.

### 11.5 Cross-pattern resolution

When an `upgrade.rules` rewrite uses `${crossPattern:<capture>}`, the engine:

1. Takes the captured value (e.g. `"PROJ-123"`).
2. Tries each `kind: "url"`/`"local"` pattern's `regex` against that value in
   *anchored* mode (must match the whole captured string, not a substring).
3. If exactly one pattern matches, the rewrite uses the *canonical reference
   form* of that pattern (e.g. `JIRA(PROJ-123)` for the `jira` pattern).
4. If multiple patterns match, declaration order + `priority` settle the tie;
   doctor warns `upgrade.crossPatternAmbiguous` at config time so the user
   either adjusts priorities or sharpens the rule's `match`.
5. If no pattern matches, this rule fails and the engine falls through to the
   next rule in the list. (Bare token stays as is.)

The matching pattern's *canonical reference form* is derived from a synthetic
unparse of the regex (the reverse of doctor's synthetic-match generator,
§9.4): the first literal segment of the regex is the keyword (`JIRA`, `RFC`),
and the body is `(<captured value>)`. For patterns whose canonical form
can't be inferred this way (uncommon), the pattern declares
`canonicalForm: "JIRA(${ticket})"` explicitly under `pattern.upgrade.canonicalForm`.

### 11.6 CLI

```
coderef upgrade [<paths>...]
                [--apply]                 # actually write files (default is dry-run)
                [--diff]                  # output unified diff (suitable for `patch`)
                [--pattern <id>]          # restrict to one pattern (repeatable)
                [--limit <N>]             # max rewrites per file (safety, default 200)
                [--report json|sarif|text]
```

Defaults to dry-run: prints proposed changes. `--apply` writes them
atomically (temp file + rename). `--diff` outputs a unified diff that can be
piped to `patch` (`coderef upgrade --diff | git apply --3way`).

`coderef upgrade` is **opt-in only**: it is *not* run by the default
`coderef-check` pre-commit hook. A separate `coderef-upgrade-check` hook
exists (§16.1) that fails commits with legacy markers, for projects that
want enforcement after their initial conversion is done.

Exit codes: `0` if any proposed changes (or zero changes), `2` config error,
`3` internal error. No "failure" code — upgrade is advisory.

### 11.7 Editor UX (VSCode)

The extension registers a `CodeActionProvider` that, for each legacy marker
the upgrade engine recognises, surfaces a quick-fix:

- **"Upgrade to `TODO(@marcus)`"** (with the actual proposal text) — applies
  this one rewrite.
- **"Upgrade all TODOs in this file"** — applies every upgrade rule in this
  file.
- **"Upgrade all TODOs in workspace"** — palette command; equivalent to
  `coderef upgrade --apply`.

Diagnostics for legacy markers are emitted at `Information` severity by
default (configurable via `coderef.upgrade.diagnostics.severity`), so the
user sees them inline without noisy errors.

Settings:

| Key                                       | Default | Notes                                                |
| ----------------------------------------- | ------- | ---------------------------------------------------- |
| `coderef.upgrade.enabled`                 | `true`  | Master switch for the editor-side upgrade UX.        |
| `coderef.upgrade.diagnostics.severity`    | `info`  | `error`/`warning`/`info`/`hint`/`off`.               |
| `coderef.upgrade.codeActionsOnSave`       | `false` | Auto-apply on save. Off by default; opt-in.          |
| `coderef.upgrade.previewDiff`             | `true`  | Show before/after diff in hover before applying.     |

### 11.8 Doctor integration

| Check                              | Default severity | Description                                                                                                  |
| ---------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------ |
| `upgrade.legacyPresent`            | info             | Files contain legacy markers; suggest `coderef upgrade --diff`.                                              |
| `upgrade.unreachableRule`          | warning          | An upgrade rule's `match` is a strict subset of an earlier rule's; this rule never fires.                    |
| `upgrade.invalidRewrite`           | error            | A rule's `rewrite` template produces a string that doesn't match the pattern's canonical `regex`.            |
| `upgrade.blameMissingMapping`      | info             | The author email from blame isn't in `blame.userMapping`; the fallback is being applied.                     |
| `upgrade.crossPatternAmbiguous`    | warning          | A `${crossPattern:*}` reference matches more than one pattern; tie broken by priority/declaration order.     |
| `upgrade.crossPatternUnresolved`   | warning          | A `${crossPattern:*}` reference matches no pattern; rule falls through to the next.                          |

### 11.9 Safety guarantees

- **Idempotent.** Running `coderef upgrade --apply` twice on the same tree
  produces the same file content as running it once. Skip rules and
  canonical-form detection ensure this.
- **Atomic per-file.** Either every rewrite in a file applies or none do.
  The implementation uses `tempfile::NamedTempFile::persist_with(target)`
  — POSIX `rename(2)` on Unix, `MoveFileExW MOVEFILE_REPLACE_EXISTING` on
  Windows — for same-filesystem atomic replace. A `File::sync_all`
  plus parent-dir fsync (Unix) makes the rename crash-safe.
- **Line-preserving.** Rewrites are in-place edits of comment text; no
  reordering or moving lines around.
- **`--limit N` per file.** Default 200. Prevents runaway transforms from a
  buggy rule.
- **Dry-run by default.** `--apply` is explicit. CI / pre-commit never
  rewrites unless the user has installed the opt-in hook.
- **Blame disabled in `--no-blame` mode.** For corp environments without git
  history available, `--no-blame` skips rules with `uses: ["blame"]`.
- **Workspace-exclusive lock** (§11.10). A concurrent `coderef upgrade`,
  `coderef checksum`, or other write-mode subcommand is serialised via an
  advisory `flock` on `<workspace>/.coderef/lock`.
- **Pre-flight git-clean check** (§11.10). Refuse to write to files with
  uncommitted edits unless `--allow-dirty` is passed.

### 11.10 Concurrency model

All write-mode subcommands — `coderef upgrade --apply`,
`coderef checksum {add,update,remove,migrate}`, future write-modes —
share one concurrency model patterned on Cargo's
`target/.cargo-lock` discipline. Three layers:

#### 11.10.1 Workspace advisory lock

On `--apply`, the subcommand opens `<workspace>/.coderef/lock` (created
if absent) and attempts `fs4::FileExt::try_lock_exclusive()`. If a
competing writer holds the lock, the subcommand prints

```
another coderef write is in progress (pid <N>); waiting up to 30s …
```

and blocks via `lock_exclusive()` until either the lock is granted or
`concurrency.lockTimeoutMs` elapses. On timeout, exit code `2` (config
class) with a hint to retry. The lock is held for the entire write
phase; dropped on subcommand exit (including panic, via RAII).

Read-only subcommands (`coderef check`, `coderef list`, `coderef
explain`, `coderef doctor`) acquire a *shared* lock so they don't see
torn state mid-write, but multiple readers run concurrently.

```jsonc
{
  "concurrency": {
    "lockPath":      "${workspaceFolder}/.coderef/lock",
    "lockTimeoutMs": 30000,
    "allowDirty":    false
  }
}
```

#### 11.10.2 Per-file atomic replace

Every individual file rewrite goes through:

```rust
let temp = NamedTempFile::new_in(target_dir)?;       // same FS as target
temp.as_file().write_all(&new_content)?;
temp.as_file().sync_all()?;                          // fsync file contents
temp.persist_with(target_path)?;                     // atomic rename
// Unix only: fsync the parent directory so the rename is durable.
File::open(target_dir)?.sync_all()?;
```

A reader observing the target file at any instant sees either the old
content or the new content — never a torn write. Even mid-codemod
crashes leave the target intact (the temp file is on the same FS;
discarded by tempfile cleanup or harmless after a reboot).

#### 11.10.3 Pre-flight git-clean check

Before any write, the subcommand runs `git status --porcelain` over
the paths it intends to modify. If any are in `MM` / `AM` / `DD` /
`AA` state (uncommitted edits at the same paths), refuse with

```
coderef: refusing to overwrite uncommitted changes:
  src/auth/hash.py      M
  docs/security.md       M
Pass --allow-dirty to proceed anyway, or commit first.
```

and exit `2`. The `--allow-dirty` flag overrides per-invocation;
`concurrency.allowDirty: true` in config makes it the default (not
recommended — the safety check is cheap).

Doctor: `concurrency.dirtyOverride` (info) — config has
`allowDirty: true`. Surfaces the relaxed posture so it's not
accidentally left on.

#### 11.10.4 Why not per-file locks

Considered and rejected:

- **Per-file `flock` on every rewrite.** Doubles syscalls per file;
  meaningless because the codemod already serialises file work within
  one process via `rayon`. Cross-process contention is what matters,
  and that's what the workspace lock catches.
- **Optimistic concurrency with content-hash compare-and-swap.** Adds
  read-then-write race. Atomicity already comes from `persist_with` —
  no need for a CAS layer on top.

### 11.11 Limitations

- **Multi-line legacy markers** (`# TODO\n# continues here`) are not
  recognised. Single-line only.
- **Squashed commits** attribute lines to the squash author, not the
  original.
- **Co-authored commits.** `Co-authored-by:` trailers in commit messages are
  *not* parsed; blame returns the primary author.
- **Mapping precision.** Emails change, people leave. The mapping is a
  snapshot of "who do we attribute legacy work to *today*." Doctor surfaces
  unmapped emails so the table can be kept current.
- **`coderef upgrade` does not rewrite history.** It edits the working tree;
  users commit the result. Past commits remain attributable via git history.

---

## 12. Network Profiles & Proxy

### 12.1 Why profiles

The same `.coderef.jsonc` is committed to the repo. It must work for:

- **Office / corporate network.** Internal hosts reachable directly; external
  hosts behind an HTTP proxy.
- **VPN / remote.** Internal hosts via VPN, no proxy for external.
- **External-only / contractor / OSS contributor.** No internal access; skip
  internal references instead of failing.
- **CI runners.** Often a mix; per-environment overrides.

### 12.2 Profile shape

```jsonc
{
  "networkProfiles": {
    "office": {
      "internalHostPatterns": ["*.internal.example.com",
                               "jira.internal.example.com"],
      "externalProxy": "http://proxy.example.com:8080",
      "internalProxy": null,
      "noProxy":  ["localhost", "127.0.0.1", "*.internal.example.com"],
      "extraHeaders": { "User-Agent": "coderef/0.1 (office)" },
      "variables": {                         // §12.2.1: profile-scoped variable overrides
        "jiraBase":  "https://jira.internal.example.com",
        "usersBase": "https://users.internal.example.com"
      }
    },
    "vpn": {
      "internalHostPatterns": ["*.internal.example.com"],
      "externalProxy": null,
      "variables": {
        "jiraBase":  "https://jira.internal.example.com",
        "usersBase": "https://users.internal.example.com"
      }
    },
    "external-only": {
      "internalHostPatterns": ["*.internal.example.com"],
      "skipInternal": true,                  // mark internal refs as "skipped", exit 0
      "externalProxy": null,
      "variables": {
        "jiraBase":  "https://example.atlassian.net"
        // usersBase intentionally absent — falls back to top-level default
      }
    },
    "ci-github": {
      "internalHostPatterns": [],
      "externalProxy": null,
      "extraHeaders": { "User-Agent": "coderef/0.1 (ci)" }
    }
  }
}
```

`internalHostPatterns` are matched against the *target host* (after URL
template interpolation). A host that matches is **internal**; everything else
is **external**. Each side gets its own proxy setting.

#### 12.2.1 Profile-scoped variable overrides

Many resources have *different hostnames per environment* — a JIRA
instance reachable as `jira.internal.example.com` on the corporate
network but `example.atlassian.net` from outside; a user-directory
service that's `users.internal` on-VPN but unavailable externally. The
same `JIRA(PROJ-123)` reference must resolve to the right URL
automatically depending on where the user is running coderef.

A network profile may declare a `variables` block that **overrides**
top-level `variables` entries when that profile is active. The variable
resolver consults the active profile's overrides first, then falls
back to the top-level definition:

```jsonc
{
  "variables": {                            // top-level defaults
    "companyHost": "example.com",
    "jiraBase":    "https://jira.${config:variables.companyHost}",   // external default
    "usersBase":   "https://users.${config:variables.companyHost}"
  },
  "networkProfiles": {
    "office": {
      "variables": {
        "jiraBase":  "https://jira.internal.${config:variables.companyHost}",
        "usersBase": "https://users.internal.${config:variables.companyHost}"
      }
    },
    "external-only": {
      "variables": {
        "jiraBase":  "https://example.atlassian.net"
      }
    }
  }
}
```

When `office` is active, `${config:variables.jiraBase}` resolves to
`https://jira.internal.example.com`. The host matches
`internalHostPatterns`, so the verifier routes the request through
`internalProxy` (or directly, depending on `noProxy`). When
`external-only` is active, the same `${config:variables.jiraBase}`
resolves to `https://example.atlassian.net`, which doesn't match
`internalHostPatterns` and goes through the `externalProxy`.

**Merge semantics.** Profile `variables` is a shallow merge over the
top-level — entries the profile doesn't list inherit from top-level
(e.g. `external-only` doesn't override `usersBase`, so that variable
still resolves to its top-level value of
`https://users.example.com`). Recursive resolution (`${config:variables.x}`
referring to another `${config:variables.y}`) sees the merged set, so
profile-specific bases that reference `companyHost` still work.

**Doctor checks for profile variables:**

| Check                          | Severity | Trigger                                                                                                       |
| ------------------------------ | -------- | ------------------------------------------------------------------------------------------------------------- |
| `profileVar.unknownInTopLevel` | warning  | Profile `variables.foo` overrides a name that doesn't exist at top level. Probably a typo.                    |
| `profileVar.cyclicOverride`    | error    | Profile variable definitions create a cycle via `${config:variables.*}` after merge.                          |
| `profileVar.alwaysOverridden`  | info     | Every profile overrides the same top-level variable identically — suggests moving the value to the top level. |

### 12.3 Profile selection

```jsonc
{
  "profileSelection": {
    "order": [
      "flag",                       // --profile / setting
      "env:CODEREF_PROFILE",
      "canary",                     // active probe
      "fallback:external-only"
    ],
    "canary": {
      "url": "http://canary.internal.example.com/health",
      "timeoutMs": 800,
      "onSuccess": "office",
      "onFailure": "external-only"
    }
  }
}
```

Selection runs once per CLI invocation and once at extension activation (and
on config change). The chosen profile is logged.

### 12.4 Per-pattern profile override

A pattern's `verify.profile` (or `actions.verify.profile`) pins which profile
that pattern uses regardless of selection. Useful for "this always uses VPN
even when default profile is external-only".

---

## 13. Verifier

### 13.1 Invocation

```
coderef check [<paths>...]              # full scan of given paths or workspace
coderef check --changed [--base <ref>]  # only references in changed lines
coderef check --staged                  # only staged hunks (pre-commit default; runs §10 too)
coderef check --since <ref>             # commits since ref
coderef check --files <f1> <f2> ...     # explicit files (pre-commit can pass these)
coderef check --commit-msg <file>       # lint a commit-message file (§16.1.1, v0.2)
coderef check --commit-msg --stdin      # lint commit message read from stdin
coderef check --report json|sarif|text  # output format (sarif/json land in v0.2)
coderef check --profile <name>          # override profile selection
coderef check --offline                 # skip all HTTP verification; use cached results only; fail if cache miss (§13.5)
coderef changes                         # coupled-change check (§10.8, v0.2)
coderef upgrade [--apply|--diff|--check-only]   # rewrite legacy markers (§11.6, v0.3); --check-only fails if any legacy markers exist
coderef checksum {add,verify,update,remove,list}   # drift management (§10.14, v0.4)
coderef list [--noverify]               # dump all references; --noverify includes NoVerify markers in the list with their reason text (--noverify v0.2; NoVerify markers are §10 coupled-change feature)
coderef scan --json [--counts-only]     # raw scan output for editor integration (§14.7.7); --counts-only returns only per-pattern totals
coderef explain <ref>                   # show resolution for a single reference token
coderef doctor [--corpus <glob>...]     # see §9.3
coderef cache clear [--anchors]         # drop the verification cache; --anchors clears only anchors.json (§13.5)
coderef lsp                             # LSP server mode (v0.4; same Rust bin)
```

Exit codes: `0` clean, `1` broken references or coupled-change violations,
`2` configuration error, `3` internal error. Distinct codes let CI handle
"broken refs" differently from "crashed".

### 13.2 Changed-lines mode

The killer mode for retrofitting `coderef` onto an existing repo that has
hundreds of legacy unverified references.

Algorithm:

1. `git diff -U0 <base>..HEAD -- <paths>` (or `--cached` for `--staged`).
2. Parse hunks; build `Map<filepath, IntervalSet<lineNo>>` of added/modified
   line numbers (deletions are uninteresting).
3. Scan files for references as normal.
4. Emit only references whose match-start line is inside the changed interval.

The coupled-change verifier (§10.5) consumes the same diff overlay, so a
`--staged` run does both jobs from a single git invocation.

`--base` defaults: `--changed` → `merge-base HEAD origin/HEAD`, `--staged` →
the index, `--since X` → `X`.

### 13.3 HTTP verification policy

Defaults, all overridable per pattern (and per target in multi-target
patterns, §5.3.1):

```jsonc
{
  "verification": {
    "acceptStatus":   [200, 301, 302, 307, 308],
    "method":         "HEAD",                  // fall back to GET on 405
    "timeoutMs":      5000,
    "concurrency":    16,
    "userAgent":      "coderef/0.1",
    "followRedirects": true,
    "maxRedirects":   5,
    "anchor":         "ifPresent",              // §13.3.1: "ifPresent" | "always" | "never"
    "anchorMaxBytes": 1048576,                  // §13.3.1: cap body fetched for anchor parsing
    "backoff": {                                // §13.3.3: retry policy for transient failures
      "initialDelayMs": 250,
      "exponent":       2.0,
      "maxDelayMs":     30000,
      "maxAttempts":    5,
      "maxTotalMs":     60000,
      "jitter":         "full"                  // "full" | "none"
    },
    "onApiFailure":   "warn",                   // §13.3.3: "warn" (default) | "error" | "skip"
    "cache": {
      "path":                     "${workspaceFolder}/.coderef-cache",
      "ttlSeconds":               86400,
      "negativeTtlSeconds":       600,
      "anchorTtlSeconds":         21600,        // §13.5: 6h positive anchor cache
      "anchorNegativeTtlSeconds": 600,          // §13.5: 10min negative
      "filterTtlSeconds":         3600          // §13.3.2: 1h for responseFilter bodies
    }
  }
}
```

- A `HEAD` returning `405 Method Not Allowed` is retried as `GET` with
  range `bytes=0-0`; we never download the body unless required for
  anchor (§13.3.1) or response-filter (§13.3.2) checks.
- `429` triggers backoff per §13.3.3; if the response carries
  `Retry-After`, that header takes precedence over the computed delay
  (capped by `backoff.maxDelayMs`).
- Statuses outside `acceptStatus` are *permanent* failures (no retry) —
  bad URL, 4xx-class errors. The `backoff` policy only applies to
  *transient* failures: network errors, timeouts, `429`, `5xx`.
- A pattern can opt out of verification entirely via
  `actions.verify.enabled: false` (§5.3). The target still resolves and
  opens in the editor; the verifier never makes a network call.

#### 13.3.1 Anchor verification (URL targets)

When the resolved URL contains a fragment (`#section`) and the
pattern's `verify.anchor` is not `"never"`, the verifier fetches the
body so it can parse anchors. The MVP supports:

| `Content-Type`                     | Parser                              | Anchor source                                                       |
| ---------------------------------- | ----------------------------------- | ------------------------------------------------------------------- |
| `text/html` (default)              | `scraper` (html5ever)               | `id=` and `name=` attributes                                        |
| `text/markdown`, `text/x-markdown` | `comrak` / `pulldown-cmark`         | Heading slugs via §6.3.2 slugifier (per-pattern override available) |
| anything else                      | none                                | Skipped with `anchor.skippedContentType` (info)                     |

Mechanics:

- `HEAD` is upgraded to a streaming `GET` automatically when the URL has
  a fragment. The body is bounded by `anchorMaxBytes` (default 1 MiB);
  larger bodies are truncated and a `anchor.bodyTruncated` warning is
  emitted.
- The verifier sends `Accept: text/html, text/markdown` and
  `Accept-Encoding: identity` for predictable parsing.
- For URLs whose `Content-Type` is markdown (raw GitHub blobs, GitLab
  raw, etc.), the per-pattern `slugifier` is applied. Default `github`.
- `verify.anchor` modes mirror the local-pattern semantics (§6.3.1):
  `ifPresent` (default) verifies only when the URL contains a fragment;
  `always` makes the anchor required; `never` skips.
- **No JS execution.** SPAs with JavaScript-injected anchors are not
  supported; matches lychee. Documented as a limitation (§21).

For multi-target patterns (§5.3.1), `verify.anchor` is per-target —
some targets may demand anchor verification, others may skip. Each
target's anchor cache lives under the same `anchors.json` keyed by
resolved URL.

#### 13.3.2 Semantic response filtering (design only; post-v0.4 backlog)

**Existence is enough for most references.** `JIRA(PROJ-123)` is a
stable pointer to a ticket; the ticket being closed doesn't make the
pointer invalid — authors legitimately reference closed tickets ("fixed
in PROJ-123", "see post-mortem in SEC-99") and the linter must not
flag those. The default JIRA / GitHub / GitLab presets check existence
(HTTP 200, with auth if a token is in env) and pass for any state.

Some teams want more: "fail the build if the linked JIRA is `Done` /
`Won't Fix`." `responseFilter[]` is the **opt-in** primitive for that
class of workflow check. Configure it explicitly on patterns where
workflow-state-must-match is a hard requirement; most patterns won't
need it, and we deliberately don't ship "active-only" filters as the
default presets.

The mechanism uses predicates over the response body that reuse the
dotted-path field accessor from `preview.render` (a pattern that wants
to *display* `fields.status.name` in hover can also *filter* on it).
todocheck supports only binary open/closed; flake8-jira-todo-checker
supports configurable disallowed statuses; this generalises both.

```jsonc
{
  "patterns": {
    "jira": {
      "regex":   "JIRA\\((?<ticket>[A-Z][A-Z0-9_]+-\\d+)\\)",
      "target":  "${config:variables.jiraBase}/browse/${ticket}",
      "actions": {
        "verify": {
          "kind":          "http",
          "url":           "${config:variables.jiraBase}/rest/api/2/issue/${ticket}",
          "method":        "GET",
          "headers":       { "Accept":        "application/json",
                             "Authorization": "Bearer ${env:JIRA_TOKEN}" },
          "acceptStatus":  [200],
          "responseFilter": [
            { "field":   "fields.status.name",
              "matches": "^(To Do|In Progress|In Review|Blocked)$" },
            { "field":       "fields.labels[*]",
              "notContains": "wontfix" }
          ],
          "onFilterFail": "broken"
        }
      }
    }
  }
}
```

**Filter primitives**

| Operator                   | Example                                                            | Notes                                               |
| -------------------------- | ------------------------------------------------------------------ | --------------------------------------------------- |
| `equals` / `notEquals`     | `{ field: "state", equals: "open" }`                               | Exact string match.                                 |
| `matches` / `notMatches`   | `{ field: "fields.status.name", matches: "^(Open                   | In .*)$" }`                                         |
| `contains` / `notContains` | `{ field: "fields.labels[*]", notContains: "wontfix" }`            | Array membership; `[*]` walks all array elements.   |
| `before` / `after`         | `{ field: "fields.dueDate", before: "${git:date}" }`               | ISO-8601 date comparison; variables allowed.        |
| `lessThan` / `greaterThan` | `{ field: "fields.priority.id", lessThan: 4 }`                     | Numeric.                                            |
| `exists` / `notExists`     | `{ field: "fields.assignee", exists: true }`                       | JSON-path presence check.                           |

Filters in `responseFilter[]` are AND-combined by default. Per-filter
`onFail: "broken" | "warn" | "info" | "ignore"` overrides the
pattern-level `onFilterFail`. The pattern-level default is `broken`.

The `field` syntax matches `preview.render` exactly — dotted JSON paths
with `[*]` for array iteration and `[i]` for indexed access. There is no
separate "JSONPath" library dependency; the path subset we support is
small and implemented in `coderef-core`.

**Per-tracker presets shipped in `examples/`**

| File                                | Filter intent                                                                                          |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `examples/jira-active.jsonc`        | JIRA status NOT in `[Done, Closed, Rejected]`.                                                         |
| `examples/github-issue-open.jsonc`  | GitHub Issues `state == "open"`.                                                                       |
| `examples/gitlab-issue-open.jsonc`  | GitLab Issues `state == "opened"`.                                                                     |
| `examples/linear-active.jsonc`      | Linear GraphQL: issue `state.type ∈ {triage, backlog, started, unstarted}`.                            |
| `examples/youtrack-active.jsonc`    | YouTrack: `customFields[name=State].value.name != "Done"`.                                             |

**Caching**

A separate `filterTtlSeconds` (default 1h) bounds re-fetching. Tickets
change state more frequently than URLs go 404, so this cache is shorter
than the URL-existence cache. ETag/Last-Modified revalidation applies;
the cache entry stores the parsed body fields, not the raw response.

**Edge cases**

| Case                                        | Behaviour                                                                                                                                 |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Auth failure (401/403)                      | URL-existence check fails first; `responseFilter` not reached. Reported as `auth.failed`.                                                 |
| Non-JSON response with `field`-based filter | `responseFilter.bodyNotJson` (error). Likely the pattern's `url` is wrong, or `Accept` is being ignored by the server.                    |
| Multiple targets per pattern                | `responseFilter` is per-target — each target carries its own (or inherits the pattern-level default).                                     |
| Linear / GraphQL                            | `field` walks GraphQL response paths the same way. Linear's complexity-based rate limit needs care; see §21 open question.                |

#### 13.3.3 Retry & backoff (transient failures)

Transient failures (network errors, timeouts, `429`, `5xx`) trigger
backoff. *Permanent* failures (4xx other than `429`, DNS NXDOMAIN with
`retryNxdomain: false`, TLS handshake failures with no clear retry hint)
fail immediately.

```jsonc
"backoff": {
  "initialDelayMs": 250,         // delay before the 1st retry
  "exponent":       2.0,         // delay_n = delay_{n-1} × exponent
  "maxDelayMs":     30000,        // hard cap on any single delay
  "maxAttempts":    5,            // give up after this many total attempts
  "maxTotalMs":     60000,        // hard cap on cumulative retry time
  "jitter":         "full"        // "full" (delay × U[0.5, 1.5]) | "none"
}
```

The retry loop stops at whichever limit fires first: `maxAttempts` or
`maxTotalMs`. With defaults: attempts at t≈0, 250ms, 500ms, 1s, 2s
(cumulative ~3.75s), with full jitter pushing each individual delay
into `[0.5×, 1.5×]` of the computed value.

If the response carries `Retry-After`, that header takes precedence over
the computed delay (still capped by `maxDelayMs`).

**`onApiFailure` — what to do when retries are exhausted:**

| Value              | Behaviour                                                                    | When to use                                                                             |
| ------------------ | ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `"warn"` (default) | Emit a warning diagnostic; treat the reference as *unverified* (not broken). | Default. Tracker-down doesn't block unrelated commits.                                  |
| `"error"`          | Emit an error; reference is *broken*.                                        | When the team has a hard SLA on the tracker and a failure means "stop the line."        |
| `"skip"`           | Silently pass; treat as verified.                                            | Patterns whose targets are non-critical (RFC links, etc.) and where noise is unwelcome. |

**Disabling verification entirely:** set `actions.verify.enabled: false`
on the pattern (or per-target in `targets[].verify.enabled`). The
verifier never makes a network call; the reference still resolves and
opens. Useful for: private APIs without credentials, patterns whose
targets are intentionally opaque, and per-target opt-outs in a
multi-target list.

**Doctor checks for §13.3.2 / §13.3.3:**

| Check                              | Severity | Trigger                                                                                                              |
| ---------------------------------- | -------- | -------------------------------------------------------------------------------------------------------------------- |
| `responseFilter.fieldMissing`      | warning  | A configured `field` was absent from the response JSON.                                                              |
| `responseFilter.bodyNotJson`       | error    | A `field`-based filter found non-JSON content.                                                                       |
| `responseFilter.alwaysPasses`      | info     | The configured filter could never reject any response that reaches it (e.g. `matches: ".*"`).                        |
| `responseFilter.allRulesIgnore`    | warning  | `onFilterFail: "broken"` set but every filter has `onFail: "ignore"`.                                                |
| `backoff.zeroAttempts`             | error    | `maxAttempts: 0` and `onApiFailure: "skip"` — the verifier never tries.                                              |
| `backoff.exponentTooHigh`          | warning  | `exponent ≥ 5.0` with default `maxDelayMs` means a single retry could be capped immediately, wasting later attempts. |

### 13.4 Why we don't shell out to `lychee` / `curl` / others

The verifier is small (a `HEAD` request, status filtering, redirect handling,
proxy via `reqwest` env/config, a concurrency pool via `tokio`, a JSON cache).
On `reqwest` + `tokio` this is on the order of a few hundred lines. The same
applies to the changes verifier (§10) and the upgrade engine (§11): a
git-diff parser, an interval set, a regex scanner, and a blame parser are
each ~200–400 LOC in Rust.

Building everything in one Rust binary:

- **One static binary, no external runtime required.** `npm install`
  downloads the right prebuilt; `pre-commit` `language: node` runs it.
- **Tight integration with the variable system, profile selection, and
  per-pattern overrides.** Wrapping an external CLI means parsing its output
  and re-encoding the same configuration.
- **One process per CLI invocation.** No per-file `lychee` fork.
- **Cache integration.** Cache writes are atomic with the in-process scanner;
  no shared file format with a third-party tool.

We do, however, encourage interop in the opposite direction: `coderef`'s SARIF
output is consumable by anything that consumes SARIF.

### 13.5 Caching

The verifier persists state under `.coderef-cache/`:

```
.coderef-cache/
├── http.json       # status code + timestamp per URL; local-path mtime
└── anchors.json    # parsed anchor sets per URL and per local file
```

Note: `.coderef-checksums.json` (§10.14, v0.2) is *not* a cache — it's
committed state. It lives at the workspace root, not under
`.coderef-cache/`.

Both caches are invalidated by:

- TTL expiry (positive and negative TTLs separate; `anchorTtlSeconds`
  and `anchorNegativeTtlSeconds` are independent of HTTP status TTLs).
- `--no-cache` flag.
- `coderef cache clear` (also `coderef cache clear --anchors` for just the anchor index).
- **ETag / Last-Modified mismatch** for `anchors.json`: external anchor
  fetches send `If-None-Match`/`If-Modified-Since` on revalidation; a
  `200` (vs `304`) re-parses and refreshes the cache. Mirrors HTTP-cache
  semantics so we don't ship duplicate logic.
- **`mtime` change** for local-file anchor entries.

Cache lives under `.coderef-cache/` (added to `.gitignore` by default; we ship
the rule in the README boilerplate).

`coderef cache clear` is a write-mode subcommand and participates in
the workspace concurrency model (§11.10) — it acquires the exclusive
lock, deletes the cache files atomically (rename-then-remove), and
releases. Read-only consumers of the cache (`coderef check`,
`coderef list`, `coderef explain`, `coderef doctor`) hold a shared
lock that blocks until a `cache clear` or `upgrade --apply` finishes,
so they never see torn state.

**Offline mode.** `coderef check --offline` skips every HTTP request
and uses the cached status for each URL. Cache misses (a URL not
present in `http.json`) are reported as `verify.cacheMiss`
(default severity `error`; configurable). Use cases: airgapped CI,
flaky-network local development, deterministic CI where the cache is
populated by an earlier pipeline stage. Pairs with
`coderef check --no-cache` (the inverse — ignore the cache and hit the
network) for full re-verification.

### 13.6 Reporting

Text (default): grouped by file, colored. SARIF for GitHub/GitLab annotations.
JSON for piping into other tools.

```
docs/architecture.md
  L42  DOCREF(/arch/legacy)            → unresolved (no file or index match)
  L57  JIRA(PLAT-9999)                 → 404 https://jira.example.com/browse/PLAT-9999
src/server/auth.ts
  L13  TODO(@former-employee)          → 404 https://users.example.com/former-employee

Coupled-change violations:
  src/server/auth.ts:13-42  IfChange(auth-format-v3) — peer unchanged: docs/security.md:120-160
  src/hash.py:18-35         IfChange — missing target change: /tests/test_hash.py:40-60

Refs: 142 checked, 3 broken, 0 skipped, 119 cached
Coupled-change: 14 blocks examined, 2 violations
```

---

## 14. VSCode Extension

### 14.1 Extension manifest essentials

- Publisher: `helly25`
- Name: `coderef`
- ID: `helly25.coderef`
- Activation: workspace-wide; activate on the presence of any config file (or
  user setting), not per-language, so refs work in any text file.

### 14.2 Providers

- **`vscode.languages.registerDocumentLinkProvider({ scheme: 'file' }, ...)`** —
  emits one `DocumentLink` per resolved reference. The link target is a URI
  (`https:`/`http:` or `file:`); VSCode handles open natively. Cmd/Ctrl-click
  works for free.
- **`vscode.languages.registerHoverProvider(...)`** — invokes the pattern's
  `preview` action; renders as Markdown. Local file previews show the first N
  lines around the anchor; HTTP previews fetch and apply the `render` template.
  For coupled-change markers, the hover lists peers/targets (§10.10). For
  legacy markers, the hover shows the proposed upgrade form. For multi-target
  references (§5.3.1), the hover lists every target ranked by priority with
  its label, each clickable; failing alternates are marked "unavailable" in
  grey. For unverified references (§5.6), the hover banner reads "unverified
  — verification skipped."
- **`vscode.languages.createDiagnosticCollection('coderef')`** — surfaces
  broken references discovered by the in-editor verifier (which runs lazily
  on save, throttled, governed by `coderef.diagnostics.*` settings), plus
  legacy-marker info diagnostics from the upgrade engine.
- **`vscode.languages.registerCodeActionsProvider(...)`** — quick-fix
  actions for legacy markers (§11.7), renames detected by the changes
  verifier (§10.11), an **"Open with…"** picker that lists every non-primary
  target of a multi-target reference (§5.3.1), and **"Mark verified"** that
  removes the `?` prefix of an unverified reference (§5.6).
- **`vscode.languages.registerCodeLensProvider(...)`** *(design only;
  post-v0.4 backlog, §20.5)* — one CodeLens per **multi-target
  reference** declaration showing `▸ 3 alternates · open · verified 1d
  ago`. Two-phase loading: `provideCodeLenses()` returns the count +
  range (cheap, runs on every edit); `resolveCodeLens()` fills in
  verification status from the cache (only resolves visible lenses).
  Single-target references and unverified
  refs get no CodeLens — the gutter glyph (§5.6) and category icon
  (§5.7) are enough; one lens per *match* would be the documented
  anti-pattern. Off by default; opt in via `coderef.codeLens.enabled`.
- **`TextEditorDecorationType` for coupled-change** — gutter glyphs on
  `IfChange` / `ThenChange` lines (§10.10).

The extension itself does no scanning. It shells out to the Rust binary via
JSON I/O on every relevant event, with debounce + cache for typing-time
responsiveness.

### 14.3 Commands

- `coderef.openReference` — explicit open (palette + context menu).
- `coderef.openReferenceWith` — pick a non-primary target from `targets[]` (§5.3.1).
- `coderef.markVerified` — strip the unverified prefix from a reference under the cursor (§5.6).
- `coderef.previewReference` — show preview in a webview/markdown panel.
- `coderef.verifyWorkspace` — run a full verifier pass; results in Problems panel.
- `coderef.verifyChanged` — verify only changed lines (uses workspace git).
- `coderef.verifyCoupled` — run the coupled-change verifier (§10) for the workspace.
- `coderef.upgrade.previewWorkspace` — dry-run upgrade across the workspace.
- `coderef.upgrade.applyWorkspace` — apply upgrades across the workspace.
- `coderef.upgrade.applyFile` — apply upgrades to the active file.
- `coderef.doctor` — run integrity checks; results in Problems panel.
- `coderef.reloadConfig` — force re-read of the config file.
- `coderef.showActiveProfile` — status-bar item + command.

### 14.4 Settings (excerpt)

| Key                                       | Type    | Default       | Notes                                                                                         |
| ----------------------------------------- | ------- | ------------- | --------------------------------------------------------------------------------------------- |
| `coderef.enabled`                         | boolean | `true`        | Master switch.                                                                                |
| `coderef.configFile`                      | string  | `auto`        | Override discovery; supports `${workspaceFolder}`.                                            |
| `coderef.binPath`                         | string  | `auto`        | Override the Rust binary location (default: bundled).                                         |
| `coderef.networkProfile`                  | string  | `auto`        | Override profile selection.                                                                   |
| `coderef.diagnostics.enabled`             | boolean | `true`        | Surface broken refs as diagnostics.                                                           |
| `coderef.diagnostics.verifyOnSave`        | boolean | `true`        | Re-verify changed files on save.                                                              |
| `coderef.diagnostics.includeUrls`         | boolean | `false`       | Verify external URLs in the editor (slow).                                                    |
| `coderef.diagnostics.severity`            | string  | `warning`     | Default severity for broken refs.                                                             |
| `coderef.diagnostics.integrity`           | boolean | `true`        | Run §9 checks on config load and report.                                                      |
| `coderef.coupled.enabled`                 | boolean | `true`        | Enable in-editor coupled-change diagnostics (§10).                                            |
| `coderef.coupled.saveWarning`             | boolean | `true`        | Soft notification when peers are unedited.                                                    |
| `coderef.upgrade.enabled`                 | boolean | `true`        | Surface legacy-marker quick-fixes (§11).                                                      |
| `coderef.upgrade.diagnostics.severity`    | string  | `info`        | Severity of legacy-marker diagnostics.                                                        |
| `coderef.upgrade.codeActionsOnSave`       | boolean | `false`       | Auto-apply upgrades on save (opt-in).                                                         |
| `coderef.upgrade.previewDiff`             | boolean | `true`        | Show before/after diff in hover before applying.                                              |
| `coderef.targets.hoverList`               | boolean | `true`        | Show every target of a multi-target ref in the hover (§5.3.1).                                |
| `coderef.targets.codeActions`             | boolean | `true`        | Expose "Open with…" code action for alternates.                                               |
| `coderef.codeLens.enabled`                | boolean | `false`       | Show CodeLens above multi-target references (§14.2; post-v0.4 backlog). Off by default.       |
| `coderef.codeLens.includeStatus`          | boolean | `true`        | Include `verified Nd ago` in the lens text (requires cache read on resolve).                  |
| `coderef.codeLens.maxPerFile`             | number  | `100`         | Cap on lenses rendered per file to keep scrolling smooth.                                     |
| `coderef.style.enabled`                   | boolean | `true`        | Apply per-pattern `style` decorations (§5.8). Set false to fall back to plain rendering.      |
| `coderef.commitMessage.enabled`           | boolean | `true`        | Live SCM input-box validation in VSCode (§16.1.1; v0.2).                                      |
| `coderef.strict`                          | boolean | `false`       | Treat every missing required variable as an error, even those with declared defaults (§8.5).  |
| `coderef.references.enabled`              | boolean | `true`        | Master switch for the references browser sidebar (§14.7; v0.2).                               |
| `coderef.references.scan`                 | string  | `workspace`   | `workspace` / `openFiles` / `currentFile`.                                                    |
| `coderef.references.groupBy.primary`      | string  | `category`    | `category` (recommended) / `file` / `folder` / `author` / `status`.                           |
| `coderef.references.maxNodesPerLevel`     | number  | `1000`        | Cap to keep the tree responsive on huge repos.                                                |
| `coderef.binPath`                         | string  | `auto`        | Override the Rust binary location (default: bundled).                                         |
| `coderef.unverified.enabled`              | boolean | `true`        | Render unverified refs with a distinct decoration (§5.6).                                     |
| `coderef.unverified.diagnostics.severity` | string  | `info`        | Severity for unverified-ref diagnostics.                                                      |
| `coderef.editor.visual`                   | string  | `auto`        | `auto`/`always`/`never` — when to open the visual config editor for `.coderef.jsonc` (§14.6). |
| `coderef.preview.enabled`                 | boolean | `true`        | Enable hover previews.                                                                        |
| `coderef.openCommand`                     | string  | `vscode.open` | What command to use for local-file open.                                                      |

### 14.5 Activation cost

`coderef` activates on any file scheme present in the workspace, but does no
work until either a `DocumentLink` request, a hover, or an explicit command
arrives. The extension never blocks on the Rust binary: results are streamed
in over JSON I/O and cached per document. Heavy operations (HTTP verification,
full coupled-change pass, workspace-wide upgrade preview) are explicitly
user-triggered or save-driven, never on every keystroke.

#### 14.5.1 Hybrid execution model (v0.1)

From v0.2 onward the extension uses two execution paths against the
same `coderef-core` crate, chosen by operation kind:

| Operation                                                               | Path                                                | Why                                                                                                                                                     |
| ----------------------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Scan files, hover, document-link enumeration                            | **WASM, in-process** (`@helly25/coderef-core-wasm`) | Sub-millisecond per file. Same regex engine as the CLI, so no "documented subset" caveat. Survives hundreds of edits/second without spawning processes. |
| Verify (HTTP), `coderef upgrade`, `coderef changes`, doctor (deep), LSP | **Native binary**, spawned ad hoc                   | Needs network / file writes / git access / persistent state. Spawn overhead (~5–15 ms) is amortised over a user-action that already implies "do work."  |

The split is invisible to users — the extension API surface is one TS
module that internally dispatches. The native binary is bundled with
the extension (downloaded at install time via the npm wrapper) and
also available on `$PATH`.

v0.1 ships with the WASM hot path from the outset (the alternative —
spawning the binary per hover and accepting "documented subset"
regex divergence between editor and CLI — was rejected because it
makes the architectural commitment to "same engine in both hosts"
hollow). The CLI contract is identical to what other hosts will use
in v0.4 LSP mode.

**WASM bundle constraints (locked targets):**

| Property              | Target              | Hard cap  |
| --------------------- | ------------------- | --------- |
| Size (gzipped)        | ~600 KB             | 1.5 MB    |
| Cold-load time        | <50 ms in VSCode    | 200 ms    |
| Per-file scan latency | <1 ms (50-LOC file) | 5 ms      |

If the WASM build trips the hard cap during development, the design
falls back to v0.1's spawn-per-scan path until the cap can be regained.
Doctor reports the WASM bundle size during build.

#### 14.5.2 WASM module boundary (what `coderef-core-wasm` does *not* do)

The architectural promise is "same engine in both hosts." That promise
holds only because the WASM module is intentionally *less* than the
CLI: it does scanning, matching, and variable resolution against byte
buffers handed in by the host, and nothing else. Anything that touches
the outside world stays in the native binary. The boundary is
load-bearing — it's why we can claim engine parity without quietly
maintaining two parallel implementations.

| Capability                                                                        | WASM module | Native binary |
| --------------------------------------------------------------------------------- | :---------: | :-----------: |
| Regex compile + scan (over a buffer)                                              |      ✓      |       ✓       |
| Variable resolution (`builtin`/`capture:`/`env:`/`config:`/`file:`/`ref:`/`ide:`) |      ✓      |       ✓       |
| Reference resolver (URL templates, local-path shortcuts)                          |      ✓      |       ✓       |
| Doctor static checks at config load                                               |      ✓      |       ✓       |
| Synthetic-match overlap analysis (§9.4)                                           |      ✓      |       ✓       |
| Label discovery + block pairing (§10.5 Pass 1)                                    |      ✓      |       ✓       |
| **Filesystem walk** (workspace, gitignore)                                        |      ✗      |       ✓       |
| **HTTP verifier** (`HEAD`/`GET`, anchor parsing, cache I/O)                       |      ✗      |       ✓       |
| **`git diff` / `git blame` parsing**                                              |      ✗      |       ✓       |
| **Process spawning**                                                              |      ✗      |       ✓       |
| **Async runtime** (`tokio`)                                                       |      ✗      |       ✓       |
| **`coderef upgrade` / `checksum` writes** (tempfile + persist)                    |      ✗      |       ✓       |
| **Workspace lock** acquisition (§11.10)                                           |      ✗      |       ✓       |

The WASM module reads buffers via `wasm-bindgen` arguments; it has no
`std::fs`, no network capability, no `std::process`. The host (VSCode
extension) walks the workspace via `vscode.workspace.findFiles`, reads
file bytes, and passes them in — exactly as the native binary's
`ignore`-crate file walker would do, except the I/O happens on the
host side. Identical output by construction.

This is also why §18's threat model is two-tier: the WASM module can't
exfiltrate data or open subprocesses even if a hostile config were
loaded, because those capabilities aren't compiled in. The CLI has the
full surface and carries the corresponding hardening.

**The WASM module conforms to the CLI** (§4.1.1). For any input the
two share — the configured patterns and a buffer of source text — the
WASM module's output must be byte-identical to `coderef --report json`
running over the same inputs. CI (§16) golden-diffs the two on a
curated corpus and fails on divergence. When the editor disagrees
with `pre-commit`, the editor is wrong by construction; the CLI is the
reference.

### 14.6 Visual config editor (v0.3)

The config is the primary surface users interact with after installing the
extension. A JSONC file with thirty patterns, multiple network profiles, a
blame mapping, and upgrade rules is daunting in a plain text editor — even
with `$schema` autocomplete. The extension ships a **visual config editor**
that complements raw JSONC and makes most edits one-click.

It is opt-in via the `coderef.editor.visual` setting (`auto`/`always`/`never`,
default `auto` — visual editor offered on first open of `.coderef.jsonc`, the
choice remembered per workspace).

#### 14.6.1 Layout

Two panes:

- **Blocks pane (left).** A vertical list of *blocks*: one block per pattern
  in `patterns.*`, plus singleton blocks for `variables`, `languages`,
  `blame`, `verification`, `networkProfiles`, `profileSelection`,
  `integrity`, and `defaults`. Each block shows its id, kind, and a short
  status (e.g. "3 targets · 1 unverified marker · clean").
- **Editor pane (right).** When a block is selected, shows the
  form-based editor for that block's fields. A **Source** tab on the same
  pane reveals the raw JSONC for that block, scrolled to the right region.

Toolbar above the blocks pane: **+ Add pattern** (style picker, §14.6.2),
**Validate** (runs `coderef doctor` and pins results inline), **Source
toggle** (switches the whole view to plain JSONC), **Search** (filter
blocks).

#### 14.6.2 Pattern "styles" (templates)

The **+ Add pattern** button shows a style picker. Each style is a vetted
template that pre-populates the new block with sensible defaults and exposes
only the fields relevant to that shape. The user sees a one-line description
plus a thumbnail of the resulting marker:

| Style                             | Resulting marker                   | Template fills                                                                                                                                     |
| --------------------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| **User reference (multi-target)** | `TODO(@marcus)` / `TODO(?@marcus)` | `kind: url`, regex with `?@`-unverified prefix + `@user` capture, `targets[]` array (primary entry the user fills in), `unverified` block enabled. |
| **Bug / ticket reference**        | `JIRA(PROJ-123)` / `TODO(b/123)`   | `kind: url`, regex `KEYWORD\((?<id>...)\)`, single `target` URL template, optional `actions.preview.kind: http`.                                   |
| **Local doc reference**           | `DOCREF(/docs/x)`                  | `kind: local`, regex `KEYWORD\((?<path>/?[^)]+)\)`, `resolve` block prefilled with `.md`/`.mdx` extensions and `README.md` index file.             |
| **Coupled-change marker**         | `IfChange(id)` / `ThenChange(...)` | `kind: ifchange`, `ifChange`/`thenChange` regex pair, `block.bounding: paired`, `scope.prefix.ownLine: true`.                                      |
| **Generic URL reference**         | `KEYWORD(arg)`                     | Bare-bones `kind: url` with one `target`.                                                                                                          |
| **Free-form regex**               | (anything)                         | Blank pattern, no defaults. For power users — equivalent to source-editing.                                                                        |

Custom user-defined styles can be added by saving a current block as a
template (`Save as style…` action). Stored under
`~/.config/coderef/styles.json` or workspace-local
`.coderef-styles.json`.

#### 14.6.3 Field editors

Per field, the form uses the right UI primitive:

| Field type                | UI                                                                                                                                                                                |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Plain string              | Text input. Placeholder pulled from schema.                                                                                                                                       |
| **Regex**                 | Text input + **live regex preview**: compile-on-type via `fancy-regex`, inline error markers, up to 3 synthetic example matches (§9.4) and a strip listing captured group names.  |
| **URL template**          | Text input with variable-aware autocomplete (`${...}` triggers a popup of available namespaces and resolved values). Hover over a `${...}` reveals what it currently resolves to. |
| Number                    | Numeric input with schema-driven min/max.                                                                                                                                         |
| Enum                      | Segmented control (≤4 options) or dropdown.                                                                                                                                       |
| Array of strings          | Tag-style input; each entry is a chip with reorder handles.                                                                                                                       |
| **`targets[]`**           | Drag-and-reorder list of cards, each card editing one `TargetSpec` (label, URL template, priority, verify flags). The card with the highest priority is highlighted as "Primary." |
| Severity                  | Segmented control `error/warning/info/hint/off`.                                                                                                                                  |
| Boolean                   | Toggle with a one-line explainer.                                                                                                                                                 |
| **`blame.userMapping`**   | Table editor: columns *Email pattern* / *Username*. Add/remove rows. Glob patterns highlighted.                                                                                   |
| **`upgrade.rules[]`**     | Ordered list of cards; each card has a regex field, a rewrite field with variable autocomplete, a flags row (`skip`/`uses`).                                                      |
| **`languages.*`**         | Table editor: columns *Id* / *Extensions* / *Line comment* / *Block comment*. Built-ins greyed out; overrides surfaced.                                                           |
| **Network profile**       | Form with internal-host glob list, proxy URL fields, no-proxy list, extra-headers table.                                                                                          |

#### 14.6.4 Validation and live feedback

- **Schema-driven inline validation** as the user types: violations show as
  red marks with hover-to-explain.
- **Doctor diagnostics** (§9) stream into the visual editor: each block
  shows a coloured badge (red/yellow/green) summarising its status, and the
  field is annotated with the specific check that fired.
- **Synthetic match preview** for every regex field — eliminates the
  "I tweaked the regex and didn't realise it stopped matching" class of
  bugs.
- **Cross-pattern conflict detection** (§9.2) renders below the affected
  block: "This regex overlaps with `todo-bug` for inputs starting with
  `TODO(b/...)`. Adjust priority or sharpen with a guard."
- **Multi-target priority preview** — the card list is sorted live, and
  the primary card carries a "Open by default" badge.

#### 14.6.5 Round-trip with source

- Editing the source pane updates the visual view on debounce (250 ms).
- Editing visually applies a precise JSONC patch via `jsonc-parser`'s edit
  primitives — comments, key order, and trailing-comma style are preserved
  byte-for-byte where possible.
- Cursor position is preserved across syncs.
- If the source has a parse error, the visual pane disables editing and
  surfaces a banner: "fix the source first" with a one-click jump to the
  parse-error location.

#### 14.6.6 What the visual editor can't (yet) do

Some features remain source-editing-first; the visual editor links to the
source pane with the cursor parked on the relevant region:

- Authoring **upgrade-rule rewrite templates** — cross-pattern resolution,
  blame interpolation, and multi-rule ordering are easier in source.
- **Complex variable nesting** (`${config:variables.x}` referring to another
  `${config:variables.y}`) — surfacing the dependency graph visually is
  deferred (v0.3).
- Free-form **`preview.render`** templates with response-field accessors.
- **Hand-crafted regex bodies** beyond the synthetic-match preview.

For each of these, the form shows a short blurb and a "Edit in source" link
that jumps to the right offset.

#### 14.6.7 Implementation

- VSCode `CustomTextEditorProvider` (text-based, so the underlying file
  remains plain `.coderef.jsonc` — no binary document, full git diff and
  `git blame` work normally on it).
- Renderer: React in a webview, with `@vscode/webview-ui-toolkit` for
  primitives matching the host theme.
- JSONC parsing + edit synthesis via `jsonc-parser` (same package used by
  the Rust engine).
- Schema-driven form generation: the JSON Schema in
  `schema/coderef.schema.json` describes types, enums, and constraints; the
  renderer walks the schema and picks UI primitives. Custom widgets
  (regex live-preview, multi-target editor, upgrade-rule cards) are keyed
  off `x-coderef-widget: <name>` extensions in the schema.

#### 14.6.8 Scope and roadmap

- **v0.1**: schema-aware **JSONC source editing** (autocomplete, lint markers,
  hover descriptions from the JSON Schema). No visual block UI yet — but
  the schema is rich enough that source editing is already pleasant.
- **v0.2**: the **visual block editor** described above (blocks pane,
  style picker, field editors, live validation, round-trip).
- **v0.3**: **custom styles** (user-saved templates),
  **variable-dependency-graph panel**, **coupled-change graph view**
  (visualises IfChange/ThenChange relationships across the workspace
  — surfaces the `coupled.cycle` warnings from §10.9 as a clickable
  graph rather than just a doctor message), **regex-builder wizard**
  for users who prefer drag-and-drop over typing regex.

### 14.7 References browser (v0.2)

A sidebar tree view (`coderef.references` view container, activity-bar
entry with the project icon) that lists every reference in the
workspace, grouped category-first per §5.7. The view's purpose: let
authors browse "all the people I've mentioned in TODOs," "all the docs
I've linked to," "all the JIRA tickets I'm still tracking," with one
click to jump or a filter to narrow.

Closest prior art: **Todo Tree** (Gruntfuggly). We adopt its three
scan modes and live-update model, and add **category-first grouping**
(§5.7), **verification-status decoration**, **blame-based authorship**
for `@-less` markers, and **multi-target awareness** in the hover.

#### 14.7.1 Layout

```
REFERENCES (1247)
├── 📁 Files (180)
│   ├── /docs/security/ (45)
│   │   ├── DOCREF  src/auth.ts:13 → /docs/security/hashing.md
│   │   └── ...
│   ├── /docs/api/ (32)
│   └── ...
├── 👤 People (310)
│   ├── @marcus (123)
│   │   ├── TODO  src/auth.ts:13 — swap in the new KDF...
│   │   └── ...
│   ├── @sara (88)
│   └── (unassigned, via blame) (45)
├── 🎫 Tickets (203)
│   ├── PROJ-* (47)
│   ├── PLAT-* (32)
│   └── ...
├── 📜 Standards (8)
│   ├── RFC * (6)
│   └── CVE-2024-* (2)
├── 🔗 URLs (62)
│   ├── go/* (28)
│   ├── github.com/* (19)
│   └── ...
└── 🔄 Coupled-change (22)
    ├── auth-format-v3 (4 blocks)
    └── JIRA(PLAT-1234) (3 blocks)

[ FILTERS  🟢 Verified  🟡 Unverified  🔴 Broken  ✏️ Drifted ]
[ SCAN     workspace | open files | current ]
[ GROUP BY category ▸ file ▸ status ]
```

#### 14.7.2 Scan modes (Todo Tree convention)

| Mode             | What it scans                                 | Trigger                                              |
| ---------------- | --------------------------------------------- | ---------------------------------------------------- |
| `workspace`      | Files matching `scope.include` minus `ignore` | Default; refreshes on file save / file-watcher event |
| `openFiles`      | Currently open editors                        | Per-document on edit, debounced 250 ms               |
| `currentFile`    | Active editor only                            | Per-keystroke, throttled                             |

Toggle via the view-title-bar icon; stored in
`coderef.references.scan`.

#### 14.7.3 Grouping

| Dimension   | Order                                                                   |
| ----------- | ----------------------------------------------------------------------- |
| Primary     | `category` (default, §5.7) / `file` / `folder` / `author` / `status`    |
| Secondary   | category-specific defaults (§5.7.1 table) / overridable                 |
| Tertiary    | `status` (gives every leaf a coloured chip)                             |

Stacked via the chevron next to the grouping selector. Each pattern
declares its category (§5.7); within a category, the per-category
secondary default applies unless overridden.

#### 14.7.4 Filter chips

| Chip          | Effect                                      |
| ------------- | ------------------------------------------- |
| `Verified`    | Show verified references. (default on)      |
| `Unverified`  | Show `?`-prefixed references (§5.6). (on)   |
| `Broken`      | Show failed verification. (on)              |
| `Drifted`     | Show checksum drift (§10.14, v0.2+). (on)   |
| `Mine`        | Filter to `${git:user.email}`-authored.     |
| `Text…`       | Free-text contains.                         |

Chips combine with AND semantics. Stored in
`coderef.references.filters`.

#### 14.7.5 Per-row decoration

| Glyph         | Meaning                                 |
| ------------- | --------------------------------------- |
| 🟢             | Verified                                |
| 🟡             | Unverified (`?`-prefix)                 |
| 🔴             | Broken                                  |
| ✏️            | Drifted (v0.2+)                         |
| ⏱             | Verification cached / aged              |
| **bold**      | Number of children                      |

Hover on a row shows the full match line, multi-target alternates
(§5.3.1) if any, last-verified timestamp, and blame author when known.

#### 14.7.6 Click and command

| Action               | Effect                                                                                                     |
| -------------------- | ---------------------------------------------------------------------------------------------------------- |
| Click a leaf         | Jump to the reference site (configurable reveal).                                                          |
| Right-click          | Context menu with "Open primary target", "Open with…" (multi-target), "Mark verified", "Copy as Markdown". |
| Cmd/Ctrl-click leaf  | Open the *target* directly (file or URL).                                                                  |
| Right-click on group | "Copy subtree as Markdown" — useful for sprint planning.                                                   |

Commands:

| Command                                | Effect                                             |
| -------------------------------------- | -------------------------------------------------- |
| `coderef.references.focus`             | Reveal the view.                                   |
| `coderef.references.search`            | Workspace-wide quickpick over all matches.         |
| `coderef.references.refresh`           | Force re-scan.                                     |
| `coderef.references.copyAsMarkdown`    | Copy tree (or selection) as Markdown checklist.    |
| `coderef.references.exportJson`        | Write current view to a JSON file.                 |

#### 14.7.7 Live updates & lazy loading

`vscode.workspace.createFileSystemWatcher` on `**/*` (filtered by
`scope.include`). Diff against the last index; insert/update/remove
tree nodes incrementally. Debounced (250 ms).

Tree children are computed only when expanded. Initial workspace scan
returns just the *category and pattern counts* — a fast pass via
`coderef scan --json --counts-only`. Users see totals immediately and
pay the per-file enumeration cost only as they drill in. Full match
lists per group are cached after first expansion until invalidated.

#### 14.7.8 Settings

| Key                                           | Default                                           | Notes                                                              |
| --------------------------------------------- | ------------------------------------------------- | ------------------------------------------------------------------ |
| `coderef.references.enabled`                  | `true`                                            | Master switch.                                                     |
| `coderef.references.scan`                     | `workspace`                                       | `workspace` / `openFiles` / `currentFile`                          |
| `coderef.references.groupBy.primary`          | `category`                                        | `category` (recommended) / `file` / `folder` / `author` / `status` |
| `coderef.references.groupBy.secondary`        | `auto`                                            | `auto` honours each category's default (§5.7.1)                    |
| `coderef.references.groupBy.tertiary`         | `status`                                          |                                                                    |
| `coderef.references.filters`                  | `["Verified", "Unverified", "Broken", "Drifted"]` | Initially-active chips.                                            |
| `coderef.references.revealBehaviour`          | `startOfMatch`                                    | `startOfMatch` / `endOfMatch` / `lineStart`                        |
| `coderef.references.maxNodesPerLevel`         | `1000`                                            | Cap to keep the tree responsive on huge repos.                     |
| `coderef.references.author.fromBlame`         | `true`                                            | Use blame to assign authorship to `@-less` markers.                |

#### 14.7.9 Doctor

| Check                           | Severity | Trigger                                                                                      |
| ------------------------------- | -------- | -------------------------------------------------------------------------------------------- |
| `references.tooManyNodes`       | info     | A tree level exceeds `maxNodesPerLevel`; suggests adding a secondary grouping.               |
| `references.uncategorisedSpike` | info     | More than 10% of references land in `📁 Other`; suggests setting `category` on more patterns. |

#### 14.7.10 Differentiators vs Todo Tree

| Capability                                            | Todo Tree | coderef |
| ----------------------------------------------------- | :-------: | :-----: |
| Workspace scan with grouping                          |     ✓     |    ✓    |
| Sub-tag grouping (regex capture)                      |     ✓     |    ✓    |
| Live updates                                          |     ✓     |    ✓    |
| **Category-first display (file/people/...)**          |           |  **✓**  |
| **Status-aware decoration** (verified/broken/drifted) |           |  **✓**  |
| **Blame-based authorship for `@`-less markers**       |           |  **✓**  |
| **Multi-target alternates in hover** (§5.3.1)         |           |  **✓**  |
| **Coupled-change blocks as a category**               |           |  **✓**  |
| **Filter by verification result**                     |           |  **✓**  |
| **Copy-as-Markdown for sprint planning**              |           |  **✓**  |

---

## 15. Other Editors

V0.4: ship a `coderef lsp` mode (the same Rust binary, different transport)
that implements:

- `textDocument/documentLink` → references.
- `textDocument/hover` → previews + coupled-change peers + upgrade proposals.
- `textDocument/publishDiagnostics` → broken refs + integrity + coupled-change.
- `textDocument/codeAction` → upgrade quick-fixes.

Any LSP-capable editor (Neovim, Helix, Sublime LSP, JetBrains LSP plugin) gets
parity. The VSCode extension will eventually migrate to the LSP transport
for unified maintenance; v0.1 ships the JSON-IO transport for simplicity.

### 15.1 The CLI as the multi-IDE behavioural contract

When more than one editor plugin exists, a contract problem appears:
which plugin's behaviour is "correct" if two of them disagree on what a
reference resolves to, or whether a coupled-change block is satisfied?
`coderef` resolves this with a single rule, set in §4.1.1:

> **The CLI is the canonical reference.** Every plugin — the VSCode
> extension's WASM module today, the LSP server in v0.4, any JetBrains
> or Sublime client built on top of LSP afterwards — defines its
> "correct" behaviour as "what `coderef --report json` produces on the
> same input." Any divergence is a bug in the plugin, not in the CLI.

Consequences worth naming:

| Concern                                       | How it gets answered                                                                                                                           |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Plugin-developer onboarding                   | "Install `coderef` from cargo, point it at a fixture file, capture the JSON. That's your test oracle."                                         |
| User support: "VSCode says X, CI says Y"      | "Run `coderef check` locally. The answer it gives is the one the project's contract specifies."                                                |
| New IDE plugin (JetBrains, Helix, Sublime, …) | Define correctness as "LSP responses equal `coderef --report json --lsp-shape` for the same input." A small conformance harness ships in v0.4. |
| Regression testing across the project         | Golden-output diff against committed `coderef --report json` fixtures (§16 CI).                                                                |

This is the load-bearing reason the project is structured as it is.
`coderef-core` is the implementation source of truth (one crate, one
codebase); the CLI is the behavioural source of truth (one observable
contract); plugins are conformance-tested wrappers. The doc, the
schema, and any future SDK derive from those two.

---

## 16. Pre-commit / CI Integration

### 16.1 `pre-commit` (pre-commit.com)

`.pre-commit-hooks.yaml` shipped in this repo:

```yaml
- id: coderef-check
  name: coderef — verify changed source references
  description: Resolve and validate references in changed lines
  entry: coderef check --staged
  language: node
  pass_filenames: false
  always_run: false

- id: coderef-check-full
  name: coderef — verify all source references
  description: Resolve and validate every reference in the workspace
  entry: coderef check
  language: node
  pass_filenames: false
  stages: [manual]

- id: coderef-changes
  name: coderef — verify coupled-change blocks
  description: IfChange/ThenChange enforcement on staged hunks
  entry: coderef changes --staged
  language: node
  pass_filenames: false

- id: coderef-doctor
  name: coderef — integrity check of patterns
  description: Static + corpus-driven overlap and ambiguity checks
  entry: coderef doctor
  language: node
  pass_filenames: false

# Opt-in: lint references in the commit message itself (see §16.1.1).
# Runs at the commit-msg stage; pre-commit framework passes the message
# file path as $1. NOT installed by default.
- id: coderef-commit-msg
  name: coderef — verify commit-message references
  description: Resolve and validate references found in the commit message
  entry: coderef check --commit-msg
  language: node
  stages: [commit-msg]
  pass_filenames: true

# Opt-in: fail the commit if any legacy markers remain. Use after running
# `coderef upgrade --apply` once across the repo. NOT installed by default.
- id: coderef-upgrade-check
  name: coderef — enforce canonical marker form
  description: Fail if any legacy TODO / FIXME markers exist that `coderef upgrade` would rewrite
  entry: coderef upgrade --check-only
  language: node
  pass_filenames: false
  stages: [manual]
```

Consumer usage:

```yaml
repos:
  - repo: https://github.com/helly25/coderef
    rev: v0.1.0
    hooks:
      - id: coderef-check
        args: ["--config", ".config/coderef.jsonc"]   # only if non-standard
      - id: coderef-changes
      - id: coderef-doctor
      # opt-in:
      # - id: coderef-commit-msg
      # - id: coderef-upgrade-check       # after repo migration
```

#### 16.1.1 Commit-message linting

`coderef check --commit-msg <file>` reads the file, strips git's
comment lines (`^#`), and runs the pattern scanner over the remaining
text. Every match is resolved and verified by the same engine that
runs on source files; exit codes are identical (`0` clean, `1` broken,
`2` config error, `3` internal). The `--stdin` variant reads from
standard input for editor integrations.

Patterns participate in commit-message linting iff
`scope.commitMessage` (§5.4.3) is `true` or `"required"`. The default
for `kind: url`/`local` is `true` — the common case is that JIRA /
TODO references in commits should verify the same way they do in
source. `ifchange`/`command` patterns default to `false`.

Conventional-Commits ergonomics:

```
feat(auth): swap KDF to argon2id

Replaces the SHA-256 + PBKDF2 hashing in TODO(@marcus) with a proper
KDF as discussed in JIRA(PROJ-1234).

Refs: JIRA(PROJ-1234), JIRA(SEC-87)
Closes: #142
```

Every `JIRA(...)` and TODO marker gets verified. No special-case
grammar needed; the pattern engine handles all of it.

**`"required"` mode.** Patterns that set `scope.commitMessage:
"required"` enforce "every commit message must contain at least one
match." Doctor surfaces `commitMessage.requiredNeverFires` when running
`coderef doctor --corpus=commit-log` if no recent commit triggered a
required pattern.

**Merge / squash / amend behaviour.**

| Case                            | Behaviour                                                          |
| ------------------------------- | ------------------------------------------------------------------ |
| Empty / abort message           | `git commit` aborts; the hook never runs.                          |
| `git commit --no-verify`        | Bypasses entirely — standard git semantics.                        |
| Amend with no message change    | Hook runs; cached verifier results apply.                          |
| Merge commit (auto-generated)   | Skipped by default (`scope.commitMessage.merges: false`).          |
| Squash commit                   | Linted normally — the squashed message is the input.               |

**VSCode integration.** The extension validates the SCM input box live
as the user types; diagnostics surface inline (squiggle under
unrecognised tickets), and a status-bar item shows the verification
result. Opt-out via `coderef.commitMessage.enabled: false`.

**Doctor checks specific to commit-message mode:**

| Check                                   | Severity | Trigger                                                                                                                  |
| --------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------ |
| `commitMessage.requiredNeverFires`      | info     | A `"required"` pattern wasn't matched in the last N commits (`coderef doctor --corpus=commit-log`).                      |
| `commitMessage.allDisabled`             | info     | Every pattern has `scope.commitMessage: false`; the `--commit-msg` mode would be a no-op.                                |
| `commitMessage.ifchangeMisconfigured`   | warning  | A `kind: ifchange` pattern explicitly sets `scope.commitMessage: true` — IfChange doesn't fire on single-message inputs. |

`coderef-check` already runs the coupled-change verifier when invoked with
`--staged`; `coderef-changes` exists for projects that want it as a separate,
independently-failable hook.

### 16.2 `lefthook`

```yaml
pre-commit:
  commands:
    coderef:
      run: npx -y @helly25/coderef check --staged
```

### 16.3 `husky` + `lint-staged`

```js
// .lintstagedrc.cjs
module.exports = {
  "*": () => "coderef check --staged",
};
```

### 16.4 CI examples

Across CI systems the shape is identical — install Node (or use the
prebuilt platform binary directly), run `coderef check`, produce
SARIF, upload artefacts. Pick the snippet matching your CI; the
underlying command is unchanged.

#### 16.4.1 GitHub Actions

```yaml
- name: coderef (full)
  run: npx -y @helly25/coderef check --report sarif --profile ci-github > coderef.sarif
- uses: github/codeql-action/upload-sarif@v3
  with: { sarif_file: coderef.sarif }
```

GitHub Actions natively renders SARIF in the Code Scanning UI. For
PR-only verification, add a separate step:

```yaml
- name: coderef (changed-lines on PR)
  if: github.event_name == 'pull_request'
  run: npx -y @helly25/coderef check --changed --base origin/${{ github.base_ref }} --report sarif > coderef-pr.sarif
- uses: github/codeql-action/upload-sarif@v3
  with: { sarif_file: coderef-pr.sarif }
```

#### 16.4.2 GitLab CI

```yaml
coderef:
  image: node:22-alpine
  stage:  test
  before_script:
    - apk add --no-cache git           # alpine ships without git
  script:
    - npx -y @helly25/coderef check --report sarif --profile ci-gitlab > coderef.sarif
  artifacts:
    when:  always
    paths: [coderef.sarif]
    reports:
      sast: coderef.sarif              # GitLab Ultimate auto-converts SARIF → SAST report
```

The `reports.sast` line requires GitLab Ultimate; on Free/Premium the
SARIF is published as a plain artefact. For changed-lines-only
verification on merge requests, use:

```yaml
  script:
    - npx -y @helly25/coderef check --changed --base "$CI_MERGE_REQUEST_DIFF_BASE_SHA" --report sarif > coderef.sarif
  only:
    - merge_requests
```

#### 16.4.3 Azure Pipelines

```yaml
steps:
  - task: UseNode@1
    inputs: { version: '22.x' }

  - bash: npx -y @helly25/coderef check --report sarif --profile ci-azure > coderef.sarif
    displayName: coderef (full)

  - task: PublishBuildArtifacts@1
    displayName: Upload SARIF
    inputs:
      pathToPublish: coderef.sarif
      artifactName:  coderef-sarif
```

Azure DevOps doesn't have native SARIF rendering in the build UI; the
[Microsoft SARIF SAST Scans Tab](https://marketplace.visualstudio.com/items?itemName=sariftools.scans)
extension surfaces results in the build tab if installed.
Alternatively, the SARIF can be posted to Azure Defender / GitHub
Advanced Security via the `microsoft/sarif-azuredevops-extension`.

#### 16.4.4 Bitbucket Pipelines

```yaml
pipelines:
  default:
    - step:
        name:  coderef
        image: node:22-alpine
        script:
          - apk add --no-cache git
          - npx -y @helly25/coderef check --report sarif --profile ci-bitbucket > coderef.sarif
        artifacts:
          - coderef.sarif
```

Bitbucket Pipelines doesn't render SARIF natively; teams using
Bitbucket Code Insights publish results via the REST API. The
community workflow is to pipe coderef's JSON reporter
(`--report json`) through a small script that POSTs to
`/2.0/repositories/.../commit/.../reports`. Out of scope for the
shipped examples; a recipe lives in `docs/integrations/bitbucket.md`.

#### 16.4.5 Drone, CircleCI, Buildkite, Jenkins

The command is the same. Install Node (or download the prebuilt
binary directly from the GitHub Release), run `coderef check`, save
the SARIF as an artefact. Worked recipes live in
`docs/integrations/<system>.md`. Pull requests welcome.

#### 16.4.6 Bare binary (no Node available)

For corporate CI without Node, the GitHub Releases attach static
binaries per platform. Download in a single step:

```bash
curl -fsSL "https://github.com/helly25/coderef/releases/download/v0.1.0/coderef-${ARCH}-${OS}.tar.gz" \
  | tar -xz -C /usr/local/bin coderef
coderef check --report sarif > coderef.sarif
```

`${ARCH}` is `x86_64` or `aarch64`; `${OS}` is `linux-gnu`, `linux-musl`,
`darwin`, or `windows`. Checksums and Sigstore signatures shipped
alongside; the install script verifies before extracting.

---

## 17. Testing discipline

The architectural commitments earlier in this document — CLI as the
behavioural source of truth (§4.1.1), host-call-free `coderef-core`
(§14.5.2), conformance across hosts (§15.1) — are only credible if the
project's *test* surface reflects them. This section is a hard
requirement, not aspirational: every PR landing on `main` carries the
tests that make its claims verifiable.

### 17.1 Coverage at every level

| Level                  | What's tested                                          | Where it lives                                                  |
| ---------------------- | ------------------------------------------------------ | --------------------------------------------------------------- |
| Unit                   | One function / module behaviour                        | `mod tests` inside the relevant `.rs` file                      |
| Integration (crate)    | One crate's external API                               | `crates/coderef-core/tests/`                                    |
| Subcommand             | CLI behaviour with mock fs/net                         | `crates/coderef-cli/tests/`                                     |
| Schema                 | JSON Schema accepts valid configs and rejects invalid  | `schema/tests/` (positive + negative example configs)           |
| Conformance            | CLI output ≡ WASM output ≡ LSP output                  | `tests/conformance/` (corpus + golden JSON outputs, §17.4)      |
| Extension end-to-end   | VSCode extension with a real workspace                 | `extension/test/` via `@vscode/test-electron`                   |
| Docs hygiene           | Tables aligned, cross-refs resolve, fences balanced    | `.github/workflows/ci.yml` docs job                             |

All levels are exercised in CI. A change that affects behaviour at
level N must add or update tests at level N.

### 17.2 No one-shots

A *one-shot* is a manual verification done once, then forgotten — e.g.
running a script in a terminal to check a JSON Schema parses, pasting
an example config into a validator, eyeballing CLI output during
development.

One-shots are useful *during development*. They are **not** a substitute
for committed tests. Every one-shot has exactly one of two fates
before the PR that depended on it is merged:

1. **Codified.** The verification logic moves into a committed test
   that runs in CI thereafter. Future regressions are caught
   automatically; future contributors can see the assumption the test
   encodes.
2. **Planned.** If the one-shot is genuinely too expensive or
   impractical to codify immediately (e.g. "I clicked through the
   visual editor's drag-reorder UX and it worked"), it becomes a
   written entry in `docs/test-plan.md` that captures *what was
   verified*, *how*, and *what test should eventually replace this
   note*. The entry is the bridge until a proper test exists.

A PR is not complete until every one-shot it relies on has been
resolved into either path 1 or path 2. "I ran it and it worked"
without a follow-up is a stale anchor — it gives a false sense of
coverage and erodes with every refactor.

### 17.3 Tests vs assertions

Tests verify behaviour; assertions verify invariants. Both have a role:

- **Tests** catch known regressions and known edge cases. They are
  the project's executable specification.
- **Assertions** catch unknown bugs at the moment they manifest
  inside running code. `debug_assert!` in Rust, `console.assert` in
  TS, `assert` in Python tooling. They are guard rails for invariants
  that should be impossible to violate.

Default: prefer tests. Assertions are for things the type system
cannot already enforce.

### 17.4 Conformance harness (per §4.1.1)

The CLI is the behavioural source of truth. Conformance tests reify
that contract end-to-end:

- **Fixture inputs.** A curated corpus of configs + source files lives
  in `tests/conformance/fixtures/`. Each fixture is a directory with a
  `.coderef.jsonc` and one or more source files representative of a
  pattern category (URL refs, local refs, coupled-change, multi-target,
  unverified markers, etc.).
- **Golden outputs.** For each fixture, the output of
  `coderef <subcmd> --report json` is captured and committed under
  `tests/conformance/golden/`. These files are the contract.
- **Plugin verification.** The WASM module (and, from v0.4, the LSP
  server) runs the same inputs and produces JSON. CI diffs each
  plugin's output against the golden. Any divergence fails the
  conformance job.
- **Regenerating goldens.** When a fixture's output legitimately
  changes (a new feature, a bug fix), the golden is regenerated with
  `cargo run --bin regen-conformance` and the diff appears in the PR
  for review. A regen with no design-doc / changelog entry is a code
  review red flag.

### 17.5 What gets a test, when

Mandatory in the PR that introduces the change:

- Every new public function / API endpoint → unit test.
- Every new CLI flag → integration (subcommand) test exercising both states.
- Every schema-affecting change → positive and negative schema tests.
- Every behaviour change visible to a plugin → updated conformance fixture and golden.
- Every bug fix → regression test that fails on the broken version.

Optional / deferable with a `docs/test-plan.md` entry:

- Visual UI tests (extension hover rendering, code-action menus, the
  v0.3 visual config editor's drag-and-reorder UX). Manual verification
  is documented until a UI test harness lands.
- Performance benchmarks (`cargo bench`) — tracked once v0.1 is
  feature-complete.

### 17.6 Test naming

Format: `test_<subject>_<scenario>_<expected>`.

Examples:
- `test_parse_config_with_extends_resolves_relative_paths`
- `test_anchor_verifier_unknown_slug_in_markdown_returns_broken`
- `test_upgrade_with_blame_unmapped_author_falls_back_to_email_localpart`

The convention reads as English at the call site (`failures: test_parse_config_with_extends_resolves_relative_paths`) and surfaces what regressed at a glance.

### 17.7 Meta-checks (doctor over the tests themselves)

A `cargo run --bin doctor -- --tests` mode (post-v0.1) flags the
project itself when discipline slips:

| Check                         | Severity | Trigger                                                                                                       |
| ----------------------------- | -------- | ------------------------------------------------------------------------------------------------------------- |
| `test.untestedSubcommand`     | warning  | A `coderef` subcommand has no integration test under `crates/coderef-cli/tests/`.                             |
| `test.untestedFeatureFlag`    | info     | A v0.X+ feature flag is referenced in code but no test exercises both states.                                 |
| `test.untestedSchemaPath`     | warning  | A schema field is reachable by example configs but has no positive schema test.                               |
| `test.stalePlanEntry`         | info     | `docs/test-plan.md` entry older than 90 days without a linked committed test.                                 |
| `test.conformanceGoldenStale` | warning  | A conformance fixture's source changed but its golden didn't (or vice versa).                                 |

These checks run only when the developer invokes `--tests` explicitly;
they're meta-discipline, not user-facing.

### 17.8 Where this applies to v0.1

The minimum credible v0.1 ships with:

- Unit tests on every public function of `coderef-core` and `coderef-cli`.
- Integration tests for `check`, `check --staged`, `check --changed`,
  `list`, `explain`, `doctor`, `cache clear`.
- Schema positive tests: `examples/minimal.coderef.jsonc` validates clean.
- Schema negative tests: at least three malformed configs in
  `schema/tests/invalid/` that the validator rejects with documented
  error messages.
- Conformance harness scaffolding (fixtures + golden generator) even
  if the corpus is small at v0.1 — sets the shape for v0.2 to grow into.
- VSCode extension end-to-end smoke test: activate, open a file with
  one pattern, verify a `DocumentLink` exists at the expected range.

No PR for any of those is reviewable without its tests. The CI is
configured to fail at the lint stage for missing tests on new
public APIs, not as a separate "merge but please add tests later"
step.

---

## 18. Security Considerations

- **Regex DoS.** All user-supplied regexes are compiled at config load. The
  `regex` crate is DFA-based and immune to catastrophic backtracking;
  `fancy-regex` (used for patterns with lookaround) falls back to backtracking
  but applies a per-match step budget. Reject configs whose total compiled
  regex count exceeds a configurable cap.
- **SSRF in the verifier.** The verifier honours `internalHostPatterns` and
  refuses to call internal hosts when the active profile is `external-only`.
  We do not follow redirects to `file://`, `data:`, `gopher:`, etc.
- **Header secrets.** `${env:NAME}` interpolation in headers reads from the
  environment, never from the config file. Document this clearly. Empty
  headers are dropped, not sent with an empty value.
- **Editor command injection.** `kind: command` patterns expose VSCode
  commands. The extension restricts allowed commands to an allowlist
  declared in the config (and never executes commands containing
  `workbench.action.terminal.sendSequence` or similar).
- **Path escape in local refs.** Local resolution rejects paths that resolve
  outside `resolve.root` after normalisation (`..` traversal). A pattern
  cannot open `/etc/passwd` even with a hostile config — the workspace root
  is a hard boundary. The same rule applies to coupled-change targets.
- **`NoVerify` auditability.** All `NoVerify(coderef:*)` markers are logged
  in the verifier report with file, line, and reason text. Repos can grep
  these via `coderef list --noverify` or include `grep "NoVerify("` as a
  separate audit hook to make over-use visible in code review.
- **Upgrade safety.** `coderef upgrade` is dry-run by default, refuses to
  proceed if a rewrite would not match the pattern's canonical regex
  (doctor `upgrade.invalidRewrite` is a hard error), and applies edits
  atomically per file (temp + rename). Blame queries shell out with strict
  argument quoting; we never interpolate user input into a shell command.
- **Cache poisoning.** Cache key is the resolved URL; cache values are status
  codes only, not body content (preview-cache is separate and opt-in).
- **HTTPS certificate validation.** The verifier uses `reqwest` with TLS
  validation enabled by default — server certificates are checked
  against the host's system trust store at every request. There is no
  per-pattern opt-out for cert validation. Internal CAs are handled by
  configuring the system trust store (`SSL_CERT_FILE`, `SSL_CERT_DIR`
  on Linux/macOS; the Windows trust store on Windows) at the OS level,
  not by disabling validation. Patterns pointing at internal hosts
  with internal CAs simply need the CA installed; coderef does not ship
  a `verify.tls = false` knob, and design-wise will not.
- **WASM vs CLI sandboxing posture.** The architectural split (§14.5.1,
  §14.5.2) means the *only* code path with side effects is the native
  CLI binary. The WASM module used in-process by the VSCode extension
  has no filesystem access, no network capability, no `std::process`,
  no async runtime — it operates only on byte buffers handed in by the
  host. A hostile config loaded by the editor *cannot* exfiltrate data
  or open subprocesses through the WASM path because those
  capabilities are not compiled into the WASM target. The CLI carries
  the full surface and the corresponding hardening above.

---

## 19. Distribution & Versioning

- **Rust binary:** built per platform via `cargo dist` (or hand-written CI
  matrix) for `darwin-{amd64,arm64}`, `linux-{amd64,arm64,musl}`,
  `windows-amd64`. Released as GitHub Release assets with checksums and
  Sigstore signatures.
- **npm wrapper:** `@helly25/coderef` is a tiny TS package whose
  `postinstall` script downloads the correct prebuilt binary from the
  matching GitHub Release. Falls back to `cargo install coderef` if a
  prebuilt is not available. Exposes the `coderef` bin. Lets
  `pre-commit` `language: node` and `npx -y @helly25/coderef …` Just Work.
- **Cargo crates:** `coderef-core` and `coderef-cli` are published to
  crates.io. Semver; breaking changes bump major.
- **VSCode extension:** `helly25.coderef` on VSCode Marketplace via `vsce`
  (matches `vscode-iwyu` setup). Bundles a default binary path; falls back
  to `@helly25/coderef` if not found.
- **Schema:** the JSON Schema is mirrored at
  `https://helly25.github.io/coderef/schema/v1.json` and bundled in the
  cargo and npm packages so offline `$schema` resolution works.
- **`pre-commit` consumers** reference this Git repo and pin `rev:`. We tag
  releases as `vX.Y.Z`.
- **Versioning policy:** `coderef-core` and `coderef-cli` move in lockstep.
  The npm wrapper version tracks the binary it ships. The extension is
  allowed to drift forward for editor-only fixes.

---

## 20. Roadmap

Planning horizon: **v0.1 → v0.4**. Each version is a credible self-contained
milestone with a clear theme. v0.5+ deliberately not planned in detail —
post-v0.4 we revisit the picture once the design has been validated by
real use; see §20.5.

### v0.1 — Minimum viable: pattern engine + WASM-shared core + basic editor

The smallest credible "this works" release. A user can declare URL and
local-path patterns, see them as DocumentLinks in VSCode, verify them
in pre-commit. No coupled-change, no multi-target, no codemods. **WASM
hybrid execution is in scope from day one** — shipping with engine
divergence between editor and CLI would be a credibility loss the
design is built to avoid (§14.5.1).

- Rust workspace skeleton (`coderef-core`, `coderef-cli`) + npm wrapper.
- **`@helly25/coderef-core-wasm`** (§14.5.1): `wasm-bindgen` build of
  `coderef-core` (~600 KB gzipped target, 1.5 MB hard cap). VSCode
  extension imports it for scan / hover / document-link hot paths.
  Native binary stays for HTTP / pre-commit invocations. `coderef-core`
  is host-call-free from day one — that discipline is part of v0.1's
  scaffolding.
- JSONC config loader, schema-validated, with discovery order (§7.1).
- Variable system: `builtin`, `capture:`, `env:`, `config:`, `file:`,
  `ref:`, `ide:` (no `git:`, no `blame:`).
- Pattern engine: `url` + `local` kinds, **single-target** only
  (`target` string; `targets[]` deferred to v0.3).
- Local-path resolution (§6.1, §6.2): extensions, index files,
  leading-`/`, `anchorMode`.
- Basic comment scoping: `commentsOnly` (§5.4.1) with a built-in
  tokeniser for ~10 common language families. Full prefix policy
  (`scope.prefix`) and the full language table land in v0.2.
- Doctor static checks at config load (§9.1): duplicate names,
  regex compile, missing variables, anchor-mode coherence,
  disallowed-variable-scope. Runtime conflict detection on overlapping
  matches.
- HTTP verifier (§13.3): `HEAD` with `GET`-fallback on 405,
  `acceptStatus`, redirect handling, simple `.coderef-cache/http.json`
  status cache, configurable `timeoutMs` / `concurrency`. No anchor
  verification, no `responseFilter`, no backoff configuration (defaults
  only).
- VSCode extension: `DocumentLinkProvider` + `HoverProvider` (URL only,
  no preview templates). All hot-path work goes through the in-process
  WASM module — same regex engine as the CLI, identical semantics.
- CLI: `check`, `check --staged`, `check --changed [--base]`,
  `check --since`, `check --files`, `list`, `explain`, `doctor`,
  `cache clear` (read-only — write-mode `upgrade` lands in v0.3).
- Pre-commit hook (`coderef-check`) + `pre-commit-hooks.yaml`.
- Schema-aware JSONC editing — JSON Schema (`schema/coderef.schema.json`)
  is published, **generated from `coderef-core`'s Rust config types
  via `schemars`** (§7.3); VSCode's built-in JSON support handles
  autocomplete + hover + lint based on `$schema`. CI fails on
  generator output drifting from the committed schema.
- One network profile (`default`); CLI/extension flag overrides for
  custom profiles. Full profile model (canary detection, internal/
  external split) lands in v0.3.
- Example config + `pre-commit-hooks.yaml` in `examples/`.
- **Test scaffolding per §17.8**: unit tests on every public function
  of `coderef-core` / `coderef-cli`; integration tests for every CLI
  subcommand; positive + negative schema tests; conformance harness
  scaffolding (fixtures + golden generator + CI diff job); one VSCode
  extension end-to-end smoke test. CI fails PRs that add public APIs
  without tests.

### v0.2 — Coupled-change + categories + commit messages + browser

The release where `coderef` becomes a *system* instead of just a
linter: cross-file relationships, a navigable browser, lint coverage
of the commit message.

- **`ifchange` kind** with three-pass diff verifier (§10.5),
  Shapes A + B (explicit targets and id-anchored groups). Shape C
  composable ids land in v0.4 alongside the rest of cross-pattern work.
- **Named regions** (§10) — primary form is the id on `IfChange`
  itself: `IfChange('name') ... ThenChange(path:name)`, with
  same-file `:name` shortcut. Optional `Label('name') ... EndLabel`
  compat shape for codebases mirroring `ebrevdo/ifttt-lint`.
  Refactor-stability is the v0.2 deliverable; line-range targets
  remain supported.
- `NoVerify` escape hatch (§10.6).
- **Pattern categories** (§5.7): `files` / `people` / `tickets` /
  `standards` / `urls` / `coupled-change` / `other` + user-defined.
- **References browser** (§14.7): sidebar tree view with category-first
  grouping, three scan modes (workspace / open files / current), status
  decoration, filter chips, click-to-jump, copy-as-Markdown.
- **Full language table** (§7.5): ~50 entries — C-family, hash-family,
  dash-family, block-only, plus the v0.1 extensions (F#, Groovy, Zig,
  V, Protobuf, Make, Perl, GraphQL, Julia, Nim, Tcl, PowerShell,
  Terraform/HCL, Dockerfile, OCaml, Scheme, Fortran, Vim).
- **Full comment-prefix policy** (§5.4.2): `scope.prefix` with
  `require` / `ownLine` / `leadingWhitespace` / `trailingContent` /
  `blockComment` knobs; language-aware composition; `defaults.prefix`
  for config-wide defaults.
- **Anchor verification for in-repo Markdown** (§6.3): precise heading
  parsing via `comrak`, configurable `slugifier` (`github` / `pandoc` /
  `gitlab` / `hugo` / `mkdocs-material` / `custom`). External-URL
  anchor verification lands in v0.3.
- **Commit-message linting** (§5.4.3, §16.1.1): `coderef check
  --commit-msg <file>` and `--stdin`; opt-in `coderef-commit-msg`
  pre-commit hook (`commit-msg` stage); VSCode SCM input-box live
  validation; per-pattern `scope.commitMessage`.
- **Unverified-reference marker (`?`)** (§5.6): per-pattern capture
  binding; doctor `unverified.tooOld` ages via blame (or mtime when
  blame unavailable).
- `coderef changes` standalone subcommand (§10.8).
- **SARIF + JSON reporters** (`--report sarif|json|text`).
- VSCode: coupled-change gutter decorations + hover lists peers/targets;
  unverified-marker decoration; `CodeActionProvider` for "Mark verified".
- Synthetic-match overlap detection (§9.4) added to doctor.
- Doctor consolidation: `categories.*`, `label.*`, `commitMessage.*`,
  `references.*`, `unverified.*`, anchor checks.

### v0.3 — Adoption-phase ergonomics: multi-target + profiles + upgrade + visual editor

The release that makes `coderef` pleasant at scale and through
adoption: alternates per reference, network sophistication, the
`upgrade` codemod, the visual config editor that turns the design's
complexity into one-click setup.

- **Multi-target references** with priority (§5.3.1): `targets[]`
  array; per-target `label`/`url`/`priority`/`verify.required`;
  primary-target click semantics; hover ranks all targets; "Open
  with…" code action.
- **Per-pattern editor styling** (§5.8): optional `style` block →
  `DecorationRenderOptions`; one decoration type per pattern;
  `maxStyledPatterns` doctor cap.
- **Auto-upgrade subsystem** (§11): `coderef upgrade` subcommand,
  `blame:` namespace, `blame.userMapping`, cross-pattern resolution
  (`${crossPattern:capture}`), VSCode quick-fix `CodeActionProvider`,
  `coderef-upgrade-check` opt-in pre-commit hook. Dry-run by default;
  `--tag verify-now` for promoting unverified-to-verified.
- **`git:` variable namespace** (`${git:branch}`, `${git:sha}`,
  `${git:date}`, …).
- **Full network-profile model** (§12): `internalHostPatterns`,
  `externalProxy`/`internalProxy`, `noProxy`, `extraHeaders`,
  `skipInternal`, canary-driven `profileSelection`.
- **Profile-scoped variable overrides** (§12.2.1):
  `networkProfiles[name].variables` block; same `${config:variables.x}`
  resolves to different URLs per active profile.
- **External-URL anchor verification** (§13.3.1): `GET` with body
  parsing via `scraper` (HTML) / `comrak` (`text/markdown`);
  `anchorMaxBytes` cap; `.coderef-cache/anchors.json` ETag-revalidated
  cache; `verify.anchor: ifPresent|always|never`.
- **Verifier `backoff` config + `onApiFailure`** (§13.3.3):
  exponential backoff with jitter; honours `Retry-After`; `warn`
  default, `error`/`skip` configurable; `verify.enabled: false`
  whole-pattern opt-out.
- **Concurrency model** (§11.10) — workspace `flock` on
  `.coderef/lock`, per-file `tempfile + persist`, pre-flight
  git-clean check. Picks up `upgrade --apply` and any future
  write-mode subcommands.
- **Visual config editor** (§14.6): block-based form UI, style
  templates, field-level live validation, regex preview, multi-target
  card editor, source ↔ visual round-trip via `jsonc-parser` edits.
- **Coupled-change save-time soft warnings** in VSCode.
- VSCode diagnostics with `verifyOnSave`.
- `preview.kind: "http"` with `render` templates.

### v0.4 — Power features: LSP + composability + submodules

The release where `coderef` becomes credible across editors and across
repos (within reason). Substantially smaller than the earlier v0.4
sketch — checksum drift, semantic response filtering, CodeLens, and
tag uniqueness all moved to the post-v0.4 backlog (§20.5) as
non-immediate features.

- **Submodule pass-through** (§6.4): opt-in `submodules.follow: true`
  lets the scanner walk into `git submodule`-checked-out trees and
  coupled-change blocks span the boundary. The *only* cross-repo
  mechanism we support — linked-repo manifests rejected (§23.1).
- **LSP server mode** (`coderef lsp`): same Rust binary in a different
  transport. VSCode extension migrates to LSP. Neovim / Helix /
  Sublime LSP / JetBrains LSP plugin gain parity. The WASM build of
  `coderef-core` (v0.1) is also offered to LSP clients that prefer
  in-process embedding over spawning the binary.
- **Composable IDs** (§10.1 Shape C) for coupled-change:
  `IfChange(JIRA(PROJ-123))` groups via the resolved reference target
  through the existing variable + reference engine. Small marginal
  cost over the v0.2 Shape A + B implementation; high differentiator.
- **Multi-config monorepos via `extends:`** (§7.6): per-subdirectory
  `.coderef.jsonc` files inherit from the workspace-root config and
  override per-block. The merge semantics, doctor checks, and the
  "no remote inheritance" non-goal are all in §7.6. Resolves the
  long-standing Open Question #1.

### 20.5 Post-v0.4 — not planned in detail

After v0.4 ships, we revisit the design with real-use signal before
committing to further scope. The standing backlog (none of this is
committed):

- **Checksum-mode drift detection** (§10.14 design). External
  `.coderef-checksums.json` management file with content hashes for
  tracked ranges. Niche — checksync exists; we let users compose with
  it. We'll build this if real demand surfaces.
- **Semantic response filtering** (§13.3.2 design). `responseFilter[]`
  predicate language and per-tracker presets. Engine-side
  infrastructure is small; the maintenance liability is the
  per-tracker preset library (JIRA / Linear / GitHub Issues schemas
  drift). We exit the "tracker-integration" business unless demand
  shows up.
- **CodeLens above multi-target refs** (§14.2 design). Hover lists
  already serve discovery; CodeLens adds visual noise that most users
  disable.
- **Tag uniqueness** (§5.9 design). tagref already owns this niche;
  the §23.1 stance ("compose, don't port") applies — we point users
  at tagref rather than building the same feature inside coderef.
- **JetBrains plugin** (or other native IDE plugins beyond the LSP
  shipped in v0.4).
- **`kind: command`** actions — escape hatch for invoking arbitrary
  VSCode commands as the "open" action.
- **`extends:` mechanism** for nested/multi-config monorepos.
- **`bounding: "multipleThenChange"`** for blocks with several
  ThenChange marker lines.
- **Auto-fix proposals** — when a doc gets renamed, propose
  rewriting every `DOCREF` that pointed at the old path.
- **Visual config editor power-ups** (§14.6.8 design): user-saved
  style templates, variable-dependency-graph panel, regex-builder
  wizard.
- **Schema stability promises and marketplace polish**, **a
  documentation site at `helly25.com/coderef`**.

---

## 21. Open questions

1. **Multi-config files for monorepos.** Resolved: scheduled for v0.4 via
   the `extends:` mechanism. See §7.6.
2. **Anchors for non-Markdown.** Heading slugs are well-defined for Markdown;
   for `.rst`/`.adoc` we need adapters. v0.2.
3. **Comment detection for "exotic" languages** (Haskell, Erlang, Lisp). Ship
   defaults for the top ~20 and document the override path.
4. **License of generated/cached artefacts.** The HTTP cache stores third-party
   status codes only; safe. Preview cache (v0.2) caches third-party content —
   we'll need a per-host opt-in.
5. **Telemetry.** Off by default, no plans to add.
6. **Windows path quirks.** Drive letters, backslashes, UNC paths in local
   refs. Must round-trip through the resolver; tests required.
7. **Internal vs external classification** beyond host patterns — e.g. by
   `CIDR`. Defer; very few users will need it.
8. **Doctor on hostile inputs.** Synthetic-match generation (§9.4) is
   heuristic; a malicious config could craft regexes that evade it. The
   runtime check (§9.2) catches anything that actually fires, so this is
   accepted as a soft guarantee, not a hard one.
9. **Multiple `ThenChange` markers per `IfChange` block.** Some authors find
   it natural to split target lists. Default `bounding: "paired"` rejects
   this; a `bounding: "multipleThenChange"` mode is scheduled for v0.3, but
   semantics around partial overlap need clarifying.
10. **Coupled-change "anti-targets".** A wish-list item: "if this block
    changes, target X must *not* have changed" — would catch accidental
    breakage of stable interfaces. Defer beyond v1.0.
11. **Embedded-language detection.** Inside Markdown fenced code blocks,
    `.vue`/`.svelte`/`.astro` single-file components, or Jupyter
    notebooks, the active comment syntax depends on the embedded region's
    language, not the host file's. Single-language host detection is
    enough for the MVP; region-aware detection is scheduled for v0.3.
12. **EditorConfig / Vim modelines for language hints.** Currently we look
    only at extension/filename/shebang/VSCode language id. Some projects
    declare language overrides in `.editorconfig` or modelines. Worth
    adopting if the maintenance cost is low.
13. **Blame across renames and squashes.** `git blame --follow` would
    attribute through renames; squashed commits credit the squash author.
    The MVP accepts both as documented limitations (§11.11). A v0.3+
    `blame.followRenames: true` option would invoke `--follow` at the cost
    of additional `git` subprocess time.
14. **Maintaining `blame.userMapping` over time.** As people leave or change
    emails, the table goes stale. Worth shipping a `coderef blame report`
    helper that surfaces "authors found by upgrade but not in the mapping"
    and "mappings that no longer match anyone in the last N commits"?
15. **Co-authored-by attribution.** Parsing `Co-authored-by:` trailers from
    commit messages could yield multiple attributions per line. Adds
    complexity; deferred.
16. **Linear API complexity-based rate limiting.** Linear's GraphQL API
    bills queries by *complexity*, not just request count. A naive
    `responseFilter` against Linear could burn the budget on
    over-broad queries. Doctor should warn when a Linear-shaped filter
    requests too many nested fields; the response-filter spec should
    probably pin the GraphQL selection set per preset (deferred).
17. **GraphQL responses generally.** REST-style dotted-path field
    access works for GraphQL's data envelope (`data.issue.state...`)
    but doesn't model GraphQL errors as a first-class concept. Treat
    `errors[]` non-empty as `responseFilter.fieldMissing`-equivalent? v0.3.

---

## 22. Appendix — Example end-to-end config

```jsonc
{
  "$schema": "https://helly25.github.io/coderef/schema/v1.json",

  "ignore": ["**/node_modules/**", "**/dist/**", "**/.git/**", "**/*.min.*"],

  "variables": {
    "companyHost": "example.com",
    "usersBase":   "https://users.${config:variables.companyHost}",
    "jiraBase":    "https://jira.${config:variables.companyHost}",
    "bugsBase":    "https://bugs.${config:variables.companyHost}"
  },

  // Per-pattern defaults applied when a pattern omits its own scope/prefix.
  "defaults": {
    "prefix": { "require": "comment", "ownLine": false }
  },

  // User extensions to the built-in language table (§7.5).
  "languages": {
    "starlark": { "extensions": [".bzl", ".star"],
                  "filenames":  ["BUILD", "BUILD.bazel", "WORKSPACE", "MODULE.bazel"],
                  "lineComment": "#" }
  },

  // Author-email → username mapping for `coderef upgrade` blame lookups.
  "blame": {
    "userMapping": {
      "marcus.boerger@gresearch.co.uk": "marcus",
      "marcus.boerger@example.com":     "marcus",
      "sara.miller@example.com":        "sara"
    },
    "ignoreAuthors": ["*[bot]@*", "renovate-bot@*"],
    "fallback": "emailLocalPart"
  },

  "patterns": {
    "todo-user": {
      "regex":    "TODO\\((?<unverified>\\?)?@(?<user>[a-zA-Z][\\w.-]{0,63})\\)",
      "title":    "@${user}",
      "category": "people",
      "priority": 10,

      // Multi-target: every alternate location worth surfacing for this user.
      "targets": [
        { "label": "User home",
          "url":      "${config:variables.usersBase}?${user}",
          "priority": 100,
          "verify":   { "required": true,  "profile": "internal" } },
        { "label": "Epitaph",
          "url":      "https://company.local/epitaphs?${user}",
          "priority":  50,
          "verify":   { "required": false, "profile": "internal" } },
        { "label": "External profile",
          "url":      "https://company.local/external?${user}",
          "priority":  30,
          "verify":   { "required": false, "profile": "external-only" } },
        { "label": "Partner profile",
          "url":      "https://external.partner.com/users/${user}",
          "priority":  20,
          "verify":   { "required": false, "profile": "external-only" } }
      ],

      // `?` prefix marks unverified refs; verifier skips, doctor warns past maxAge.
      "unverified": {
        "capture":     "unverified",
        "maxAge":      "90 days",
        "diagnostics": "info"
      },

      "severity": { "broken": "warning" },

      "upgrade": {
        "rules": [
          // Already canonical (verified or unverified) — leave alone.
          { "match": "TODO\\(\\??@[\\w.-]+\\)",                   "skip": true },

          // "TODO @marcus" → "TODO(@marcus)"
          { "match":   "TODO[: ]\\s*@(?<user>[\\w.-]+)\\b",
            "rewrite": "TODO(@${user})" },

          // "TODO https://..." → "TODO(https://...)"
          { "match":   "TODO[: ]\\s*(?<url>https?://[^\\s,)]+)",
            "rewrite": "TODO(${url})" },

          // "TODO PROJ-123" → "TODO(JIRA(PROJ-123))" via cross-pattern.
          { "match":   "TODO[: ]\\s*(?<token>\\S+)\\b",
            "rewrite": "TODO(${crossPattern:token})",
            "uses":    ["crossPattern"] },

          // "TODO " with nothing useful following → blame.
          { "match":   "TODO[: ](?!\\()",
            "rewrite": "TODO(@${blame:user})",
            "uses":    ["blame"] },

          // Promote an unverified ref to verified once you've confirmed it.
          { "match":   "TODO\\(\\?@(?<user>[\\w.-]+)\\)",
            "rewrite": "TODO(@${user})",
            "tag":     "verify-now" }
        ]
      }
    },

    "todo-bug": {
      "regex":    "TODO\\(b/(?<id>\\d{3,})\\)",
      "target":   "${config:variables.bugsBase}/${id}",
      "title":    "Bug #${id}",
      "category": "tickets",
      "priority": 10
    },

    "jira": {
      "regex":         "JIRA\\((?<ticket>[A-Z][A-Z0-9_]+-\\d+)\\)",
      "target":        "${config:variables.jiraBase}/browse/${ticket}",
      "category":      "tickets",
      "canonicalForm": "JIRA(${ticket})",
      "actions": {
        // Existence-only verify — passes for any ticket state (open or
        // closed). The browse URL often redirects to SSO; the REST API
        // is a more reliable existence probe. State filtering via
        // `responseFilter` (§13.3.2) is opt-in for teams that explicitly
        // want it; the default just confirms the ticket exists.
        "verify": {
          "kind":         "http",
          "url":          "${config:variables.jiraBase}/rest/api/2/issue/${ticket}",
          "method":       "HEAD",
          "headers":      { "Authorization": "Bearer ${env:JIRA_TOKEN}" },
          "acceptStatus": [200]
        },
        "preview": {
          "kind":    "http",
          "url":     "${config:variables.jiraBase}/rest/api/2/issue/${ticket}?fields=summary,status",
          "headers": { "Accept":        "application/json",
                       "Authorization": "Bearer ${env:JIRA_TOKEN}" },
          "render":  "**{fields.summary}** — *{fields.status.name}*"
        }
      }
    },

    "docref": {
      "regex":    "DOCREF\\((?<path>/?[^)\\s#]+)(?:#(?<anchor>[^)\\s]+))?\\)",
      "kind":     "local",
      "target":   "${path}",
      "category": "files",
      "resolve": {
        "root":          "${workspaceFolder}",
        "anchorMode":    "workspace",
        "extensions":    [".md", ".mdx"],
        "indexFiles":    ["README.md"],
        "anchor":        "${anchor}",
        "anchorVerify":  "ifPresent",   // omit "#..." in the reference to opt out
        "slugifier":     "github"
      }
    },

    "rfc": {
      "regex":    "RFC\\((?<num>\\d{2,5})\\)",
      "target":   "https://www.rfc-editor.org/rfc/rfc${num}",
      "category": "standards",
      "severity": { "broken": "info" }
    },

    "ifchange-default": {
      "kind":     "ifchange",
      "category": "coupled-change",
      "label": {
        "open":  { "regex": "Label\\('(?<name>[^']+)'\\)", "nameCapture": "name" },
        "close": { "regex": "EndLabel" }
      },
      "ifChange":  { "regex": "IfChange(?:\\((?<id>[^)]*)\\))?", "idCapture": "id" },
      "thenChange": {
        "regex":          "ThenChange(?:\\((?<targets>[^)]*)\\))?",
        "targetsCapture": "targets",
        "targetGrammar":  "csv"
      },
      "block":      { "bounding": "paired", "allowNesting": true },
      "scope": {
        "prefix": {
          "require": "comment",
          "ownLine": true,
          "blockComment": { "leadingDecoration": true }
        }
      },
      "composable": true,
      "severity": {
        "missingChange":         "error",
        "orphanIfChange":        "error",
        "orphanThenChange":      "error",
        "soloId":                "warning",
        "malformedTarget":       "error",
        "unresolvedTarget":      "error",
        "noVerifyWithoutReason": "error"
      }
    }
  },

  "verification": {
    "acceptStatus":   [200, 301, 302, 307, 308],
    "method":         "HEAD",
    "timeoutMs":      5000,
    "concurrency":    16,
    "anchor":         "ifPresent",
    "anchorMaxBytes": 1048576,
    "cache": { "path": "${workspaceFolder}/.coderef-cache",
               "ttlSeconds": 86400, "negativeTtlSeconds": 600,
               "anchorTtlSeconds": 21600, "anchorNegativeTtlSeconds": 600 }
  },

  "networkProfiles": {
    "office": {
      "internalHostPatterns": ["*.internal.${config:variables.companyHost}",
                               "jira.internal.${config:variables.companyHost}",
                               "bugs.internal.${config:variables.companyHost}",
                               "users.internal.${config:variables.companyHost}"],
      "externalProxy": "http://proxy.${config:variables.companyHost}:8080",
      "noProxy":       ["localhost", "127.0.0.1",
                        "*.internal.${config:variables.companyHost}"],
      // Profile-scoped variable overrides (§12.2.1): on the office network,
      // JIRA / users / bugs all resolve to internal hostnames.
      "variables": {
        "jiraBase":  "https://jira.internal.${config:variables.companyHost}",
        "bugsBase":  "https://bugs.internal.${config:variables.companyHost}",
        "usersBase": "https://users.internal.${config:variables.companyHost}"
      }
    },
    "vpn": {
      "internalHostPatterns": ["*.internal.${config:variables.companyHost}",
                               "jira.${config:variables.companyHost}",
                               "bugs.${config:variables.companyHost}",
                               "users.${config:variables.companyHost}"]
    },
    "external-only": {
      "internalHostPatterns": ["*.internal.${config:variables.companyHost}"],
      "skipInternal": true,
      // External default — JIRA lives on Atlassian Cloud for off-network access;
      // users/bugs intentionally fall back to top-level (unavailable, so
      // references to them will fail and the user can `NoVerify` them).
      "variables": {
        "jiraBase": "https://example.atlassian.net"
      }
    }
  },

  "profileSelection": {
    "order": ["flag", "env:CODEREF_PROFILE", "canary", "fallback:external-only"],
    "canary": {
      "url":       "http://canary.internal.${config:variables.companyHost}/health",
      "timeoutMs": 800,
      "onSuccess": "office",
      "onFailure": "external-only"
    }
  },

  "integrity": {
    "onConflict": "error",
    "checks": {
      "unusedCapture":      "warning",
      "greedyCapture":      "warning",
      "syntheticOverlap":   "error",
      "anchorModeMismatch": "warning"
    },
    "coupled": {
      "maxAllGlob": 50
    }
  }
}
```

Authoring examples that the above patterns recognise:

```python
# /src/auth/hash.py

# IfChange(auth-format-v3)
HASH_FORMAT = "argon2id$..."
# ThenChange(/docs/security.md#hashing, /tests/test_hash.py:40-80)

# Named region — recommended primary form. The id on `IfChange`
# *is* the label; the block from `IfChange('name')` to its matching
# `ThenChange` is the named region. Refactor-stable; the range
# follows the content. Targets address it as `path:name`.
# IfChange('argon2-params')
HASH_PARAMS = {"memory_kib": 19456, "iterations": 2, "parallelism": 1}
def hash_password(pw): ...
# ThenChange(/docs/security.md:argon2-params, /tests/test_auth.py:argon2-params)

# Same-file shortcut: drop the path prefix to address an id in the
# same file.
# IfChange('cache-keys')
CACHE_KEY_FMT = "v3:{user}:{kind}"
# ThenChange(:cache-readers)

# Optional compat form (e.g. for codebases migrating off
# `ebrevdo/ifttt-lint`). Same semantics; explicitly bracketed region
# inside the IfChange/ThenChange pair via `Label(...) / EndLabel`.
# Configure via `patterns.<id>.label.{open,close}` in §10.3 — the
# default ifchange pattern leaves it disabled.
# IfChange
# Label('legacy-region')
LEGACY_DATA = {...}
# EndLabel
# ThenChange(/docs/security.md:legacy-region)

# IfChange(JIRA(PLAT-1234))
def feature_x(): ...
# ThenChange

# TODO(@marcus): swap in the new KDF once PLAT-1234 lands
# JIRA(PLAT-1234) tracks the rollout
# DOCREF(/docs/security) explains the threat model
```

Pre-upgrade input that `coderef upgrade --apply` would canonicalise:

```python
# TODO refactor this once @sara is back            →  # TODO(@sara) refactor this once is back
# TODO: PROJ-123                                   →  # TODO(JIRA(PROJ-123))
# TODO @marcus: swap the KDF                       →  # TODO(@marcus): swap the KDF
# TODO drop this once the cache is wired up        →  # TODO(@marcus.boerger) drop this once the cache is wired up
```

---

## 23. Appendix — Out-of-scope features (deliberately deferred)

- LLM-suggested ref completion or auto-classification.
- LLM-assisted blame-mapping or username inference.
- Heavy GUI dashboards.
- IDE-level refactor support (rename file → rewrite all `DOCREF`s) — partial
  in v0.3.
- Server-side metrics on which refs are clicked.
- Anti-targets in coupled-change ("must NOT have changed").
- Rewriting git history with `coderef upgrade` (the codemod edits the
  working tree only; commits are the user's choice).
- **AST-aware codemods.** `coderef upgrade` (§11) is *intentionally
  regex-only* — the rewrites it performs are bounded edits inside
  source-code comments, not refactorings of the surrounding language.
  AST-level rewriting (rename a function across a codebase, restructure
  a class hierarchy) is the job of tools like
  [`jscodeshift`](https://github.com/facebook/jscodeshift),
  [`ast-grep`](https://ast-grep.github.io),
  [`comby`](https://comby.dev), or language-server "rename symbol"
  refactors. coderef's lane is comment markers and the URLs/paths/IDs
  they contain; adding AST awareness would double the surface area for
  a use case adjacent tools already nail.

### 23.1 The cross-repo non-goal

`coderef`'s only cross-repo mechanism is `git submodule` pass-through
(§6.4). We deliberately do **not** support:

- Linked-repo manifests (declarative "this URL is a virtual root in
  the workspace").
- Remote fetch-and-verify for arbitrary git URLs.
- Cross-repo reference graphs / linkbacks beyond what submodules give us.
- Coupled-change targets that resolve into unfetched remote repos.

The reasoning, stated explicitly because it's load-bearing on what
*won't* be added later: **shadow-manifest tooling enables an org-setup
that should have been a monorepo or a submodule structure in the first
place.** Teams that find themselves wanting "linkedRepos: [...]" have
an organisational problem (split repos that should have stayed
together, or weren't set up with submodules at the boundary). Adding
machinery here would paper over that, encourage more of the same, and
introduce auth/branch-coherence/offline-mode failure modes that `git`
already solves correctly when used as designed.

For teams that genuinely have multiple independent repos that need to
coordinate, `coderef`'s position is: keep cross-repo references as
URL-style (`JIRA(...)`, `https://...`) — they verify via the standard
HTTP verifier (§13.3) like any other link. Coupled-change inside that
multi-repo setup is the team's coordination problem, not a tooling
problem `coderef` should claim to solve.
