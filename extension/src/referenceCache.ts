// Per-document cache of scanned references.
//
// Rescans happen on document open and on text-change with a small
// debounce. The cache is keyed by `Uri.toString()` so different scheme
// instances (file:/// vs untitled:) don't collide.

import * as vscode from "vscode";

import { type LoadedConfig } from "./configLoader";
import { type EngineReference, scanBuffer } from "./wasmEngine";

const DEBOUNCE_MS = 150;

export class ReferenceCache {
  private readonly cache = new Map<string, EngineReference[]>();
  private readonly debounces = new Map<string, NodeJS.Timeout>();

  constructor(private readonly getConfig: () => LoadedConfig | undefined) {}

  /** Synchronously rescan a document and update the cache. */
  scanNow(doc: vscode.TextDocument): EngineReference[] {
    const loaded = this.getConfig();
    if (!loaded) {
      this.cache.delete(doc.uri.toString());
      return [];
    }
    const ext = extOf(doc);
    const file = relativeOrAbsolutePath(doc.uri);
    try {
      const refs = scanBuffer(doc.getText(), loaded.config, ext, file);
      this.cache.set(doc.uri.toString(), refs);
      return refs;
    } catch (err) {
      // A scan failure for one document shouldn't break the editor;
      // log and return empty.
      console.error("coderef: scan failed for", file, err);
      this.cache.set(doc.uri.toString(), []);
      return [];
    }
  }

  /** Schedule a debounced rescan. */
  scanDebounced(doc: vscode.TextDocument): void {
    const key = doc.uri.toString();
    const prev = this.debounces.get(key);
    if (prev) {
      clearTimeout(prev);
    }
    this.debounces.set(
      key,
      setTimeout(() => {
        this.debounces.delete(key);
        this.scanNow(doc);
      }, DEBOUNCE_MS),
    );
  }

  /** Read cached references for a document; rescan synchronously if
   *  the cache is cold (first call after open or after configuration
   *  change). */
  get(doc: vscode.TextDocument): EngineReference[] {
    const key = doc.uri.toString();
    const cached = this.cache.get(key);
    if (cached !== undefined) {
      return cached;
    }
    return this.scanNow(doc);
  }

  /** Invalidate every cached document; next `get` rescans. */
  invalidateAll(): void {
    this.cache.clear();
  }

  dispose(): void {
    for (const t of this.debounces.values()) {
      clearTimeout(t);
    }
    this.debounces.clear();
    this.cache.clear();
  }
}

function extOf(doc: vscode.TextDocument): string | undefined {
  const fsPath = doc.uri.fsPath;
  const dot = fsPath.lastIndexOf(".");
  if (dot === -1 || dot === fsPath.length - 1) {
    return undefined;
  }
  return fsPath.slice(dot + 1);
}

function relativeOrAbsolutePath(uri: vscode.Uri): string {
  const folder = vscode.workspace.getWorkspaceFolder(uri);
  if (folder) {
    const rel = vscode.workspace.asRelativePath(uri, false);
    return rel;
  }
  return uri.fsPath;
}
