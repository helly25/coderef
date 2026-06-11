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

## UTF-16 vs UTF-8 offset mismatch in providers.ts (closed — codified)

- **What was verified**: hover, document-link, and the
  `coderef.explainReference` command translate between VSCode's
  UTF-16 positions and the engine's UTF-8 `byte_start` / `byte_end`
  rather than comparing them naively. Previously they used
  `document.offsetAt()` (UTF-16) against engine byte offsets
  (UTF-8); a single em-dash earlier in the file misaligned all
  subsequent lookups.
- **Replaced by**: `extension/src/textOffset.ts` (helper) +
  `extension/src/textOffset.test.ts` (unit tests covering em-dash,
  emoji surrogate pairs, CJK round-trip, off-by-one boundary cases)
  + a new runtime test
  `extension/src/test/runtime/extension.test.ts:hover-resolves-the-SECOND-TODO-bob`
  that hovers TODO(@bob) after an em-dash + emoji in the fixture.
  Any regression to UTF-16-vs-UTF-8 comparison fails the hover
  lookup in that test.
- **Tracked**: closed by the PR adding `extension/src/textOffset.ts`.

---
