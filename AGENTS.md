# Agent instructions

Instructions for any AI agent working in this repository (Claude Code, Cursor,
Aider, etc.). Treat these as hard requirements, not suggestions.

## Markdown table formatting

All markdown tables in this repository **must have vertically-aligned columns**.
Every cell in a column is padded with spaces to the width of the widest cell in
that column, so header, separator, and body rows line up character-for-character.
Preserve alignment markers in the separator row (`:---`, `---:`, `:---:`).

Example — required form:

```markdown
| Column A             | Column B   | Column C |
| -------------------- | ---------- | -------- |
| short                | abc        | 1        |
| a much longer value  | def        | 22       |
```

Example — forbidden form:

```markdown
| Column A | Column B | Column C |
|---|---|---|
| short | abc | 1 |
| a much longer value | def | 22 |
```

Tables inside fenced code blocks (` ```jsonc … ``` `, ` ```python … ``` ` etc.)
are not markdown tables and must be left alone.

**After any edit that touches a markdown table, re-align it.** For multi-table
sweeps, run [`tools/align-md-tables.py`](./tools/align-md-tables.py):

```sh
python3 tools/align-md-tables.py DESIGN.md README.md
```

The script walks the file, identifies tables outside fenced code blocks, and
pads cells to per-column maximum width without disturbing prose or code.

**Local pre-commit hook.** [`.pre-commit-config.yaml`](./.pre-commit-config.yaml)
wires the aligner (plus `cargo fmt --check` and `cargo clippy -D warnings`)
into the `pre-commit` framework. Install once:

```sh
pip install pre-commit && pre-commit install
```

After that, every `git commit` runs the aligner over changed `.md` files
and rejects commits that would have failed the docs-hygiene CI job. The
same `pre-commit` framework drives the consumer-facing hooks declared in
[`.pre-commit-hooks.yaml`](./.pre-commit-hooks.yaml), but those serve a
different audience — installers of `coderef`, not contributors *to* it.

## Source of truth

`DESIGN.md` is the working specification for `coderef`. When editing code or
extending features, treat the design document as authoritative. When the design
and the code disagree, surface the conflict in your response rather than
silently choosing one.

## Conservative edits

- Do not create new files unless explicitly requested.
- Do not introduce dependencies, abstractions, or speculative features the
  design does not call for.
- When iterating on a feature, edit the relevant existing section rather than
  appending a new one at the end.
- Prefer `Edit` over `Write` for surgical changes; reserve `Write` for full
  rewrites and brand-new files.

## Project conventions

- License: Apache-2.0.
- Publisher namespace: `helly25`.
- The repo is a Rust workspace (`crates/coderef-core`, `crates/coderef-cli`)
  plus a TypeScript VSCode extension (`extension/`) and an npm binary wrapper
  (`npm/coderef/`). See `DESIGN.md` §4.3 for the layout.
- Pre-commit hooks for the repo itself follow the `vscode-iwyu` sibling
  project's style.
