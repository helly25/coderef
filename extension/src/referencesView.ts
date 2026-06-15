// References browser — sidebar tree view that lists every reference
// in the workspace, grouped category-first per DESIGN.md §5.7.3.
//
// v0.2 slice (DESIGN.md §14.7):
//   - Activity-bar view container `coderef.references`
//   - Primary grouping: category (DESIGN §5.7.3 display order)
//   - Secondary grouping: file path
//   - Leaf click → jump to the reference site
//   - Live updates via fileSystemWatcher + onDidSaveTextDocument
//   - Filter chips: Verified / Broken (status placeholders for now —
//     verification status lives in `coderef check`, not the in-process
//     scan; until that's wired in, leaves are uncoloured)
//
// Deferred to v0.3 (DESIGN.md §14.7 longer tail):
//   - Scan modes (workspace / openFiles / currentFile selector)
//   - Mine / Unverified / Drifted filters
//   - Multi-target hover alternates, exportJson
//   - references.tooManyNodes / uncategorisedSpike doctor checks
//   - maxNodesPerLevel cap
//
// Shipped in v0.3:
//   - Copy-as-Markdown command (this file: getAllRefs +
//     renderReferencesAsMarkdown).

import * as path from "node:path";

import * as vscode from "vscode";

import { type LoadedConfig } from "./configLoader";
import { type EngineReference, scanBuffer } from "./wasmEngine";

/** Display-order index for a category. Mirrors `coderef_core::category::display_order`
 *  so the TS side doesn't need to round-trip through WASM for ordering. */
const DISPLAY_ORDER: Record<string, number> = {
  files: 0,
  people: 1,
  tickets: 2,
  standards: 3,
  urls: 4,
  "coupled-change": 5,
  // user-defined slot here between coupled-change (5) and other
  // (sentinel max); resolved at sort time.
  other: Number.MAX_SAFE_INTEGER,
};

const USER_DEFINED_ORDER = 100;

const CATEGORY_GLYPH: Record<string, string> = {
  files: "📁",
  people: "👤",
  tickets: "🎫",
  standards: "📜",
  urls: "🔗",
  "coupled-change": "🔄",
  other: "❓",
};

const USER_DEFINED_GLYPH = "🏷";

/** Default category for a pattern that doesn't declare one. Mirrors
 *  `coderef_core::category::infer_category`. */
function inferCategory(kind: string | undefined): string {
  switch (kind) {
    case "local":
      return "files";
    case "ifchange":
      return "coupled-change";
    default:
      return "other";
  }
}

/** Look up the resolved category for a reference. Reads the pattern's
 *  declared `category` (if any) via the loaded config, falling back to
 *  the kind-based inference. */
function categoryOf(
  ref: EngineReference,
  config: LoadedConfig | undefined,
): string {
  const pat = config?.config?.patterns?.[ref.pattern_id];
  const declared = typeof pat?.category === "string" ? pat.category : undefined;
  return declared ?? inferCategory(ref.pattern_kind);
}

function displayOrder(category: string): number {
  if (category in DISPLAY_ORDER) return DISPLAY_ORDER[category]!;
  return USER_DEFINED_ORDER;
}

function glyphOf(category: string): string {
  if (category in CATEGORY_GLYPH) return CATEGORY_GLYPH[category]!;
  return USER_DEFINED_GLYPH;
}

/** One leaf in the tree. */
class ReferenceLeaf extends vscode.TreeItem {
  constructor(
    public readonly ref: EngineReference,
    public readonly fileUri: vscode.Uri,
  ) {
    const label = `${ref.matched_text}  →  ${ref.target}`;
    super(label, vscode.TreeItemCollapsibleState.None);
    this.description = `${path.basename(ref.file)}:${ref.line}`;
    this.tooltip = `${ref.file}:${ref.line}:${ref.column}\n[${ref.pattern_id}]\n${ref.matched_text} → ${ref.target}`;
    this.iconPath = new vscode.ThemeIcon("link");
    this.contextValue = "coderef.references.leaf";
    // Clicking a leaf jumps to the ref site. Reveal at the start of
    // the match (matches the DocumentLink behaviour).
    this.command = {
      command: "vscode.open",
      title: "Open reference",
      arguments: [
        fileUri,
        <vscode.TextDocumentShowOptions>{
          selection: new vscode.Range(
            new vscode.Position(Math.max(0, ref.line - 1), Math.max(0, ref.column - 1)),
            new vscode.Position(Math.max(0, ref.line - 1), Math.max(0, ref.column - 1)),
          ),
          preserveFocus: false,
        },
      ],
    };
  }
}

/** One file under a category — its children are the leaves. */
class FileNode extends vscode.TreeItem {
  constructor(
    public readonly file: string,
    public readonly fileUri: vscode.Uri,
    public readonly refs: EngineReference[],
  ) {
    super(file, vscode.TreeItemCollapsibleState.Expanded);
    this.description = `${refs.length}`;
    this.iconPath = vscode.ThemeIcon.File;
    this.resourceUri = fileUri;
    this.contextValue = "coderef.references.file";
  }
}

/** One top-level category — its children are FileNodes. */
class CategoryNode extends vscode.TreeItem {
  constructor(
    public readonly category: string,
    public readonly refs: EngineReference[],
  ) {
    const glyph = glyphOf(category);
    super(`${glyph} ${category} (${refs.length})`, vscode.TreeItemCollapsibleState.Expanded);
    this.contextValue = "coderef.references.category";
  }
}

type Node = CategoryNode | FileNode | ReferenceLeaf;

/** Public for unit testing — pure function over a Reference[] + config. */
export function buildTree(
  refs: EngineReference[],
  config: LoadedConfig | undefined,
  workspaceFolderUri: vscode.Uri,
): CategoryNode[] {
  // Group by category → file.
  const byCategory = new Map<string, Map<string, EngineReference[]>>();
  for (const ref of refs) {
    const cat = categoryOf(ref, config);
    if (!byCategory.has(cat)) byCategory.set(cat, new Map());
    const byFile = byCategory.get(cat)!;
    if (!byFile.has(ref.file)) byFile.set(ref.file, []);
    byFile.get(ref.file)!.push(ref);
  }
  const cats: CategoryNode[] = [];
  for (const [cat, byFile] of byCategory) {
    const all: EngineReference[] = [];
    for (const f of byFile.values()) all.push(...f);
    cats.push(new CategoryNode(cat, all));
  }
  cats.sort((a, b) => {
    const ao = displayOrder(a.category);
    const bo = displayOrder(b.category);
    if (ao !== bo) return ao - bo;
    return a.category.localeCompare(b.category);
  });
  // Mark `_workspace` once on each cat so we can recompute children
  // without re-grouping. Use a side map.
  TREE_INDEX.set(cats, { byCategory, workspaceFolderUri });
  return cats;
}

// Side-table keyed by the returned root array. Lets `getChildren`
// resolve a CategoryNode → its FileNodes without re-grouping.
const TREE_INDEX = new WeakMap<
  CategoryNode[],
  {
    byCategory: Map<string, Map<string, EngineReference[]>>;
    workspaceFolderUri: vscode.Uri;
  }
>();

/** The TreeDataProvider VSCode registers. */
export class ReferencesTreeProvider implements vscode.TreeDataProvider<Node> {
  private readonly emitter = new vscode.EventEmitter<Node | undefined | void>();
  readonly onDidChangeTreeData = this.emitter.event;

  private roots: CategoryNode[] = [];
  private index: {
    byCategory: Map<string, Map<string, EngineReference[]>>;
    workspaceFolderUri: vscode.Uri;
  } | undefined;

  constructor(
    private readonly getConfig: () => LoadedConfig | undefined,
  ) {}

  /** Replace the tree contents with a freshly-scanned reference set. */
  setRefs(refs: EngineReference[], workspaceFolderUri: vscode.Uri): void {
    this.roots = buildTree(refs, this.getConfig(), workspaceFolderUri);
    this.index = TREE_INDEX.get(this.roots);
    this.emitter.fire();
  }

  refresh(): void {
    this.emitter.fire();
  }

  /** Flatten the current tree back to a `EngineReference[]`. Used by
   *  Copy-as-Markdown and any downstream export — the provider owns
   *  the most-recent scan, so consumers don't need a separate cache. */
  getAllRefs(): EngineReference[] {
    if (!this.index) return [];
    const out: EngineReference[] = [];
    for (const byFile of this.index.byCategory.values()) {
      for (const refs of byFile.values()) out.push(...refs);
    }
    return out;
  }

  getTreeItem(element: Node): vscode.TreeItem {
    return element;
  }

  getChildren(element?: Node): Node[] {
    if (!element) return this.roots;
    if (element instanceof CategoryNode) {
      if (!this.index) return [];
      const folder = this.index.workspaceFolderUri;
      const byFile = this.index.byCategory.get(element.category);
      if (!byFile) return [];
      const files: FileNode[] = [];
      for (const [file, refs] of byFile) {
        const uri = vscode.Uri.joinPath(folder, file);
        files.push(new FileNode(file, uri, refs));
      }
      files.sort((a, b) => a.file.localeCompare(b.file));
      return files;
    }
    if (element instanceof FileNode) {
      return element.refs
        .slice()
        .sort((a, b) => a.byte_start - b.byte_start)
        .map((r) => new ReferenceLeaf(r, element.fileUri));
    }
    return [];
  }
}

/** Walk the workspace, scan every in-scope file, and update the tree. */
export async function rescanWorkspace(
  provider: ReferencesTreeProvider,
  getConfig: () => LoadedConfig | undefined,
): Promise<void> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (!folder) {
    provider.setRefs([], vscode.Uri.file("/"));
    return;
  }
  const config = getConfig();
  if (!config) {
    provider.setRefs([], folder.uri);
    return;
  }

  // findFiles respects .gitignore + the supplied exclude. We only
  // exclude obvious noise here; per-pattern scope.exclude is honoured
  // by the engine itself.
  const ignore = config.config.ignore ?? [];
  const excludeGlob =
    ignore.length > 0
      ? `{${ignore.join(",")}}`
      : "{**/node_modules/**,**/target/**,**/.git/**,**/out/**,**/pkg/**}";

  const uris = await vscode.workspace.findFiles("**/*", excludeGlob);
  const allRefs: EngineReference[] = [];
  for (const uri of uris) {
    try {
      const doc = await vscode.workspace.openTextDocument(uri);
      const ext = extOf(doc.uri.fsPath);
      const rel = vscode.workspace.asRelativePath(uri, false);
      const refs = scanBuffer(doc.getText(), config.config, ext, rel);
      allRefs.push(...refs);
    } catch (err) {
      // Skip files that can't be read (binary, oversized). The browser
      // is best-effort.
      console.warn("coderef: skipped file in references scan", uri.toString(), err);
    }
  }
  provider.setRefs(allRefs, folder.uri);
}

function extOf(fsPath: string): string | undefined {
  const dot = fsPath.lastIndexOf(".");
  if (dot === -1 || dot === fsPath.length - 1) return undefined;
  return fsPath.slice(dot + 1);
}

/** Render a reference set as a category-grouped Markdown document.
 *
 *  Pure function — testable without VSCode. Used by the
 *  Copy-as-Markdown command (DESIGN.md §14.7 v0.3 long tail). The
 *  result is suitable for pasting into PR descriptions, design docs,
 *  ticketing systems, etc.
 *
 *  Layout:
 *    # coderef references
 *    _N references across F files in C categories_
 *
 *    ## 🎫 tickets (5)
 *
 *    ### src/lib.rs
 *    - `src/lib.rs:12` — `[jira] JIRA(PROJ-1)` → https://jira.example/PROJ-1
 *    ...
 *
 *  We use a list rather than a markdown table on purpose: table
 *  formatting requires column alignment to round-trip cleanly through
 *  table-aware reformatters, and the alignment cost dominates the
 *  layout. A bullet list reads well in any renderer and survives
 *  edits.
 */
export function renderReferencesAsMarkdown(
  refs: EngineReference[],
  config: LoadedConfig | undefined,
): string {
  const lines: string[] = [];
  lines.push("# coderef references");
  lines.push("");
  if (refs.length === 0) {
    lines.push("_No references in the current scan._");
    return lines.join("\n");
  }

  // Group by category → file the same way the tree does.
  const byCategory = new Map<string, Map<string, EngineReference[]>>();
  for (const ref of refs) {
    const cat = categoryOf(ref, config);
    if (!byCategory.has(cat)) byCategory.set(cat, new Map());
    const byFile = byCategory.get(cat)!;
    if (!byFile.has(ref.file)) byFile.set(ref.file, []);
    byFile.get(ref.file)!.push(ref);
  }
  const categories = [...byCategory.keys()].sort((a, b) => {
    const ao = displayOrder(a);
    const bo = displayOrder(b);
    if (ao !== bo) return ao - bo;
    return a.localeCompare(b);
  });
  const fileSet = new Set<string>();
  for (const byFile of byCategory.values()) for (const f of byFile.keys()) fileSet.add(f);
  lines.push(
    `_${refs.length} reference${refs.length === 1 ? "" : "s"} across ` +
      `${fileSet.size} file${fileSet.size === 1 ? "" : "s"} in ` +
      `${categories.length} categor${categories.length === 1 ? "y" : "ies"}._`,
  );
  lines.push("");

  for (const cat of categories) {
    const byFile = byCategory.get(cat)!;
    const total = [...byFile.values()].reduce((sum, refs) => sum + refs.length, 0);
    lines.push(`## ${glyphOf(cat)} ${cat} (${total})`);
    lines.push("");
    const files = [...byFile.keys()].sort();
    for (const file of files) {
      lines.push(`### ${file}`);
      lines.push("");
      const fileRefs = byFile.get(file)!.slice().sort((a, b) => a.byte_start - b.byte_start);
      for (const ref of fileRefs) {
        const matched = escapeBackticks(ref.matched_text);
        lines.push(
          `- \`${file}:${ref.line}\` — \`[${ref.pattern_id}] ${matched}\` → ${ref.target}`,
        );
      }
      lines.push("");
    }
  }

  return lines.join("\n").replace(/\n+$/, "\n");
}

function escapeBackticks(s: string): string {
  return s.replace(/`/g, "\\`");
}

/** `coderef.references.copyAsMarkdown` command body. Pulls the current
 *  reference set from the provider, renders it via
 *  `renderReferencesAsMarkdown`, copies to the clipboard, and surfaces
 *  a short status-bar acknowledgement. Returns the rendered text for
 *  test introspection.
 */
export async function copyReferencesAsMarkdownCommand(
  provider: ReferencesTreeProvider,
  getConfig: () => LoadedConfig | undefined,
  api: {
    clipboardWrite: (text: string) => Promise<void>;
    showInfo: (msg: string) => void;
  } = {
    clipboardWrite: (text) => Promise.resolve(vscode.env.clipboard.writeText(text)),
    showInfo: (msg) => void vscode.window.showInformationMessage(msg),
  },
): Promise<string> {
  const refs = provider.getAllRefs();
  const markdown = renderReferencesAsMarkdown(refs, getConfig());
  await api.clipboardWrite(markdown);
  if (refs.length === 0) {
    api.showInfo("coderef: copied an empty-references stub to the clipboard.");
  } else {
    api.showInfo(
      `coderef: copied ${refs.length} reference${refs.length === 1 ? "" : "s"} as Markdown.`,
    );
  }
  return markdown;
}
