#!/usr/bin/env node
// Build the coderef-core-wasm package and copy its output into the
// extension's out/wasm/ so `vsce package` can pick it up as a static
// asset shipped inside the VSIX.
//
// Algorithm:
//   1. Find the repo root (two levels up from this script).
//   2. Run `wasm-pack build --target nodejs --out-dir pkg
//      crates/coderef-core-wasm`. Skipped if CODEREF_SKIP_WASM_BUILD=1.
//   3. Copy the generated `pkg/*` into `<extension>/out/wasm/`.
//
// Both steps are idempotent. Re-running on a clean tree is a no-op
// after the first successful run (wasm-pack's incremental build kicks
// in; the copy overwrites existing files in-place).

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const HERE = __dirname; // extension/scripts
const EXTENSION_DIR = path.resolve(HERE, "..");
const REPO_ROOT = path.resolve(HERE, "..", "..");
const WASM_CRATE_DIR = path.join(REPO_ROOT, "crates", "coderef-core-wasm");
const PKG_SRC = path.join(WASM_CRATE_DIR, "pkg");
const PKG_DEST = path.join(EXTENSION_DIR, "out", "wasm");

function main() {
  if (!process.env.CODEREF_SKIP_WASM_BUILD) {
    checkToolchainCompat();
    runWasmPack();
  } else {
    log("CODEREF_SKIP_WASM_BUILD=1 — skipping wasm-pack invocation");
  }
  if (!fs.existsSync(PKG_SRC)) {
    fail(
      `coderef-core-wasm/pkg/ doesn't exist at ${PKG_SRC}. ` +
        "Run `wasm-pack build --target nodejs --out-dir pkg crates/coderef-core-wasm` " +
        "from the repo root, or unset CODEREF_SKIP_WASM_BUILD.",
    );
  }
  copyPkg();
  log(`bundled coderef-core-wasm → ${PKG_DEST}`);
}

/**
 * Declarative target-compat probe: verifies the active Rust toolchain
 * can actually build for `wasm32-unknown-unknown` before we invoke
 * wasm-pack and get a more opaque failure halfway through.
 *
 * Pattern borrowed from bazel-contrib/toolchains_llvm: declare what
 * targets are needed (in rust-toolchain.toml), check compatibility
 * at configure time, fail with an actionable message rather than
 * silently mutating the user's installation. See rust-toolchain.toml
 * for the declaration.
 */
function checkToolchainCompat() {
  const rustc = which("rustc");
  if (!rustc) {
    fail(
      "`rustc` not found on PATH. Install Rust via rustup " +
        "(https://rustup.rs) so the wasm32 target declared in " +
        "rust-toolchain.toml gets picked up automatically.",
    );
  }
  const r = spawnSync(rustc, ["--print", "sysroot"], { encoding: "utf8" });
  if (r.status !== 0) {
    fail(`\`rustc --print sysroot\` failed: ${r.stderr || r.stdout}`);
  }
  const sysroot = r.stdout.trim();
  const wasmTargetDir = path.join(sysroot, "lib", "rustlib", "wasm32-unknown-unknown");
  if (!fs.existsSync(wasmTargetDir)) {
    fail(
      "rust-toolchain.toml declares wasm32-unknown-unknown as a needed\n" +
        "target, but the active rustc sysroot has no matching stdlib at:\n" +
        `  ${wasmTargetDir}\n\n` +
        "Two ways to fix:\n" +
        "  - Recommended (canonical): install rustup so the toolchain\n" +
        "    manager reads rust-toolchain.toml and auto-installs the\n" +
        "    declared targets. `brew install rustup-init && rustup-init -y`\n" +
        "    then re-run this script.\n" +
        "  - Manual: download rust-std-<version>-wasm32-unknown-unknown.tar.gz\n" +
        "    matching `rustc --version`, extract its rustlib/wasm32-\n" +
        "    unknown-unknown/ into the sysroot above. Fragile (breaks on\n" +
        "    brew upgrade); use only as a stopgap.",
    );
  }
  log(`rust toolchain check: wasm32-unknown-unknown stdlib found at ${wasmTargetDir}`);
}

function runWasmPack() {
  const wasmPack = which("wasm-pack");
  if (!wasmPack) {
    fail(
      "`wasm-pack` not found on PATH. " +
        "Install via `brew install wasm-pack` or `cargo install wasm-pack`. " +
        "Set CODEREF_SKIP_WASM_BUILD=1 if you already have crates/coderef-core-wasm/pkg/ from a previous build.",
    );
  }
  log("running wasm-pack build --target nodejs --release crates/coderef-core-wasm");
  const r = spawnSync(
    wasmPack,
    ["build", "--release", "--target", "nodejs", "--out-dir", "pkg", WASM_CRATE_DIR],
    { cwd: REPO_ROOT, stdio: "inherit" },
  );
  if (r.status !== 0) fail(`wasm-pack exited with status ${r.status}`);
}

function copyPkg() {
  fs.mkdirSync(PKG_DEST, { recursive: true });
  const entries = fs.readdirSync(PKG_SRC, { withFileTypes: true });
  for (const entry of entries) {
    const src = path.join(PKG_SRC, entry.name);
    const dest = path.join(PKG_DEST, entry.name);
    if (entry.isFile()) {
      fs.copyFileSync(src, dest);
    }
    // Skip subdirectories — wasm-pack doesn't produce any at this level,
    // and our consumer only cares about the top-level .js + .wasm + .d.ts.
  }
}

function which(cmd) {
  const exts = process.platform === "win32"
    ? (process.env.PATHEXT || ".EXE;.CMD;.BAT").split(";")
    : [""];
  const dirs = (process.env.PATH || "").split(path.delimiter);
  for (const d of dirs) {
    for (const ext of exts) {
      const candidate = path.join(d, cmd + ext);
      if (fs.existsSync(candidate)) return candidate;
    }
  }
  return undefined;
}

function log(msg) {
  console.log(`bundle-wasm: ${msg}`);
}

function fail(msg) {
  console.error(`bundle-wasm: ${msg}`);
  process.exit(1);
}

main();
