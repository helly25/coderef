#!/usr/bin/env node
// Wrapper around the platform-native `coderef` binary placed here by
// install.js. Forwards every argument verbatim and propagates the
// exit status so callers (pre-commit hooks, CI scripts, shells) see
// identical behaviour to invoking the binary directly.

"use strict";

const path = require("node:path");
const fs = require("node:fs");
const { spawnSync } = require("node:child_process");

const HERE = __dirname;
const BIN = path.join(
  HERE,
  process.platform === "win32" ? "coderef.exe" : "coderef",
);

if (!fs.existsSync(BIN)) {
  console.error(
    "coderef: binary not found at " + BIN + "\n" +
      "The npm postinstall (install.js) didn't manage to place one. " +
      "Re-run `npm install` for this package, or set CODEREF_BINARY_PATH " +
      "to an existing coderef binary.",
  );
  process.exit(127);
}

const r = spawnSync(BIN, process.argv.slice(2), { stdio: "inherit" });
if (r.error) {
  console.error("coderef: failed to spawn binary:", r.error.message);
  process.exit(127);
}
process.exit(r.status ?? 1);
