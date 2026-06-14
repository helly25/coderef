# Changelog

All notable changes to coderef are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/spec/v2.0.0.html).

## Unreleased

## v0.2.1 — 2026-06-14

### Highlights

- **`kind: "block"`** — source-level `DO NOT COMMIT` / `NOCOMMIT` /
  `DONOTMERGE` guards. The match itself is the diagnostic; `coderef
  check` exits 1 on the first hit, blocking the pre-commit hook
  (PR #26).
- **Pattern categories (DESIGN §5.7)** — `files` / `people` / `tickets`
  / `standards` / `urls` / `coupled-change` / `other` + user-defined.
  Doctor surfaces `category.unset`, `category.tooBroadOther`, and
  scan-dependent `category.mismatch`. `coderef patterns --by-category`
  groups in DESIGN-order display (PR #28, #41).
- **Markdown anchor verification (DESIGN §6.3)** — `DOCREF(/path.md#section)`
  verifies the anchor against the target file's heading slugs.
  Slugifiers: `github` (default), `pandoc`, `gitlab`, `hugo`,
  `mkdocs-material`. Pandoc explicit `{#id}` overrides honoured under
  every slugifier. Suggests Levenshtein-1 hits on miss (PR #29, #36).
- **`coderef commit-msg` linter (DESIGN §16.1.1)** — runs the pattern
  engine over a commit-message file (or stdin), with per-pattern
  `scope.commitMessage` opt-in / `"required"` enforcement (PR #30).
- **`coderef changes` IfChange/ThenChange verifier (DESIGN §10)** —
  three-pass coupled-change algorithm with full Shape A / B / C
  support. ThenChange targets: bare file, `path:N` / `path:N-M`,
  `path:label-name`, `path#anchor`, glob (`path/*.md{any|all}`). Shape
  C composable ids (`IfChange(JIRA(PROJ-1234))`) resolve through the
  reference engine so blocks in different files coalesce. NoVerify
  escape hatch + 11 integration tests (PRs #31, #37, #38, #39, #40).
- **References browser** — activity-bar tree view in the VSCode
  extension. Category-first grouping (DESIGN §5.7.3 display order),
  click-to-jump, live updates via file-system watcher (PR #34).
- **Doctor diagnostics shipped** — `category.unset`,
  `category.tooBroadOther`, `category.mismatch`, `anchor.skippedExt`,
  `anchor.styleMismatch`, `commitMessage.allDisabled`,
  `commitMessage.ifchangeMisconfigured`, `coupled.composableTypo`
  (PR #28, #35, #41).
- **CI infrastructure** — bzl-style `done` aggregator gate with a yq
  self-check that fails CI if any newly-added job isn't wired into the
  required-check list (PR #32, #33). Synced to mbo, bashtest, proto,
  vscode-iwyu, mbo-tools.

### Removed

- **x86_64-apple-darwin (Intel Mac) release binary.** Apple stopped
  shipping Intel hardware in 2023 and GitHub's `macos-13` runner queue
  was the slowest in the release matrix (PR #43). The npm wrapper now
  emits an `unsupported platform/arch` error for `darwin x64` instead
  of trying to download a missing asset. Apple Silicon Macs (the
  `aarch64-apple-darwin` binary) are unaffected.

### Fixed

- **UTF-16 ↔ UTF-8 offset translation** in the VSCode extension's
  hover and DocumentLink providers. Files containing em-dashes,
  emoji, or CJK characters no longer shifted ref positions by N
  multi-byte characters (PR #27).

### Workspace stats

- Test count grew from 209 → **368** (Rust + integration).
- 7-job CI matrix (`rust` / `wasm` / `extension` / `npm-wrapper` /
  `schema` / `docs` / `done`); branch protection requires only `done`.

### Deferred to v0.3

- `commitMessage.requiredNeverFires` doctor check (needs git-log
  corpus plumbing).
- Strict `{all}` glob semantics (needs workspace-wide enumeration).
- `{soft}` glob flag (warning severity).
- `Label('name') ... EndLabel` compat form (per-pattern label config).
- References-browser longer tail: scan modes, Mine/Drifted filters,
  Copy-as-Markdown, exportJson.
- Multi-target references, network profiles, `coderef upgrade`
  codemod, visual config editor, external-URL anchor verification.

## v0.2.0 — yanked, never shipped

The original release run (workflow 27498341045) was cancelled while
the `x86_64-apple-darwin` build sat in the `macos-13` GitHub Actions
queue. The matrix was reshaped to drop Intel Mac support (PR #43, #44)
and the actual release ships as **v0.2.1**. The `v0.2.0` tag exists on
the remote as an orphan ref with no GitHub Release, no published
binaries, and no presence on npm or the VSCode Marketplace.

## v0.1.0 — 2026-06-07

Minimum viable foundation:

- Pattern engine + JSONC config loader.
- HTTP verifier (HEAD + GET fallback, configurable timeout).
- VSCode extension with DocumentLinkProvider + HoverProvider,
  in-process WASM-shared core so editor and CLI never diverge.
- CLI subcommands: `config show`, `list`, `check`, `doctor`,
  `patterns`, `explain`, `help`.
- Pre-commit hook plumbing.
- npm wrapper (`@helly25/coderef`) that downloads platform binaries
  from GitHub Releases on install.
- Distribution: GitHub Releases (CLI binaries) → npm wrapper →
  VSCode marketplace, ordered (`docs/release.md`).
