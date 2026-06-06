# @helly25/coderef

npm wrapper for the [`coderef`](https://github.com/helly25/coderef) CLI.

On install, the wrapper downloads the right native `coderef` binary
for your platform/architecture from the matching GitHub Release and
exposes it as `coderef` in `node_modules/.bin/`. Lets `pre-commit`
hooks with `language: node` and `npx -y @helly25/coderef …` Just
Work without the consumer needing Rust toolchain installed.

**v0.0.0 status**: scaffold only. No native binary is built yet; the
postinstall is a no-op. Track v0.1 progress in the parent repo.

## License

Apache License 2.0. See `../../LICENSE` in the source repository.
