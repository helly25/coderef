// Locates and loads the workspace's `.coderef.jsonc`.
//
// Search order matches DESIGN.md §7.2 and the v0.0.0 activation
// patterns in package.json:
//
//   <workspace>/.coderef.jsonc
//   <workspace>/.coderef.json
//   <workspace>/.config/coderef.jsonc
//   <workspace>/.config/coderef.json
//
// `coderef.configPath` setting overrides the search if non-empty.

import * as fs from "node:fs";
import * as path from "node:path";

import * as vscode from "vscode";

import { type EngineConfig, parseConfig } from "./wasmEngine";

export interface LoadedConfig {
  /** Absolute path of the config file on disk. */
  path: string;
  /** Parsed `Config` from the engine. */
  config: EngineConfig;
}

const DEFAULT_CANDIDATES: readonly string[] = [
  ".coderef.jsonc",
  ".coderef.json",
  ".config/coderef.jsonc",
  ".config/coderef.json",
];

/** Resolve the absolute path of the config, or undefined if not found. */
export function findConfigPath(workspaceRoot: string): string | undefined {
  const settings = vscode.workspace.getConfiguration("coderef");
  const override = settings.get<string>("configPath", "");
  if (override) {
    const abs = path.isAbsolute(override) ? override : path.join(workspaceRoot, override);
    return fs.existsSync(abs) ? abs : undefined;
  }
  for (const candidate of DEFAULT_CANDIDATES) {
    const abs = path.join(workspaceRoot, candidate);
    if (fs.existsSync(abs)) {
      return abs;
    }
  }
  return undefined;
}

/** Load + parse the config. Throws on read or parse failure (callers
 *  surface a friendly message via vscode.window.showErrorMessage). */
export function loadConfigFrom(absPath: string): LoadedConfig {
  const text = fs.readFileSync(absPath, "utf8");
  const config = parseConfig(text);
  return { path: absPath, config };
}
