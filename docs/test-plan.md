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
