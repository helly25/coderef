# Changelog

All notable changes to coderef are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- **`references.tooManyNodes` + `references.uncategorisedSpike`
  doctor diagnostics** (DESIGN §14.7.3). Two new scan-dependent
  Info-severity checks for the references-browser tree.
  `references.tooManyNodes` fires when any pattern matches more
  than 1000 references workspace-wide, or any single file contains
  more than 1000 references — both signal that the references
  browser will truncate the corresponding tree branch and a more
  granular grouping is in order. `references.uncategorisedSpike`
  fires when more than 10% of references land in the `other`
  fallback category — suggests declaring `category` explicitly on
  more patterns.
- **References-browser exportJson command** (DESIGN §14.7 v0.3 long
  tail). New `coderef.references.exportJson` command pops a save
  dialog and writes the current reference set to disk as a stable
  schema-versioned JSON document. Schema `1` includes a header
  (generated_at, engine version, totals) plus one entry per
  reference with its resolved category baked in, so downstream
  tooling can group/filter without re-reading the config. Entries
  are sorted by `(file, byte_start)` so the output is diffable
  across runs. Surfaced as a save-as-icon button in the
  references-browser view title bar (between the Copy-as-Markdown
  and Refresh icons).
- **References-browser Copy-as-Markdown command** (DESIGN §14.7 v0.3
  long tail). New `coderef.references.copyAsMarkdown` command shows
  up as a clippy-icon button in the references-browser view title
  bar. Click it (or run from the command palette) and the current
  reference set lands in your clipboard as a category-grouped
  Markdown document, suitable for pasting into a PR description, a
  design doc, a ticket comment — anywhere the team is already
  reading Markdown. Format: one section per category (display-order
  sorted), each section subdivided per file, each file as a bullet
  list of `path:line` — `[pattern] matched-text` → target lines.
  Backticks in matched text are escaped so the inline-code spans
  don't break.
- **`commitMessage.requiredNeverFires` doctor diagnostic** (DESIGN
  §16.1.1). A pattern declared `scope.commitMessage: "required"`
  whose regex doesn't match any commit in the host-supplied corpus
  is flagged at `Warning` severity. `coderef doctor` now walks
  `git log -n 200 --format=%B` (NUL-separated bodies) and feeds the
  result to a new
  `run_doctor_with_workspace_and_commit_corpus(root, cfg, msgs)`
  entry point in `coderef-core`. The original
  `run_doctor_with_workspace(root, cfg)` stays as a 2-arg backward-
  compatible alias that passes `None` for the corpus. With no
  corpus (host couldn't reach git, not a repo, or the log is
  genuinely empty), the check is silently skipped rather than
  flagging every required pattern as never-fired.
- **`label.*` doctor diagnostics** (DESIGN §10.3). Three new
  scan-dependent checks fire when at least one `kind: "ifchange"`
  pattern is configured: `label.duplicateInFile` (Error — two labelled
  regions in the same file share an id; resolution is
  non-deterministic), `label.unused` (Info advisory — labelled region
  with no peer block and no `ThenChange(path:label-name)` reference),
  and `label.ambiguousName` (Error — name is purely numeric or matches
  `N-M`, colliding with line/range parsing in `ThenChange(path:line)`
  targets). The remaining DESIGN §10.3 diagnostics
  (`label.orphanOpen`/`Close`) are explicitly compat-only and pair
  with the per-pattern `patterns.<id>.label` config in a follow-up
  PR.
- **Strict `{all}` glob semantics** in `ThenChange` (DESIGN §10.2).
  `IfChange ... ThenChange(/docs/*.md{all})` now enforces that *every*
  workspace file matching the glob is touched by the diff, not just
  "at least one" as the v0.2-shipped lax semantics required. Powered
  by a new `enumerate_workspace_files` helper in `coderef-core` plus
  a new `workspace_files: Option<&[String]>` parameter on
  `verify_changes_composable`. The CLI's `coderef changes` walks
  the workspace with the same `.gitignore` + config `ignore[]`
  semantics that block-scanning uses, so the file universe stays
  consistent. Callers without a workspace handy (WASM in-editor
  path, isolated unit tests) pass `None` and `{all}` falls back to
  the lax v0.2 any-mode behaviour. Empty match set is a vacuous
  pass — a stale-glob doctor check is the right surface for that,
  not a coupled-change violation.
- **`{soft}` glob flag** on `ThenChange` glob targets (DESIGN §10.2
  severity modifier). A `ThenChange(/docs/*.md{soft})` target whose
  matched-and-changed count is zero still surfaces a violation, but
  with `Severity::Warning` — `ChangesReport::passed()` ignores
  warnings, so `coderef changes` exits 0. The flag is comma-combinable
  with the existing `{any}` / `{all}` mode flags: `{soft,all}` (and
  `{all,soft}`) mean "every matched file should change, but if not,
  warn rather than fail". The previous single-flag-only parser is
  generalised to a comma-separated flag set inside the braces.
- `GlobFlag` enum split into `GlobMode { Any, All }` (mutually
  exclusive match-mode) and `GlobFlags { mode, soft }` (struct that
  groups the mode with the orthogonal `soft` severity bit). All
  `Target::FileGlob` sites now carry `flags: GlobFlags` instead of
  `flag: GlobFlag`. `format_target` omits the brace suffix entirely
  when the flags are at their default (any-mode, not soft) so
  violation messages don't sprout a redundant `{any}` on every
  default glob.
- `Violation` and `ViolationReport` carry a `severity: Severity`
  field. JSON consumers that ignore unknown fields keep working
  (additive change); consumers that depend on `severity` get
  `"warning"` for soft glob misses and `"error"` for everything
  else.
- CLI `coderef changes --report text` prefixes warning violations
  with `[warn/missing-target]` instead of the plain
  `[missing-target]`; the trailing summary line splits `violation(s)`
  into `error(s)` and `warning(s)` so the reader can see at a glance
  whether the run failed because of a hard mismatch or only flagged
  soft ones.
- `split_targets` (the comma-splitter for the `ThenChange(...)` arg
  list) now respects `{...}` brace boundaries — commas inside a
  flag set don't split into separate targets.
- **`Label('name') ... EndLabel` marker recognition** in the IfChange
  block parser (DESIGN §10.2 / §10.3 compat form). The compat markers
  are now parsed as alternative open/close pairs alongside the
  canonical `IfChange / ThenChange` — they produce the same internal
  `IfChangeBlock` representation with the captured id, so a
  `ThenChange(path:label-name)` elsewhere can target either form's
  labelled region. Cross-form pairing is allowed
  (`Label('foo') ... ThenChange(targets)`, `IfChange('bar') ... EndLabel`)
  so codebases mid-migration aren't forced into one spelling. Per-pattern
  config (DESIGN §10.3 `patterns.<id>.label`) and the
  `label.*` doctor diagnostics (orphanOpen, orphanClose, duplicateInFile,
  unused, ambiguousName) remain v0.3+ work — this PR ships the parser
  recognition only.

### Changed

- **`npm_publish.yml` workflow now triggers on tag push** instead of
  the previously-used `release: types: [published]` event. The former
  shape never fired because GitHub Actions suppresses workflows
  triggered by events caused by a workflow's own `GITHUB_TOKEN`
  (anti-recursion). The new shape adds a poll-and-wait step that
  blocks up to 30 minutes for `release.yml` to finish building and
  creating the GitHub Release, then publishes. Effect for releasers:
  pushing `vX.Y.Z` now fires all three channels (GitHub Release, npm,
  VSCode Marketplace) zero-touch — no manual `gh workflow run` for
  npm anymore.

## v0.2.2 — 2026-06-14

### Fixed

- **npm wrapper `install.js` no longer copies a script as if it were a
  binary**, fixing the infinite-recursion silent hang where the second
  `npm install -g @helly25/coderef` after a v0.2.1 install would copy
  the npm shim left by the prior install (a `#!/usr/bin/env node`
  script) into `bin/coderef`, so the wrapper spawned itself
  recursively. `install.js` now sniffs the magic bytes of any candidate
  before copying — accepts ELF, Mach-O single-arch, Mach-O Universal
  (`CA FE BA BE`, what `lipo` produces and what `/bin/ls` is on macOS),
  and PE; rejects anything beginning with `#!`.

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
