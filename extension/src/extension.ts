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

import { explainReferenceCommand } from "./commands";
import { findConfigPath, loadConfigFrom, type LoadedConfig } from "./configLoader";
import {
  CoderefDocumentLinkProvider,
  CoderefHoverProvider,
} from "./providers";
import { ReferenceCache } from "./referenceCache";
import {
  ReferencesTreeProvider,
  copyReferencesAsMarkdownCommand,
  exportReferencesAsJsonCommand,
  rescanWorkspace,
  setScanModeCommand,
} from "./referencesView";
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
      new CoderefHoverProvider(cache, () => currentConfig),
    ),
  );

  // Commands.
  context.subscriptions.push(
    vscode.commands.registerCommand("coderef.explainReference", () =>
      explainReferenceCommand(cache, () => currentConfig),
    ),
  );

  // References browser (DESIGN §14.7). Tree view in the activity-bar
  // container declared in package.json. Refresh on demand or on
  // file-system events (debounced).
  const refsProvider = new ReferencesTreeProvider(() => currentConfig);
  context.subscriptions.push(
    vscode.window.registerTreeDataProvider("coderef.references", refsProvider),
  );
  const triggerRescan = debounce(() => {
    void rescanWorkspace(refsProvider, () => currentConfig);
  }, 300);
  context.subscriptions.push(
    vscode.commands.registerCommand("coderef.references.refresh", () => {
      void rescanWorkspace(refsProvider, () => currentConfig);
    }),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("coderef.references.copyAsMarkdown", () => {
      void copyReferencesAsMarkdownCommand(refsProvider, () => currentConfig);
    }),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("coderef.references.exportJson", () => {
      void exportReferencesAsJsonCommand(
        refsProvider,
        () => currentConfig,
        engineVersion(),
      );
    }),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("coderef.references.setScanMode", () => {
      void setScanModeCommand(() => triggerRescan());
    }),
  );
  // Initial population.
  triggerRescan();
  // Refresh on file change/save.
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument(() => triggerRescan()),
  );
  const refsWatcher = vscode.workspace.createFileSystemWatcher("**/*");
  context.subscriptions.push(refsWatcher);
  context.subscriptions.push(refsWatcher.onDidCreate(() => triggerRescan()));
  context.subscriptions.push(refsWatcher.onDidDelete(() => triggerRescan()));

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
        triggerRescan();
      }
    }),
  );
}

/** Small debounce so a flurry of file events collapses into one rescan. */
function debounce<F extends (...args: never[]) => void>(fn: F, ms: number): F {
  let t: NodeJS.Timeout | undefined;
  return ((...args: never[]) => {
    if (t) clearTimeout(t);
    t = setTimeout(() => {
      t = undefined;
      fn(...args);
    }, ms);
  }) as F;
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
