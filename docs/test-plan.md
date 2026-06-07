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

## VSCode extension end-to-end (closed — codified)

- **What was verified**: extension activates in a workspace
  containing `.coderef.jsonc`; DocumentLinks resolve for planted
  TODO(@user) markers; hover returns content with pattern id +
  description; `coderef.explainReference` is registered.
- **Replaced by**: `extension/src/test/runtime/extension.test.ts`,
  run via `@vscode/test-electron` (script: `npm run test-runtime`).
  CI runs the same path in the `VSCode extension (TS + VSIX)` job.
- **Tracked**: closed by the PR adding `@vscode/test-electron`.

---

## UTF-16 vs UTF-8 offset mismatch in providers.ts

- **What was verified**: surfaced by writing the runtime tests for
  the above entry — the hover provider's position-vs-ref-byte-range
  comparison uses `document.offsetAt()` (UTF-16 code units on
  VSCode's side) against the engine's `byte_start` (UTF-8 bytes).
  For ASCII-only content they coincide; non-ASCII characters
  earlier in the file shift the byte offset relative to UTF-16,
  causing hover lookups to miss.
- **How it was verified**: runtime test failed when the fixture
  contained an em-dash; passed when the fixture was made
  ASCII-only. The DocumentLinkProvider has the same shape
  (`document.positionAt(r.byte_start)`); links are still drawn but
  the position is off by N (where N is the count of multi-byte
  characters before the ref). User would see clickable links
  shifted from the actual text on documents containing emoji /
  diacritics / Asian scripts / etc.
- **What test should replace this note**: a unit test on a small
  conversion helper (`byteOffsetToVscodePosition(doc, byteOffset)`
  and the inverse) using a fixture document with multi-byte
  characters before known reference positions. Plus updating the
  runtime fixture to include non-ASCII and asserting hover still
  resolves.
- **Tracked**: follow-up PR.

---
