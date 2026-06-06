# coderef

Regex-driven references in source code — resolved, click-opened, and
verified identically from VSCode and from CI. A `.coderef.jsonc` config
declares the patterns; the same engine runs inside the editor (via WASM,
in-process) and inside the Rust CLI binary (for `pre-commit` and CI).

The working specification is [`DESIGN.md`](./DESIGN.md); this README is
the elevator pitch.

```python
# Click-to-open in VSCode; verified by pre-commit; same regex engine in both.
#
# TODO(@marcus): swap to argon2id — JIRA(SEC-87) tracks it.
# DOCREF(/docs/security/hashing) explains the threat model.
# RFC(8259) is the JSON spec.

# IfChange / ThenChange enforces co-modification across files:
# IfChange
# Label('hash-params')
HASH_PARAMS = {"memory_kib": 19456, "iterations": 2, "parallelism": 1}
# EndLabel
# ThenChange(/docs/security.md:hash-params, /tests/test_auth.py:hash-params)
```

Patterns declare their regex, the URL or local-file they resolve to,
how they're verified, what category they belong to, and what should
happen on hover. Same regex flavour, same semantics, in both hosts.

## Planning horizon

| Version  | Theme                                                                                                                                                 |
| -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| **v0.1** | Minimum viable: pattern engine + HTTP verifier + click-to-open. WASM-shared core (no engine divergence between editor and CLI).                       |
| **v0.2** | Coupled-change (`IfChange`/`ThenChange`) + categories + references browser + commit-message linting + anchor verification for in-repo Markdown.       |
| **v0.3** | Multi-target references + full network profiles + auto-upgrade codemod (`coderef upgrade`) + visual config editor + external-URL anchor verification. |
| **v0.4** | LSP server mode + composable coupled-change IDs + git-submodule pass-through.                                                                         |

Anything past v0.4 is deliberately not planned in detail (see
[`DESIGN.md` §19.5](./DESIGN.md)). The full per-version scope is in
[`DESIGN.md` §19](./DESIGN.md).

## Repo layout

- [`DESIGN.md`](./DESIGN.md) — the working spec. Source of truth.
- [`AGENTS.md`](./AGENTS.md) — conventions every contributor (human or
  AI agent) follows. Includes the markdown table-alignment rule and a
  pointer to the script that enforces it.
- [`CLAUDE.md`](./CLAUDE.md) — entry-point for Claude Code; redirects
  to `AGENTS.md`.
- [`tools/align-md-tables.py`](./tools/align-md-tables.py) — the
  table-alignment script.
- [`LICENSE`](./LICENSE) — Apache 2.0.

The workspace, JSON Schema, extension scaffold, and CI plumbing land
in focused follow-up merges before v0.1's first feature.

## What `coderef` is *not*

- Not a TODO tracker — it resolves the references you put in source,
  it doesn't manage them.
- Not a tag-uniqueness enforcer ([`tagref`](https://github.com/stepchowfun/tagref)
  already covers that niche).
- Not a generic markdown link checker
  ([`lychee`](https://github.com/lycheeverse/lychee) covers that).
- Not an AST-aware refactoring tool — `coderef upgrade` is regex-only
  by design; for AST work, use jscodeshift / ast-grep / comby.

See [`DESIGN.md` §22](./DESIGN.md) for the full out-of-scope list and
the reasoning.

## License

Apache License 2.0. See [`LICENSE`](./LICENSE).
