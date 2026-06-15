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
import {
  type EngineReference,
  explainText,
  type ExplainMatch,
  patternFor,
} from "./wasmEngine";

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
    const primaryLink = linkTargetFor(document, r);
    // Alternates: any *other* pattern that would also match this
    // matched_text. Useful when a token is interpretable several ways
    // (e.g. `JIRA(PROJ-1)` matched by both a `jira` and a generic
    // ticket pattern). The cache only carries the highest-priority
    // resolution; explainText surfaces the full set.
    const alternates: HoverAlternate[] = [];
    if (cfg) {
      try {
        const report = explainText(cfg.config, r.matched_text);
        for (const m of report.matches) {
          if (m.pattern_id === r.pattern_id) continue;
          alternates.push({
            pattern_id: m.pattern_id,
            pattern_kind: m.pattern_kind,
            target: m.target,
            uri: resolveTargetUri(document, m.pattern_kind, m.target),
            title: m.title,
          });
        }
      } catch {
        // Best-effort enrichment; a failed explain mustn't break the hover.
      }
    }
    const md = buildHoverMarkdown(r, pattern?.description, primaryLink, alternates);
    return new vscode.Hover(md, byteRangeToRange(document, r.byte_start, r.byte_end));
  }
}

/** Renderable alternate-target entry for the hover popover. Slimmed
 *  shape of `ExplainMatch` so callers can build alternates from any
 *  source (currently `explainText`, future scan-side multi-target).
 */
export interface HoverAlternate {
  pattern_id: string;
  pattern_kind: ExplainMatch["pattern_kind"];
  target: string;
  uri: vscode.Uri;
  title: string | null;
}

/** Build the markdown content for a hover popover. Pure function so
 *  it's testable without a real VSCode runtime. When `alternates` is
 *  non-empty, an "Alternative targets" list is appended below the
 *  primary target — clickable links to every other pattern that
 *  matches the same `matched_text`. */
export function buildHoverMarkdown(
  r: EngineReference,
  description: string | undefined,
  linkTarget: vscode.Uri,
  alternates: readonly HoverAlternate[] = [],
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
  if (alternates.length > 0) {
    const label =
      alternates.length === 1 ? "Alternative target" : `Alternative targets (${alternates.length})`;
    md.appendMarkdown(`\n\n**${label}:**\n\n`);
    for (const alt of alternates) {
      md.appendMarkdown(
        `- \`${alt.pattern_id}\` (${alt.pattern_kind}) → [${escapeMarkdown(alt.target)}](${alt.uri})`,
      );
      if (alt.title) {
        md.appendMarkdown(` — ${escapeMarkdown(alt.title)}`);
      }
      md.appendMarkdown("\n");
    }
  }
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
  return resolveTargetUri(document, r.pattern_kind, r.target);
}

/** Resolve a target string to a `vscode.Uri`. Factored out so
 *  alternate-target alternates (from `explainText`) and the primary
 *  reference go through the same path. */
export function resolveTargetUri(
  document: vscode.TextDocument,
  kind: ExplainMatch["pattern_kind"],
  target: string,
): vscode.Uri {
  if (kind === "url") {
    return vscode.Uri.parse(target);
  }
  // Local / ifchange / command kinds: resolve as workspace-rooted.
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  const base = folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath);
  const trimmed = target.startsWith("/") ? target.slice(1) : target;
  return vscode.Uri.file(path.join(base, trimmed));
}

function escapeMarkdown(s: string): string {
  return s.replace(/([\\`*_{}[\]()#+\-.!|])/g, "\\$1");
}
