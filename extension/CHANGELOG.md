# Change Log

# [0.2.1]

* Initial publication to the VSCode Marketplace.
* DocumentLinkProvider: click-through navigation for any reference matched by a configured pattern — filenames, JIRA tickets, RFCs, doc anchors, etc.
* HoverProvider: inline preview showing which pattern matched, the resolved URL, and the pattern's category.
* References browser: activity-bar tree view with DESIGN §5.7.3 category-first grouping; live-updates via filesystem watcher.
* Bridges to the native `coderef` binary for write-mode subcommands (`check`, `doctor`, `changes`, `commit-msg`) and HTTP verification.
* UTF-16 ↔ UTF-8 offset translation: files containing em-dashes, emoji, or CJK characters now show reference positions correctly (prior versions shifted by N multi-byte characters).
* In-process WASM core (`@helly25/coderef-core-wasm` bundled in the VSIX) so editor pattern resolution matches the CLI exactly.

The companion CLI ships from the [coderef](https://github.com/helly25/coderef) repo's GitHub Releases. The full feature changelog is in [`CHANGELOG.md`](https://github.com/helly25/coderef/blob/main/CHANGELOG.md) at the repo root.
