# docs/test-plan.md — the one-shot ledger

Holding pen for manual verifications that haven't been codified as
committed tests yet. See `DESIGN.md` §17.2 for the discipline.

Every entry has the same shape:

```
## <short title>

- **What was verified**: …
- **How it was verified**: …
- **What test should replace this note**: …
- **Tracked**: <commit / PR / issue link>
```

Empty is good. A long list here is a code-smell signal that some test
needs writing.

---

## Schema-load smoke (already codified — keep as the template)

- **What was verified**: that `schema/coderef.schema.json` parses as
  valid JSON and that `examples/minimal.coderef.jsonc` validates
  cleanly against it.
- **How it was verified**: ran `python3 tools/validate-config.py
  schema/coderef.schema.json examples/minimal.coderef.jsonc` locally
  during the schema PR (commit `c7c4fe9`).
- **Replaced by**: `.github/workflows/ci.yml` job `schema` — runs the
  same validation on every push.
- **Tracked**: commits `c7c4fe9`, `cfc9263`.

This entry is kept as documentation of the template. New entries go
below.

---

## VSCode extension end-to-end (DocumentLink + Hover under Extension Host)

- **What was verified**: with the v0.1 extension PR, that activating
  the extension in a workspace containing a `.coderef.jsonc` results
  in references being clickable (DocumentLink) and producing a hover
  tooltip (Hover) on every match. Both URL and local-path kinds were
  verified manually by spawning the Extension Host (`F5` in the dev
  workspace), opening a test file with planted `TODO(@user)` and
  `DOCREF(/docs/x.md)` markers, and observing the link decoration +
  hover popup.
- **How it was verified**: manual smoke test, one-shot. The unit
  tests in `extension/src/providers.test.ts` cover `linkTargetFor`
  (the pure URL/local resolution function) via a mocked `vscode`
  module, and the WASM smoke in CI covers the engine. What is *not*
  covered by code yet: the actual VSCode runtime wiring — provider
  registration, document open/change cache invalidation, config-file
  watcher, error paths when the WASM engine fails to load.
- **What test should replace this note**: an `@vscode/test-electron`
  integration test that:
    1. Boots a real VSCode instance pointed at a fixture workspace.
    2. Waits for the extension to activate.
    3. Opens a fixture document, queries
       `vscode.commands.executeCommand('vscode.executeLinkProvider', uri)`
       and asserts the returned links match the planted refs.
    4. Queries `vscode.executeHoverProvider` and asserts the hover
       contains the pattern id + target.
- **Tracked**: extension PR + a follow-up issue
  `extension: wire @vscode/test-electron for runtime integration tests`.

---
