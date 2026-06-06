# Schema

`coderef.schema.json` is the JSON Schema (draft 2020-12) for
`.coderef.jsonc` / `.config/coderef.jsonc` config files.

It's the authoritative reference for what fields exist, their types,
and their version slots. The narrative reference is
[`../DESIGN.md`](../DESIGN.md) §7; the schema is the machine-readable
form of the same surface.

## Use in a config file

Add `$schema` at the top of your `.coderef.jsonc`:

```jsonc
{
  "$schema": "https://helly25.github.io/coderef/schema/v1.json",
  ...
}
```

VSCode (and any editor that respects `$schema`) will load it for
autocomplete, lint, and hover descriptions. Until the schema is
published at the canonical URL, point `$schema` at a relative path
during development:

```jsonc
{
  "$schema": "./schema/coderef.schema.json",
  ...
}
```

## Validating manually

The schema validates with any draft-2020-12 validator. Two common
choices:

```sh
# Node + ajv
npx -y ajv-cli validate -s schema/coderef.schema.json -d examples/minimal.coderef.jsonc

# Python
python3 -m pip install --user check-jsonschema
check-jsonschema --schemafile schema/coderef.schema.json examples/minimal.coderef.jsonc
```

A CI pipeline that exercises the schema against the example configs
lands with the CI plumbing commit (see DESIGN.md §19 follow-up
sequence).

## Versioning

Major versions of the schema move in lockstep with major versions of
the runtime. `v1.json` covers the v0.1–v0.4 design surface (post-v0.4
features are tagged in field descriptions but the schema does not
reject them — forward-compat). When the runtime hits v1.0 and the
schema stabilises, this file is frozen at `v1.json` and breaking
schema changes move to `v2.json`.

## Forward compatibility

The top-level schema enforces `additionalProperties: false` to surface
typos. Pattern-level config blocks (`scope`, `actions`, etc.) similarly
enforce closed structures so that an unknown field at any level is
caught as a doctor warning, not silently ignored. New fields land via
schema PRs alongside the feature that introduces them.

Variables, blame user mapping, and integrity check overrides have
open shapes (`additionalProperties: true`) because their keys are
user-chosen.
