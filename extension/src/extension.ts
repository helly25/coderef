// VSCode extension entry point.
//
// Activation:
//   1. Locate the workspace's .coderef.jsonc.
//   2. Parse it via the WASM engine.
//   3. Register DocumentLinkProvider + HoverProvider.
//   4. Watch the config file for changes; reload + invalidate the
//      per-document cache on change.
//
// Per DESIGN.md §14.5.1 the engine itself runs in-process via WASM.
// Workspace-wide verification (`coderef check`) is delegated to the
// CLI binary in a later PR; the in-process surface is hot-path only.

import * as vscode from "vscode";

import { findConfigPath, loadConfigFrom, type LoadedConfig } from "./configLoader";
import {
  CoderefDocumentLinkProvider,
  CoderefHoverProvider,
} from "./providers";
import { ReferenceCache } from "./referenceCache";
import { engineVersion } from "./wasmEngine";

let currentConfig: LoadedConfig | undefined;

export function activate(context: vscode.ExtensionContext): void {
  if (!vscode.workspace.getConfiguration("coderef").get<boolean>("enabled", true)) {
    return;
  }

  try {
    console.log(`coderef: engine ${engineVersion()}`);
  } catch (err) {
    void vscode.window.showErrorMessage(
      `coderef: WASM engine failed to load (${err instanceof Error ? err.message : String(err)}). ` +
        `Run \`wasm-pack build --target nodejs --out-dir pkg crates/coderef-core-wasm\` and reload.`,
    );
    return;
  }

  reloadConfig();

  const cache = new ReferenceCache(() => currentConfig);
  context.subscriptions.push(cache);

  // Providers — register once; the cache handles freshness.
  context.subscriptions.push(
    vscode.languages.registerDocumentLinkProvider(
      { scheme: "file" },
      new CoderefDocumentLinkProvider(cache),
    ),
  );
  context.subscriptions.push(
    vscode.languages.registerHoverProvider(
      { scheme: "file" },
      new CoderefHoverProvider(cache),
    ),
  );

  // Document lifecycle — keep the cache warm.
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (doc.uri.scheme === "file") {
        cache.scanNow(doc);
      }
    }),
  );
  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((e) => {
      cache.scanDebounced(e.document);
    }),
  );

  // Config file lifecycle.
  const watcher = vscode.workspace.createFileSystemWatcher(
    "**/{.coderef.jsonc,.coderef.json,.config/coderef.jsonc,.config/coderef.json}",
  );
  const onConfigChanged = (): void => {
    reloadConfig();
    cache.invalidateAll();
  };
  context.subscriptions.push(watcher);
  context.subscriptions.push(watcher.onDidCreate(onConfigChanged));
  context.subscriptions.push(watcher.onDidChange(onConfigChanged));
  context.subscriptions.push(watcher.onDidDelete(onConfigChanged));

  // Settings: react to coderef.configPath / coderef.enabled changes.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("coderef")) {
        reloadConfig();
        cache.invalidateAll();
      }
    }),
  );
}

export function deactivate(): void {
  currentConfig = undefined;
}

/** Locate + parse the config for the first workspace folder. v0.1
 *  supports single-root workspaces; multi-root (with per-folder
 *  configs) lands in v0.2 per DESIGN.md §7.6. */
function reloadConfig(): void {
  currentConfig = undefined;
  const folders = vscode.workspace.workspaceFolders ?? [];
  if (folders.length === 0) {
    return;
  }
  const folder = folders[0];
  if (!folder) {
    return;
  }
  const cfgPath = findConfigPath(folder.uri.fsPath);
  if (!cfgPath) {
    return;
  }
  try {
    currentConfig = loadConfigFrom(cfgPath);
  } catch (err) {
    void vscode.window.showErrorMessage(
      `coderef: failed to load ${cfgPath}: ${err instanceof Error ? err.message : String(err)}`,
    );
  }
}
