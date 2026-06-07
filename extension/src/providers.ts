// VSCode language providers backed by the engine's scan results.
//
//   DocumentLinkProvider — turns each reference into a clickable link.
//                          URL kind → opens the resolved target.
//                          Local kind → opens the resolved path under
//                                       the workspace root.
//   HoverProvider        — shows the pattern id, resolved title (if any),
//                          and target on hover.

import * as path from "node:path";

import * as vscode from "vscode";

import { type ReferenceCache } from "./referenceCache";
import { type EngineReference } from "./wasmEngine";

export class CoderefDocumentLinkProvider implements vscode.DocumentLinkProvider {
  constructor(private readonly cache: ReferenceCache) {}

  provideDocumentLinks(
    document: vscode.TextDocument,
    _token: vscode.CancellationToken,
  ): vscode.ProviderResult<vscode.DocumentLink[]> {
    const refs = this.cache.get(document);
    return refs.map((r) => toLink(document, r));
  }
}

export class CoderefHoverProvider implements vscode.HoverProvider {
  constructor(private readonly cache: ReferenceCache) {}

  provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken,
  ): vscode.ProviderResult<vscode.Hover> {
    const refs = this.cache.get(document);
    const offset = document.offsetAt(position);
    const r = refs.find(
      (ref) => offset >= ref.byte_start && offset < ref.byte_end,
    );
    if (!r) {
      return undefined;
    }
    const md = new vscode.MarkdownString();
    md.appendMarkdown(`**coderef** &nbsp; \`${r.pattern_id}\` (${r.pattern_kind})\n\n`);
    if (r.title) {
      md.appendMarkdown(`${escapeMarkdown(r.title)}\n\n`);
    }
    md.appendMarkdown(`→ [${escapeMarkdown(r.target)}](${linkTargetFor(document, r)})`);
    return new vscode.Hover(
      md,
      new vscode.Range(
        document.positionAt(r.byte_start),
        document.positionAt(r.byte_end),
      ),
    );
  }
}

/** Build a VSCode `DocumentLink` for the given engine reference. */
export function toLink(
  document: vscode.TextDocument,
  r: EngineReference,
): vscode.DocumentLink {
  const range = new vscode.Range(
    document.positionAt(r.byte_start),
    document.positionAt(r.byte_end),
  );
  return new vscode.DocumentLink(range, linkTargetFor(document, r));
}

/** Resolve a `Reference`'s `target` field to a `vscode.Uri` suitable
 *  for `DocumentLink.target`. URL kinds parse as-is; Local kinds
 *  resolve under the workspace root (DESIGN.md §6.1 default
 *  `workspace` anchor mode — leading slash means workspace-rooted). */
export function linkTargetFor(
  document: vscode.TextDocument,
  r: EngineReference,
): vscode.Uri {
  if (r.pattern_kind === "url") {
    return vscode.Uri.parse(r.target);
  }
  // Local. Resolve under workspace folder of this document.
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  const base = folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath);
  const trimmed = r.target.startsWith("/") ? r.target.slice(1) : r.target;
  return vscode.Uri.file(path.join(base, trimmed));
}

function escapeMarkdown(s: string): string {
  return s.replace(/([\\`*_{}[\]()#+\-.!|])/g, "\\$1");
}
