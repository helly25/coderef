// postinstall: download the right prebuilt `coderef` binary for the host
// platform/arch from the matching GitHub Release, verify checksum, install
// next to bin/coderef.js. At v0.0.0 this is a no-op; the binary doesn't
// exist yet. See DESIGN.md §4.2 and §18 for the planned distribution shape.

"use strict";

console.log(
  "coderef-npm: v0.0.0 scaffold — no native binary to download yet. " +
    "Real install logic lands with the first cross-compiled release.",
);
process.exit(0);
