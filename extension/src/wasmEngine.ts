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

/** Engine-side `Pattern`. The extension reads only the fields surfaced
 *  in hover / link UI; everything else passes through opaquely on the
 *  way back to scan_buffer. */
export interface EnginePattern {
  description?: string;
  kind?: "url" | "local" | "ifchange" | "command";
  regex: string;
  target?: string;
  title?: string;
  // Other fields (scope, severity, resolve, …) are accepted by the
  // engine but not read here. Allow them via index signature so the
  // round-trip JSON.stringify preserves them.
  [key: string]: unknown;
}

/** Engine-side `Config` returned by `parse_config`. */
export interface EngineConfig {
  patterns: Record<string, EnginePattern>;
  variables?: Record<string, unknown>;
  ignore?: string[];
  [key: string]: unknown;
}

/** Look up a pattern in a config by id, or undefined if absent. */
export function patternFor(
  config: EngineConfig | undefined,
  patternId: string,
): EnginePattern | undefined {
  return config?.patterns?.[patternId];
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

/** One match returned by `explain_text`. */
export interface ExplainMatch {
  pattern_id: string;
  pattern_kind: "url" | "local" | "ifchange" | "command";
  description: string | null;
  matched_text: string;
  captures: Record<string, string>;
  target: string;
  title: string | null;
  priority: number;
  scope_notes: string[];
  resolution_warnings: string[];
}

/** Full `explain_text` result. */
export interface ExplainReport {
  input: string;
  matches: ExplainMatch[];
  non_matching_pattern_ids: string[];
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
  explain_text(config_json: string, input: string): ExplainReport;
}

let cached: WasmModule | null = null;
let loadError: Error | null = null;

/** Resolve and require the wasm-pack output. Cached after first call.
 *
 *  Tried paths, in order:
 *  1. `extension/out/wasm/coderef_core_wasm` — the bundled location
 *     when the extension was installed from a VSIX. `scripts/
 *     bundle-wasm.cjs` puts the wasm-pack output here as part of
 *     `npm run build` / `vscode:prepublish`.
 *  2. `<repo>/crates/coderef-core-wasm/pkg/coderef_core_wasm` —
 *     the dev location when the extension is being run from a repo
 *     checkout (e.g. via F5 in the Extension Host).
 */
function loadModule(): WasmModule {
  if (cached) return cached;
  if (loadError) throw loadError;
  // __dirname = .../extension/out (after compile)
  const bundled = path.join(__dirname, "wasm", "coderef_core_wasm");
  const dev = path.resolve(
    __dirname,
    "..",
    "..",
    "crates",
    "coderef-core-wasm",
    "pkg",
    "coderef_core_wasm",
  );
  const tries: string[] = [bundled, dev];
  const errors: string[] = [];
  for (const candidate of tries) {
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      cached = require(candidate) as WasmModule;
      return cached;
    } catch (err) {
      errors.push(`${candidate}: ${(err as Error).message}`);
    }
  }
  loadError = new Error(
    "coderef: failed to load WASM engine from any of:\n  " +
      errors.join("\n  ") +
      "\nRun `npm run build` from the extension directory (or " +
      "`wasm-pack build --target nodejs --out-dir pkg crates/coderef-core-wasm` " +
      "from the repo root).",
  );
  throw loadError;
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

export function explainText(
  config: EngineConfig,
  input: string,
): ExplainReport {
  return loadModule().explain_text(JSON.stringify(config), input);
}
