# CI

`ci.yml` runs on every push to `main` and every pull request. Five
jobs in parallel:

| Job             | What it does                                                                                                                                                                      |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `rust`          | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo check`, `cargo test`, `cargo build --release`, sanity-runs `coderef --version`.                                           |
| `extension`     | `tsc --noEmit -p extension/`. ESLint runs but is non-blocking at v0.0.0 (no config shipped yet).                                                                                  |
| `npm-wrapper`   | `npm install` in `npm/coderef/` (runs the postinstall stub), then expects `bin/coderef.js` to exit 127 (placeholder behaviour).                                                   |
| `schema`        | Validates `schema/coderef.schema.json` parses, then validates `examples/*.coderef.jsonc` against it via `tools/validate-config.py`.                                               |
| `docs`          | Re-runs the table aligner and `git diff --exit-code` to enforce idempotency. Verifies every `§N.M` cross-ref in `DESIGN.md` resolves to a real header. Checks code-fence balance. |

If any of these fail on a PR, that's the signal to look at the diff
before approving.

Future workflows (post-v0.1):

- `release.yml` — `cargo dist` cross-compiled binaries on tag push.
- `marketplace.yml` — `vsce publish` for the VSCode extension.
- `npm-publish.yml` — `npm publish` for `@helly25/coderef`.
