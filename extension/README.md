# coderef

[`coderef`](https://github.com/helly25/coderef) turns regex-defined references in your source — filenames, ticket IDs, RFC numbers, doc anchors, coupled-change markers — into clickable, hoverable, verifiable links. This extension is the VSCode integration: a `DocumentLinkProvider`, a `HoverProvider`, and a references browser, all backed by the same engine that powers the `coderef` CLI.

The engine ships as WASM inside the VSIX, so the in-editor experience and the CLI never diverge — they parse `.coderef.jsonc` the same way and resolve the same regexes against the same comment-region rules.

## Features

1) **Click-through references.** Anything matched by a pattern in your `.coderef.jsonc` becomes a `DocumentLink`. Ticket IDs jump to your tracker, file paths open the file, doc anchors open the doc at the right heading.

2) **Hover preview.** Hover over a reference to see which pattern matched, the resolved target URL, and the pattern's category (`files` / `people` / `tickets` / `standards` / `urls` / `coupled-change` / `other` per DESIGN §5.7).

3) **References browser.** Activity-bar tree view (look for the link icon) grouping every reference in the workspace by category, then by file. Click-to-jump. Live-updates via filesystem watcher; refresh button in the view header.

4) **CLI bridge.** The extension shells out to the native `coderef` CLI binary for write-mode subcommands (`check`, `doctor`, `changes`, `commit-msg`) and for HTTP verification. The binary ships separately via GitHub Releases / npm; the extension keeps working without it for read-only features.

5) **Doctor diagnostics.** Pattern-level checks like `category.unset`, `category.tooBroadOther`, `category.mismatch`, `anchor.styleMismatch`, `coupled.composableTypo`, `commitMessage.allDisabled`, etc. — run `coderef doctor` from the CLI to surface them.

6) **Anchor verification.** `DOCREF(/path.md#section)` resolves the heading slug against the target file (slugifiers: `github` / `pandoc` / `gitlab` / `hugo` / `mkdocs-material`). Levenshtein-1 suggestions on miss.

7) **Coupled-change checks.** `IfChange ... ThenChange(target)` blocks with Shape A/B/C support (anchor, label, glob targets, composable IDs like `IfChange(JIRA(PROJ-1234))`). The CLI's `coderef changes` enforces the three-pass algorithm.

## Requirements

A `.coderef.jsonc` (or `.coderef.json`) config in your workspace root, or in `.config/`. The repository's [README](https://github.com/helly25/coderef#readme) covers the schema; [`DESIGN.md`](https://github.com/helly25/coderef/blob/main/DESIGN.md) is the canonical reference.

For the write-mode subcommands and HTTP verification you also need the native `coderef` CLI on `$PATH`. Install via:

```sh
npm install -g @helly25/coderef
# or download a release: https://github.com/helly25/coderef/releases
# or build from source: cargo install --path crates/coderef-cli
```

## Extension Settings

- `coderef.enabled` (default `true`) — master switch.
- `coderef.configPath` (default `""` = auto-detect) — explicit path to your `.coderef.jsonc`, relative to the workspace root.

## Commands

- `coderef: Explain reference at cursor` — describes which pattern matched at the cursor position, the resolved target, and the category.
- `coderef: Refresh references browser` — re-scans the workspace and rebuilds the tree view.

## Related

- **CLI**: [helly25/coderef](https://github.com/helly25/coderef) — `coderef check`, `coderef doctor`, `coderef changes`, `coderef commit-msg`, `coderef patterns`, `coderef explain`.
- **npm wrapper**: [`@helly25/coderef`](https://www.npmjs.com/package/@helly25/coderef) — downloads the platform binary at install time.
- **Changelog**: see [`CHANGELOG.md`](./CHANGELOG.md) for extension-scoped changes; the repo's [`CHANGELOG.md`](https://github.com/helly25/coderef/blob/main/CHANGELOG.md) covers the whole project.

## License

Apache License 2.0. See [`LICENSE`](./LICENSE).
