# helly25.coderef — VSCode extension

VSCode integration for [`coderef`](https://github.com/helly25/coderef).
Imports the `@helly25/coderef-core-wasm` module for hot-path scanning
(DocumentLink, Hover) and shells out to the native `coderef` binary
for HTTP verification and write-mode subcommands.

**v0.0.0 status**: scaffold only. `activate()` / `deactivate()` are
no-ops; providers and config UI land per the v0.1 roadmap in the
parent repo's `DESIGN.md` §14.

## Build

```sh
npm install
npm run compile
```

## License

Apache License 2.0. See `../LICENSE`.
