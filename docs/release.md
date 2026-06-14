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
git tag v0.2.2 -m "coderef v0.2.2"
git push origin v0.2.2
```

Then watch `gh run watch --branch v0.2.2` until the `release` job
shows green. Verify:

```bash
gh release view v0.2.2 --json assets --jq '.assets[].name'
```

Expect 8 entries (4 platforms × 2 files: archive + `.sha256`).

## 2. npm wrapper

Automated by `.github/workflows/npm_publish.yml`. The workflow fires
on `release: types: [published]` — i.e. as soon as step 1 finishes
creating the GitHub Release — so there's no window where the
wrapper exists on npm but the binaries it tries to download don't
exist yet. The job double-checks `gh release view` before calling
`npm publish` and refuses to publish into a broken state.

### One-time setup

1. **npm account + scope**: sign up at npmjs.com with username
   `helly25` (auto-creates the `@helly25` scope under which
   `@helly25/coderef` lives).
2. **2FA**: enable on the publisher account with mode
   **`Auth Only`** — the CI token then bypasses interactive OTP
   for automation. (Auth-and-Writes mode forces OTP prompts and
   won't work in CI.)
3. **Mint the token**: Profile → Access Tokens → Generate New
   Token → **Granular Access Token**. Permission: **Read and
   write**. Packages and scopes: **`@helly25`**. Expiration: your
   rotation horizon (365d is a reasonable default). Copy the
   token immediately — npm shows it once.
4. **Store as repo secret**: GitHub → repo → Settings → Secrets
   and variables → Actions → New repository secret. Name
   `NPM_TOKEN`, value the token.

### Per-release flow

The workflow runs automatically on the GitHub Release-published
event from step 1 — no extra action needed for normal releases.
Watch progress at
`https://github.com/helly25/coderef/actions/workflows/npm_publish.yml`.

For ad-hoc / retro-publish (e.g. a tag pushed before this workflow
existed, or a re-publish after a transient registry failure):

```bash
gh workflow run npm_publish.yml -f tag=v<X.Y.Z>
gh run watch                                     # follow the run
```

Verify on npm:

```bash
npm view @helly25/coderef version
```

### Manual fallback

If the workflow is broken and you need to publish from a local
machine, the `NPM_TOKEN` is in keychain on the publisher's box;
retrieve and use it for a one-off:

```bash
export NODE_AUTH_TOKEN=$(security find-generic-password -a "$USER" -s NPM_TOKEN -w)
cd "$REPO_ROOT/npm/coderef"
echo "//registry.npmjs.org/:_authToken=$NODE_AUTH_TOKEN" > ~/.npmrc
npm publish --access public
rm ~/.npmrc                                      # don't leave it lying around
```

Test the install against the local checkout first if you want a
dry run:

```bash
cd /tmp && rm -rf coderef-npm-test && mkdir coderef-npm-test
cd coderef-npm-test && npm init -y >/dev/null
npm install --no-audit --no-fund "$REPO_ROOT/npm/coderef"
./node_modules/.bin/coderef --version            # must print engine version
```

## 3. VSCode marketplace (extension VSIX)

Automated by `.github/workflows/vscode_marketplace.yml`. The workflow fires
on the same tag push that drives step 1, builds the VSIX in CI (Rust
+ wasm32 + wasm-pack + Node), uploads it as a downloadable artifact,
and calls `vsce publish` using the `VSCE_PAT` repo secret. This
channel is independent of step 1 — the VSIX bundles its own WASM
via `scripts/bundle-wasm.cjs`.

### One-time setup

1. **Mint the PAT**: https://dev.azure.com → User Settings → Personal
   Access Tokens → New Token. Organization: **All accessible
   organizations**. Scopes: **Custom defined → Marketplace → Manage**
   (nothing else — least privilege).
2. **Store as repo secret**: GitHub → repo → Settings → Secrets and
   variables → Actions → New repository secret. Name `VSCE_PAT`,
   value is the token. Once stored, GitHub redacts it from all logs.

### Per-release flow

The workflow runs automatically on the tag push from step 1 — no
extra action needed for normal releases. Watch progress at
`https://github.com/helly25/coderef/actions/workflows/vscode_marketplace.yml`.

For ad-hoc / retro-publish (e.g. a tag pushed before this workflow
existed, or a re-publish after a marketplace-side issue), trigger
via `workflow_dispatch`:

```bash
gh workflow run vscode_marketplace.yml -f tag=v<X.Y.Z>
gh run watch                                     # follow the run
```

Verify on the marketplace:

```bash
npx vsce show helly25.coderef --json | jq '.versions[0]'
```

### Manual fallback

If the workflow is broken and you need to publish from a local
machine:

```bash
export VSCE_PAT=$(security find-generic-password -a "$USER" -s VSCE_PAT -w)
cd "$REPO_ROOT/extension"
npm install --no-audit --no-fund
npm run package                                  # produces coderef-<ver>.vsix
ls -la coderef-*.vsix
npx vsce publish --packagePath coderef-<ver>.vsix
```

Note: `npm run package` calls `scripts/bundle-wasm.cjs`, which
requires `rustup`-managed `rustc` with `wasm32-unknown-unknown` on
`$PATH`. If `which rustc` resolves to a brew-managed binary, the
build will fail; either `brew unlink rust` or prepend the rustup
toolchain bin:

```bash
export PATH="$HOME/.rustup/toolchains/$(rustup show active-toolchain | awk '{print $1}')/bin:$PATH"
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
