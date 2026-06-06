#!/usr/bin/env node
// Placeholder bin shim. Will exec the platform-native `coderef` binary
// downloaded by ./install.js when the workspace ships its first feature.
// At v0.0.0 there is no real binary yet; we just report status.

console.error(
  "coderef: no native binary installed yet (v0.0.0 scaffold). " +
    "Track v0.1 progress at https://github.com/helly25/coderef.",
);
process.exit(127);
