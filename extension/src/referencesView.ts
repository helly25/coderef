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
//   - Multi-target hover alternates
//   - references.tooManyNodes / uncategorisedSpike doctor checks
//   - maxNodesPerLevel cap
//
// Shipped in v0.3:
//   - Copy-as-Markdown command (this file: getAllRefs +
//     renderReferencesAsMarkdown).
//   - exportJson command (this file: serializeReferencesForExport +
//     exportReferencesAsJsonCommand).
//   - maxNodesPerLevel cap (this file: capLevel + TruncatedNode),
//     wired to the `coderef.references.maxNodesPerLevel` setting.
//
// Shipped in v0.4:
//   - Scan modes (workspace / openFiles / currentFile) +
//     setScanMode command.
//   - Mine filter (this file: isReferenceMine + filter wired to
//     `coderef.references.filter` + `mineIdentities` settings +
//     setFilter command).

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

/** Default cap for nodes per tree level. Mirrors DESIGN §14.7.3.
 *  Overridable via the `coderef.references.maxNodesPerLevel` setting.
 *  When a level exceeds the cap, the tree shows the first N entries
 *  and appends a truncation placeholder; the doctor's
 *  `references.tooManyNodes` diagnostic surfaces the same condition
 *  ahead of viewing the tree.
 */
const DEFAULT_MAX_NODES_PER_LEVEL = 1000;

/** Read `coderef.references.maxNodesPerLevel` honouring overrides
 *  and falling back to {@link DEFAULT_MAX_NODES_PER_LEVEL}. Capped
 *  at the minimum of 10 so a hostile-low setting still leaves room
 *  for the truncation placeholder.
 */
function maxNodesPerLevelSetting(): number {
  const cfg = vscode.workspace.getConfiguration("coderef.references");
  const raw = cfg.get<number>("maxNodesPerLevel", DEFAULT_MAX_NODES_PER_LEVEL);
  if (typeof raw !== "number" || !Number.isFinite(raw) || raw < 10) {
    return DEFAULT_MAX_NODES_PER_LEVEL;
  }
  return Math.floor(raw);
}

/** Truncation placeholder shown at a level the cap clipped. Pure
 *  presentational — clicking it does nothing.
 */
class TruncatedNode extends vscode.TreeItem {
  constructor(hidden: number, cap: number) {
    super(
      `…and ${hidden} more (cap: ${cap}, set \`coderef.references.maxNodesPerLevel\` to raise)`,
      vscode.TreeItemCollapsibleState.None,
    );
    this.iconPath = new vscode.ThemeIcon("ellipsis");
    this.contextValue = "coderef.references.truncated";
  }
}

/** Cap an array at `max` entries; when truncated, returns the
 *  prefix plus a `TruncatedNode` describing the hidden count.
 *  Pure (excepting the TreeItem subtype) so it's testable.
 */
export function capLevel<T>(items: T[], render: (t: T) => Node, max: number): Node[] {
  if (items.length <= max) return items.map(render);
  const kept = items.slice(0, max).map(render);
  kept.push(new TruncatedNode(items.length - max, max));
  return kept;
}

/** Exported for tests. */
export function _maxNodesPerLevelSettingForTests(): number {
  return maxNodesPerLevelSetting();
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

type Node = CategoryNode | FileNode | ReferenceLeaf | TruncatedNode;

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
    const cap = maxNodesPerLevelSetting();
    if (element instanceof CategoryNode) {
      if (!this.index) return [];
      const folder = this.index.workspaceFolderUri;
      const byFile = this.index.byCategory.get(element.category);
      if (!byFile) return [];
      const entries = [...byFile.entries()].sort(([a], [b]) => a.localeCompare(b));
      return capLevel(
        entries,
        ([file, refs]) => new FileNode(file, vscode.Uri.joinPath(folder, file), refs),
        cap,
      );
    }
    if (element instanceof FileNode) {
      const ordered = element.refs.slice().sort((a, b) => a.byte_start - b.byte_start);
      return capLevel(ordered, (r) => new ReferenceLeaf(r, element.fileUri), cap);
    }
    return [];
  }
}

/** Filter mode for the references browser. `all` shows every ref;
 *  `mine` shows only those whose matched_text or any capture value
 *  contains one of the identifiers in `coderef.references.mineIdentities`.
 *  The identity-driven match is intentionally substring-based and
 *  case-insensitive — the identifier list is short, the user-facing
 *  use case is "show me my outstanding TODOs across the repo", and
 *  the simplest predicate that surfaces the user-name in TODOs,
 *  JIRA assignee captures, etc. wins.
 */
export type FilterMode = "all" | "mine";

/** Read `coderef.references.filter`. Falls back to `all` on
 *  missing / unknown values.
 */
export function filterModeSetting(): FilterMode {
  const cfg = vscode.workspace.getConfiguration("coderef.references");
  const raw = cfg.get<string>("filter", "all");
  return raw === "mine" ? "mine" : "all";
}

/** Read `coderef.references.mineIdentities` (string array). Trimmed
 *  and de-empty-stringed. The user might enter literal identifiers
 *  (`alice`, `@alice`, `[email protected]`) — we don't transform them; the
 *  match is a plain case-insensitive substring check.
 */
export function mineIdentitiesSetting(): string[] {
  const cfg = vscode.workspace.getConfiguration("coderef.references");
  const raw = cfg.get<unknown>("mineIdentities", []);
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((s): s is string => typeof s === "string")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** Predicate for the Mine filter. Returns `true` when the ref's
 *  `matched_text` or any of its capture values contains any of the
 *  provided identifiers (case-insensitive substring match). When the
 *  identifier list is empty, every ref is treated as Mine — there's
 *  no other way to distinguish "yours" from "theirs", and treating
 *  an unconfigured Mine filter as an empty result would be a
 *  confusing UX. Pure function — testable.
 */
export function isReferenceMine(
  ref: EngineReference,
  identities: readonly string[],
): boolean {
  if (identities.length === 0) return true;
  const haystacks: string[] = [ref.matched_text];
  for (const v of Object.values(ref.captures)) haystacks.push(v);
  const haystack = haystacks.join("\x00").toLowerCase();
  return identities.some((id) => haystack.includes(id.toLowerCase()));
}

/** Apply the active filter (read from settings) to a reference set.
 *  Returns a new array; the caller's slice is untouched. */
export function applyFilter(refs: EngineReference[]): EngineReference[] {
  if (filterModeSetting() === "all") return refs;
  const ids = mineIdentitiesSetting();
  return refs.filter((r) => isReferenceMine(r, ids));
}

/** Scan mode for the references browser (DESIGN §14.7). Determines
 *  which subset of files the next rescan walks. Sourced from the
 *  `coderef.references.scanMode` setting; defaults to `workspace`.
 */
export type ScanMode = "workspace" | "openFiles" | "currentFile";

/** Read `coderef.references.scanMode` and normalise to a recognised
 *  enum value. Unknown / missing values fall back to `workspace`.
 */
export function scanModeSetting(): ScanMode {
  const cfg = vscode.workspace.getConfiguration("coderef.references");
  const raw = cfg.get<string>("scanMode", "workspace");
  switch (raw) {
    case "workspace":
    case "openFiles":
    case "currentFile":
      return raw;
    default:
      return "workspace";
  }
}

/** Collect the URI set to scan for a given mode. Pure-ish (depends on
 *  the live editor state for `currentFile` / `openFiles`). For
 *  `workspace`, delegates to `findFiles` with the standard exclude
 *  glob so the result matches the v0.2 behaviour exactly.
 */
async function collectScanUris(
  mode: ScanMode,
  excludeGlob: string,
): Promise<readonly vscode.Uri[]> {
  switch (mode) {
    case "workspace":
      return await vscode.workspace.findFiles("**/*", excludeGlob);
    case "openFiles": {
      // textDocuments includes every doc the extension host knows
      // about — open editors, plus a few transient items. Filter to
      // file-scheme URIs so output/scratchpad/walk-through docs
      // don't sneak in.
      return vscode.workspace.textDocuments
        .filter((d) => d.uri.scheme === "file")
        .map((d) => d.uri);
    }
    case "currentFile": {
      const active = vscode.window.activeTextEditor?.document;
      if (active && active.uri.scheme === "file") return [active.uri];
      return [];
    }
  }
}

/** Walk the workspace (or a subset, per `scanMode`), scan every
 *  in-scope file, and update the tree.
 */
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

  const mode = scanModeSetting();
  const uris = await collectScanUris(mode, excludeGlob);
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
  provider.setRefs(applyFilter(allRefs), folder.uri);
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

/** Shape of the JSON document emitted by `exportJson`. A stable
 *  schema — downstream tooling (dashboards, audits, scripts) can rely
 *  on the field names below across coderef releases. New fields will
 *  be additive (no breaking changes within `schema: 1`).
 */
export interface ExportedReferences {
  /** Schema version. Bumps on breaking shape changes. */
  schema: 1;
  /** ISO-8601 UTC timestamp at export time. */
  generated_at: string;
  /** Engine version (`coderef-core <X.Y.Z>`) snapshotted at export
   *  time so an audit trail records exactly which engine produced
   *  the dump. */
  engine: string;
  /** Totals for a quick at-a-glance summary at the top of the file.
   *  Per-file / per-category counts are derivable from `references`. */
  totals: {
    references: number;
    files: number;
    categories: number;
  };
  references: ExportedReference[];
}

/** A single reference entry in the exported JSON. Mirrors
 *  `EngineReference` plus the resolved category for one-shot
 *  grouping downstream without needing the config.
 */
export interface ExportedReference {
  pattern_id: string;
  pattern_kind: string;
  category: string;
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

/** Serialise a reference set into the stable `ExportedReferences`
 *  shape. Pure function — testable without VSCode. Used by the
 *  exportJson command and any future downstream exporter.
 */
export function serializeReferencesForExport(
  refs: EngineReference[],
  config: LoadedConfig | undefined,
  engine: string,
  now: Date = new Date(),
): ExportedReferences {
  const fileSet = new Set<string>();
  const catSet = new Set<string>();
  const entries: ExportedReference[] = refs.map((r) => {
    fileSet.add(r.file);
    const cat = categoryOf(r, config);
    catSet.add(cat);
    return {
      pattern_id: r.pattern_id,
      pattern_kind: r.pattern_kind,
      category: cat,
      file: r.file,
      line: r.line,
      column: r.column,
      byte_start: r.byte_start,
      byte_end: r.byte_end,
      matched_text: r.matched_text,
      captures: r.captures,
      target: r.target,
      title: r.title,
      in_comment: r.in_comment,
    };
  });
  // Sort entries deterministically so the JSON output is diffable
  // across runs (file, then byte_start).
  entries.sort((a, b) => {
    if (a.file !== b.file) return a.file.localeCompare(b.file);
    return a.byte_start - b.byte_start;
  });
  return {
    schema: 1,
    generated_at: now.toISOString(),
    engine,
    totals: {
      references: entries.length,
      files: fileSet.size,
      categories: catSet.size,
    },
    references: entries,
  };
}

/** `coderef.references.exportJson` command body. Asks the user to
 *  pick an output file, serialises the current reference set, and
 *  writes the JSON document. Surfaced both via the command palette
 *  and a save-icon button in the view title bar.
 *
 *  The `api?` parameter mirrors `copyReferencesAsMarkdownCommand`:
 *  defaults to live `vscode.window.showSaveDialog` / fs writes /
 *  `vscode.window.showInformationMessage`, but accepts mocks for
 *  tests so the pure path is exercisable without a VSCode runtime.
 */
export async function exportReferencesAsJsonCommand(
  provider: ReferencesTreeProvider,
  getConfig: () => LoadedConfig | undefined,
  engine: string,
  api: {
    pickPath: () => Promise<vscode.Uri | undefined>;
    writeFile: (uri: vscode.Uri, content: Uint8Array) => Promise<void>;
    showInfo: (msg: string) => void;
    showWarn: (msg: string) => void;
  } = {
    pickPath: async () =>
      await vscode.window.showSaveDialog({
        defaultUri: defaultExportUri(),
        filters: { JSON: ["json"] },
        saveLabel: "Export references",
      }),
    writeFile: async (uri, content) => {
      await vscode.workspace.fs.writeFile(uri, content);
    },
    showInfo: (msg) => void vscode.window.showInformationMessage(msg),
    showWarn: (msg) => void vscode.window.showWarningMessage(msg),
  },
): Promise<vscode.Uri | undefined> {
  const refs = provider.getAllRefs();
  if (refs.length === 0) {
    api.showWarn(
      "coderef: no references in the current scan — nothing to export. Refresh first.",
    );
    return undefined;
  }
  const target = await api.pickPath();
  if (!target) {
    // User cancelled the save dialog — silent (no notification).
    return undefined;
  }
  const doc = serializeReferencesForExport(refs, getConfig(), engine);
  const text = JSON.stringify(doc, null, 2);
  await api.writeFile(target, new TextEncoder().encode(text));
  api.showInfo(
    `coderef: exported ${refs.length} reference${refs.length === 1 ? "" : "s"} to ${target.fsPath}`,
  );
  return target;
}

function defaultExportUri(): vscode.Uri | undefined {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (!folder) return undefined;
  return vscode.Uri.joinPath(folder.uri, "coderef-references.json");
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

/** `coderef.references.setFilter` command body. Quick-pick between
 *  `all` and `mine`; persists the choice and triggers a rescan so
 *  the tree reflects the new filter immediately. `api?` lets tests
 *  inject mocks for the quick-pick + settings.update + rescan calls.
 */
export async function setFilterCommand(
  rescan: () => void | Promise<void>,
  api: {
    pickFilter: () => Promise<FilterMode | undefined>;
    setFilter: (mode: FilterMode) => Promise<void>;
    showInfo: (msg: string) => void;
  } = {
    pickFilter: async () => {
      const items: { label: string; description: string; mode: FilterMode }[] = [
        {
          label: "all",
          description: "Show every reference (default)",
          mode: "all",
        },
        {
          label: "mine",
          description: "Only refs matching coderef.references.mineIdentities",
          mode: "mine",
        },
      ];
      const picked = await vscode.window.showQuickPick(items, {
        placeHolder: "coderef: references filter",
        ignoreFocusOut: false,
      });
      return picked?.mode;
    },
    setFilter: async (mode) => {
      const cfg = vscode.workspace.getConfiguration("coderef.references");
      const target =
        vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0
          ? vscode.ConfigurationTarget.Workspace
          : vscode.ConfigurationTarget.Global;
      await cfg.update("filter", mode, target);
    },
    showInfo: (msg) => void vscode.window.showInformationMessage(msg),
  },
): Promise<FilterMode | undefined> {
  const picked = await api.pickFilter();
  if (!picked) return undefined;
  await api.setFilter(picked);
  api.showInfo(`coderef: references filter → ${picked}`);
  await rescan();
  return picked;
}

/** `coderef.references.setScanMode` command body. Pops a quick-pick
 *  with the three modes, writes the choice to the workspace settings
 *  (or global if there's no workspace), and triggers a rescan so the
 *  tree reflects the new mode immediately. `api?` lets tests inject
 *  mocks for the quick-pick + settings.update + rescan calls.
 */
export async function setScanModeCommand(
  rescan: () => void | Promise<void>,
  api: {
    pickMode: () => Promise<ScanMode | undefined>;
    setMode: (mode: ScanMode) => Promise<void>;
    showInfo: (msg: string) => void;
  } = {
    pickMode: async () => {
      const items: { label: string; description: string; mode: ScanMode }[] = [
        {
          label: "workspace",
          description: "Scan every in-scope file (.gitignore + config ignore[] honoured)",
          mode: "workspace",
        },
        {
          label: "openFiles",
          description: "Scan only files currently open as TextDocuments",
          mode: "openFiles",
        },
        {
          label: "currentFile",
          description: "Scan only the active editor's file",
          mode: "currentFile",
        },
      ];
      const picked = await vscode.window.showQuickPick(items, {
        placeHolder: "coderef: references scan mode",
        ignoreFocusOut: false,
      });
      return picked?.mode;
    },
    setMode: async (mode) => {
      const cfg = vscode.workspace.getConfiguration("coderef.references");
      // Workspace settings take precedence if a workspace is open;
      // otherwise fall back to user-global so the value still sticks.
      const target =
        vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0
          ? vscode.ConfigurationTarget.Workspace
          : vscode.ConfigurationTarget.Global;
      await cfg.update("scanMode", mode, target);
    },
    showInfo: (msg) => void vscode.window.showInformationMessage(msg),
  },
): Promise<ScanMode | undefined> {
  const picked = await api.pickMode();
  if (!picked) return undefined; // user cancelled
  await api.setMode(picked);
  api.showInfo(`coderef: references scan mode → ${picked}`);
  await rescan();
  return picked;
}
