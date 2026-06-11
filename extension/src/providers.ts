// VSCode language providers backed by the engine's scan results.
//
//   DocumentLinkProvider — turns each reference into a clickable link.
//                          URL kind → opens the resolved target.
//                          Local kind → opens the resolved path under
//                                       the workspace root.
//   HoverProvider        — shows the pattern id, description from the
//                          config (if any), resolved title (if any),
//                          and target on hover. The description lookup
//                          gives the hover its "what is this pattern
//                          for" answer without the reader having to
//                          open .coderef.jsonc.

import * as path from "node:path";

import * as vscode from "vscode";

import { type LoadedConfig } from "./configLoader";
import { type ReferenceCache } from "./referenceCache";
import { byteRangeToRange, positionToByteOffset } from "./textOffset";
import { type EngineReference, patternFor } from "./wasmEngine";

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
  constructor(
    private readonly cache: ReferenceCache,
    /** Resolves to the currently-loaded config, so the hover can look
     *  up the pattern description by id without re-parsing. */
    private readonly getConfig: () => LoadedConfig | undefined,
  ) {}

  provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken,
  ): vscode.ProviderResult<vscode.Hover> {
    const refs = this.cache.get(document);
    const byteOffset = positionToByteOffset(document, position);
    const r = refs.find(
      (ref) => byteOffset >= ref.byte_start && byteOffset < ref.byte_end,
    );
    if (!r) {
      return undefined;
    }
    const cfg = this.getConfig();
    const pattern = patternFor(cfg?.config, r.pattern_id);
    const md = buildHoverMarkdown(r, pattern?.description, linkTargetFor(document, r));
    return new vscode.Hover(md, byteRangeToRange(document, r.byte_start, r.byte_end));
  }
}

/** Build the markdown content for a hover popover. Pure function so
 *  it's testable without a real VSCode runtime. */
export function buildHoverMarkdown(
  r: EngineReference,
  description: string | undefined,
  linkTarget: vscode.Uri,
): vscode.MarkdownString {
  const md = new vscode.MarkdownString();
  md.appendMarkdown(`**coderef** &nbsp; \`${r.pattern_id}\` (${r.pattern_kind})\n\n`);
  if (description) {
    md.appendMarkdown(`${escapeMarkdown(description)}\n\n`);
  }
  if (r.title) {
    md.appendMarkdown(`${escapeMarkdown(r.title)}\n\n`);
  }
  md.appendMarkdown(`→ [${escapeMarkdown(r.target)}](${linkTarget})`);
  return md;
}

/** Build a VSCode `DocumentLink` for the given engine reference. */
export function toLink(
  document: vscode.TextDocument,
  r: EngineReference,
): vscode.DocumentLink {
  return new vscode.DocumentLink(
    byteRangeToRange(document, r.byte_start, r.byte_end),
    linkTargetFor(document, r),
  );
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
