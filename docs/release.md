# Releasing coderef

Three independent distribution channels, each tied to a credentialed
external service. The release workflow plumbs them but the actual
triggers stay manual on purpose — every channel is publicly visible
and the consequences of a bad publish are slow to reverse.

Always do the channels in this order: **GitHub Release → npm wrapper
→ VSCode marketplace**. The npm wrapper downloads from the GitHub
Release at install time; the marketplace extension bundles the WASM
locally so it doesn't depend on the others, but a misordered publish
makes the npm wrapper unusable until the GitHub Release exists.

## 0. Pre-flight (every release)

```bash
git checkout main && git pull --ff-only
git status                                       # must be clean
cargo test --all-features --locked               # all unit + integration
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
./target/release/coderef doctor .                # 0 errors
./target/release/coderef check .                 # 0 broken
```

Make sure every package agrees on the version:

```bash
grep -rn '"version": "[0-9]' --include='package.json' .
grep -n 'version    = "[0-9]' Cargo.toml
grep -n 'html_root_url' crates/coderef-core/src/lib.rs
```

All five should show the same `MAJOR.MINOR.PATCH` (no trailing `-rc`
unless this is a pre-release).

## 1. GitHub Release (CLI binaries)

Triggers the cross-build in `.github/workflows/release.yml`. Produces
the four platform tarballs the npm wrapper downloads at install time.

```bash
git tag v0.2.1 -m "coderef v0.2.1"
git push origin v0.2.1
```

Then watch `gh run watch --branch v0.2.1` until the `release` job
shows green. Verify:

```bash
gh release view v0.2.1 --json assets --jq '.assets[].name'
```

Expect 8 entries (4 platforms × 2 files: archive + `.sha256`).

## 2. npm wrapper

Needs `NPM_TOKEN` either via `~/.npmrc` (`npm login`) or an env var.
The wrapper's `install.js` downloads from the GitHub Release created
in step 1, so this step is unusable until step 1 is green.

Test the install locally first against the fresh release:

```bash
cd /tmp && rm -rf coderef-npm-test && mkdir coderef-npm-test
cd coderef-npm-test && npm init -y >/dev/null
# Before publish, point at the local path:
npm install --no-audit --no-fund "$REPO_ROOT/npm/coderef"
./node_modules/.bin/coderef --version            # must print engine version
```

Publish:

```bash
cd "$REPO_ROOT/npm/coderef"
npm publish --access public                      # public scope @helly25/
```

Verify on npm:

```bash
npm view @helly25/coderef version
```

## 3. VSCode marketplace (extension VSIX)

Needs `VSCE_PAT` from Azure DevOps (https://dev.azure.com →
User Settings → Personal Access Tokens → Marketplace › Manage scope).
This channel is independent of the CLI release — the VSIX bundles
its own WASM via `scripts/bundle-wasm.cjs`.

```bash
cd "$REPO_ROOT/extension"
npm run package                                  # produces coderef-<ver>.vsix
ls -la coderef-*.vsix                            # sanity-check the size
vsce publish                                     # reads VSCE_PAT from env
```

Verify on the marketplace:

```bash
vsce show helly25.coderef
```

## Tag → publish window: minutes, not hours

The npm wrapper's install fetches `https://github.com/helly25/coderef/releases/download/v<X>/...`.
If npm publishes before the GitHub Release is fully populated,
`npm install -g @helly25/coderef@<X>` fails with a 404 download error
until the assets land. Don't let users hit that window — finish all
three channels in one sitting, or hold off on the npm publish until
step 1's release page is fully populated.

## Rollback

- **GitHub Release**: `gh release delete v<X>` + `git tag -d v<X> &&
  git push --delete origin v<X>`.
- **npm**: a published version is permanent (npm rejects republishing
  the same number). Use `npm deprecate '@helly25/coderef@<X>' "reason"`
  and publish a patch with the fix.
- **VSCode marketplace**: `vsce unpublish helly25.coderef@<X>` works
  but is visible in the extension's history.

## Pre-release / rc

For a release candidate, use the SemVer pre-release form: tag as
`v0.2.0-rc1`, bump all five `version` fields to `0.2.0-rc.1` (note
the `.` before the number for npm/SemVer; Cargo accepts both). Test
publish to a private npm scope or skip npm entirely for rcs.
