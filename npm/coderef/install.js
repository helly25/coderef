#!/usr/bin/env node
// @ts-check
//
// postinstall: install a runnable `coderef` binary next to bin/coderef.js.
//
// Resolution order:
//   1. CODEREF_BINARY_PATH env var (test/CI escape hatch).
//   2. A `coderef` (or `coderef.exe`) on PATH (developer override).
//   3. Sibling `target/release/coderef` from a workspace checkout
//      (when this package is installed via `file:` from inside the
//      coderef repo).
//   4. Download the matching release tarball from
//      https://github.com/helly25/coderef/releases. Verify SHA-256.
//   5. Fall back to `cargo build --release` if cargo is on PATH.
//   6. Print a clear, actionable error and exit non-zero.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");

const PKG = require("./package.json");
const VERSION = PKG.version;

const HERE = __dirname;
const BIN_DEST_DIR = path.join(HERE, "bin");
const BIN_DEST = path.join(
  BIN_DEST_DIR,
  process.platform === "win32" ? "coderef.exe" : "coderef",
);

function main() {
  if (process.env.CODEREF_BINARY_PATH) {
    install(process.env.CODEREF_BINARY_PATH, "env CODEREF_BINARY_PATH");
    return;
  }

  const onPath = which(process.platform === "win32" ? "coderef.exe" : "coderef");
  if (onPath && !sameFile(onPath, BIN_DEST)) {
    install(onPath, "found on PATH");
    return;
  }

  const sibling = locateRepoSiblingBinary();
  if (sibling) {
    install(sibling, "sibling target/release/coderef");
    return;
  }

  if (tryDownload()) return;
  if (tryCargoBuild()) return;

  fail(
    "coderef-npm: no way to install a binary.\n" +
      "  - No CODEREF_BINARY_PATH env var.\n" +
      "  - No `coderef` on PATH.\n" +
      "  - No sibling `target/release/coderef`.\n" +
      "  - Release v" + VERSION + " download failed (release may not exist yet, or no internet).\n" +
      "  - No `cargo` on PATH for source build.\n\n" +
      "Fix one of:\n" +
      "  - Wait for a v" + VERSION + " GitHub Release; re-run `npm install`.\n" +
      "  - Install Rust (https://rustup.rs) and re-run `npm install`.\n" +
      "  - Set CODEREF_BINARY_PATH to a coderef binary you already have.",
  );
}

function install(src, reason) {
  if (!fs.existsSync(src)) {
    fail(`coderef-npm: ${reason}, but source doesn't exist: ${src}`);
  }
  fs.mkdirSync(BIN_DEST_DIR, { recursive: true });
  fs.copyFileSync(src, BIN_DEST);
  if (process.platform !== "win32") {
    fs.chmodSync(BIN_DEST, 0o755);
  }
  console.log(`coderef-npm: installed binary from ${reason} → ${BIN_DEST}`);
}

function locateRepoSiblingBinary() {
  // npm/coderef → walk up two levels to repo root, then target/release.
  const repoRoot = path.resolve(HERE, "..", "..");
  const candidate = path.join(
    repoRoot,
    "target",
    "release",
    process.platform === "win32" ? "coderef.exe" : "coderef",
  );
  return fs.existsSync(candidate) ? candidate : undefined;
}

function tryDownload() {
  const { platform, arch } = mapPlatform();
  if (!platform || !arch) {
    console.log(
      `coderef-npm: unsupported platform/arch (${process.platform}/${process.arch}); ` +
        "skipping download.",
    );
    return false;
  }
  const assetExt = platform === "windows" ? "zip" : "tar.gz";
  const assetName = `coderef-v${VERSION}-${platform}-${arch}.${assetExt}`;
  const url = `https://github.com/helly25/coderef/releases/download/v${VERSION}/${assetName}`;
  const sumUrl = `${url}.sha256`;

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "coderef-dl-"));
  const archivePath = path.join(tmp, assetName);
  const sumPath = `${archivePath}.sha256`;

  console.log(`coderef-npm: downloading ${url}`);
  if (!downloadSync(url, archivePath)) {
    console.log(
      `coderef-npm: download failed (release v${VERSION} may not exist yet).`,
    );
    return false;
  }
  if (!downloadSync(sumUrl, sumPath)) {
    console.log("coderef-npm: checksum download failed; refusing to install unverified binary.");
    return false;
  }
  const expected = fs.readFileSync(sumPath, "utf8").trim().split(/\s+/)[0];
  const actual = crypto
    .createHash("sha256")
    .update(fs.readFileSync(archivePath))
    .digest("hex");
  if (expected !== actual) {
    fail(
      `coderef-npm: checksum mismatch for ${assetName}.\n` +
        `  expected: ${expected}\n  actual:   ${actual}`,
    );
  }

  const extracted = extractArchive(archivePath, tmp);
  if (!extracted) {
    console.log(`coderef-npm: failed to extract ${assetName}.`);
    return false;
  }
  install(extracted, `downloaded release v${VERSION}`);
  return true;
}

function tryCargoBuild() {
  const cargo = which("cargo");
  if (!cargo) return false;

  // Only attempt if there's a Cargo workspace to build from — i.e. we
  // were installed via `file:` from inside the coderef repo.
  const repoRoot = path.resolve(HERE, "..", "..");
  if (!fs.existsSync(path.join(repoRoot, "Cargo.toml"))) return false;

  console.log("coderef-npm: building from source via `cargo build --release`...");
  const r = spawnSync(
    cargo,
    ["build", "--release", "--bin", "coderef", "--locked"],
    { cwd: repoRoot, stdio: "inherit" },
  );
  if (r.status !== 0) {
    console.log("coderef-npm: cargo build failed.");
    return false;
  }
  const out = locateRepoSiblingBinary();
  if (!out) {
    console.log("coderef-npm: cargo build succeeded but binary not found.");
    return false;
  }
  install(out, "cargo build --release");
  return true;
}

function mapPlatform() {
  let platform, arch;
  switch (process.platform) {
    case "linux":   platform = "linux"; break;
    case "darwin":  platform = "macos"; break;
    case "win32":   platform = "windows"; break;
    default:        platform = undefined;
  }
  switch (process.arch) {
    case "x64":    arch = "x64"; break;
    case "arm64":  arch = "arm64"; break;
    default:       arch = undefined;
  }
  // Intel Macs are unsupported as of v0.2.1: Apple stopped shipping
  // Intel hardware in 2023 and the macos-13 GitHub runner was the
  // slowest in our release matrix. Emit an explicit error rather than
  // letting the download fall over on a 404.
  if (platform === "macos" && arch === "x64") {
    arch = undefined;
  }
  return { platform, arch };
}

// Synchronous HTTPS download via spawnSync of a child Node interpreter.
// install.js is invoked by `npm install` and the caller expects it to
// finish before `npm install` returns; we want sync I/O semantics
// without pulling in a deasync-style native dep.
function downloadSync(url, dest) {
  const script =
    "const https=require('node:https'),fs=require('node:fs'),{URL}=require('node:url');" +
    "function go(u,n){if(n<0)return process.exit(1);https.get(u,r=>{" +
    "if([301,302,307,308].includes(r.statusCode)&&r.headers.location){return go(new URL(r.headers.location,u).toString(),n-1)}" +
    `if(r.statusCode!==200)return process.exit(1);const f=fs.createWriteStream(${JSON.stringify(dest)});r.pipe(f);` +
    "f.on('finish',()=>f.close(()=>process.exit(0)));f.on('error',()=>process.exit(1))}).on('error',()=>process.exit(1))}" +
    `go(${JSON.stringify(url)},5);`;
  const r = spawnSync(process.execPath, ["-e", script], { stdio: "inherit" });
  return r.status === 0;
}

function extractArchive(archive, intoDir) {
  const isZip = archive.toLowerCase().endsWith(".zip");
  if (isZip) {
    const r = spawnSync("powershell", [
      "-NoProfile",
      "-Command",
      `Expand-Archive -Path "${archive}" -DestinationPath "${intoDir}" -Force`,
    ], { stdio: "inherit" });
    if (r.status !== 0) return undefined;
  } else {
    const r = spawnSync("tar", ["-xzf", archive, "-C", intoDir], { stdio: "inherit" });
    if (r.status !== 0) return undefined;
  }
  const entries = fs.readdirSync(intoDir);
  for (const entry of entries) {
    const full = path.join(intoDir, entry);
    if (fs.statSync(full).isDirectory()) {
      const inner = path.join(
        full,
        process.platform === "win32" ? "coderef.exe" : "coderef",
      );
      if (fs.existsSync(inner)) return inner;
    }
  }
  return undefined;
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

function sameFile(a, b) {
  try {
    return fs.realpathSync(a) === fs.realpathSync(b);
  } catch {
    return false;
  }
}

function fail(msg) {
  console.error(msg);
  process.exit(1);
}

main();
