// Thin wrapper around the wasm-pack-generated `coderef-core-wasm`
// bindings.
//
// The WASM module is loaded by `require()` at runtime rather than via
// an npm dependency. Reason: `wasm-pack build` is part of the engine's
// build, not of the TS compile — declaring it as an npm dep would
// force everyone (and CI's TS-only job) to install Rust and wasm-pack
// before `tsc --noEmit` could run. The runtime path is
// `<repo-root>/crates/coderef-core-wasm/pkg/coderef_core_wasm` which
// matches what `wasm-pack --target nodejs --out-dir pkg` produces.
//
// Types are spelled out here rather than imported from the wasm-pack
// `.d.ts`; that keeps `tsc` self-contained and avoids coupling the TS
// build to a generated artifact.

import * as path from "node:path";

/* eslint-disable @typescript-eslint/no-explicit-any */

/** Engine-side `Config` returned by `parse_config`. */
export interface EngineConfig {
  patterns: Record<string, unknown>;
  variables?: Record<string, unknown>;
  ignore?: string[];
}

/** Engine-side `Reference` returned by `scan_buffer`. */
export interface EngineReference {
  pattern_id: string;
  pattern_kind: "url" | "local" | "ifchange" | "command";
  file: string;
  line: number;
  column: number;
  byte_start: number;
  byte_end: number;
  matched_text: string;
  captures: Record<string, string>;
  target: string;
  title: string | null;
  in_comment: boolean;
}

interface WasmModule {
  version(): string;
  banner(): string;
  parse_config(jsonc: string): EngineConfig;
  scan_buffer(
    content: string,
    config_json: string,
    language_ext: string | undefined,
    file: string,
  ): EngineReference[];
  doctor_static(config_json: string): unknown;
}

let cached: WasmModule | null = null;
let loadError: Error | null = null;

/** Resolve and require the wasm-pack output. Cached after first call. */
function loadModule(): WasmModule {
  if (cached) return cached;
  if (loadError) throw loadError;
  // Compiled location is extension/out/wasmEngine.js, so:
  //   __dirname = .../extension/out
  //   pkg       = ../../crates/coderef-core-wasm/pkg/coderef_core_wasm
  const pkgPath = path.resolve(
    __dirname,
    "..",
    "..",
    "crates",
    "coderef-core-wasm",
    "pkg",
    "coderef_core_wasm",
  );
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    cached = require(pkgPath) as WasmModule;
    return cached;
  } catch (err) {
    loadError = err as Error;
    throw new Error(
      `coderef: failed to load WASM engine from ${pkgPath} (${loadError.message}). ` +
        `Run \`wasm-pack build --target nodejs --out-dir pkg crates/coderef-core-wasm\` ` +
        `from the repo root.`,
    );
  }
}

export function engineVersion(): string {
  return loadModule().version();
}

export function parseConfig(jsonc: string): EngineConfig {
  return loadModule().parse_config(jsonc);
}

export function scanBuffer(
  content: string,
  config: EngineConfig,
  languageExt: string | undefined,
  file: string,
): EngineReference[] {
  const configJson = JSON.stringify(config);
  return loadModule().scan_buffer(content, configJson, languageExt, file);
}
