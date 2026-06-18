// Unit tests for the pure tree-building part of referencesView.
//
// The TreeDataProvider itself depends on vscode runtime (EventEmitter,
// commands, FileSystemWatcher); those are exercised via the runtime
// test under @vscode/test-electron. The pure `buildTree` function is
// the focus here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { registerVscodeMock } from "./__test_setup";

registerVscodeMock();

// Imports after the vscode mock is registered so the require() inside
// resolves.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const referencesView: typeof import("./referencesView") = require("./referencesView");
// eslint-disable-next-line @typescript-eslint/no-require-imports
const vscodeMock: typeof import("vscode") = require("vscode");
import { type EngineReference } from "./wasmEngine";
import { type LoadedConfig } from "./configLoader";

function r(overrides: Partial<EngineReference> = {}): EngineReference {
  return {
    pattern_id: "todo-user",
    pattern_kind: "url",
    file: "src/a.rs",
    line: 1,
    column: 1,
    byte_start: 0,
    byte_end: 10,
    matched_text: "TODO(@alice)",
    captures: { user: "alice" },
    target: "https://github.com/alice",
    title: null,
    in_comment: true,
    ...overrides,
  };
}

function cfg(patterns: Record<string, { kind?: string; category?: string }>): LoadedConfig {
  return {
    path: "/tmp/.coderef.jsonc",
    config: {
      patterns: Object.fromEntries(
        Object.entries(patterns).map(([id, p]) => [
          id,
          { regex: "X", ...(p.kind ? { kind: p.kind } : {}), ...(p.category ? { category: p.category } : {}) },
        ]),
      ),
    } as unknown as LoadedConfig["config"],
  };
}

const FOLDER = vscodeMock.Uri.file("/repo");

test("buildTree empty refs yields no categories", () => {
  const roots = referencesView.buildTree([], undefined, FOLDER);
  assert.equal(roots.length, 0);
});

test("buildTree groups refs by declared category", () => {
  const refs = [
    r({ pattern_id: "todo-user", file: "src/a.rs" }),
    r({ pattern_id: "todo-user", file: "src/b.rs" }),
    r({ pattern_id: "docref", pattern_kind: "url", file: "src/c.rs" }),
  ];
  const c = cfg({
    "todo-user": { category: "people" },
    docref: { category: "files" },
  });
  const roots = referencesView.buildTree(refs, c, FOLDER);
  // files first (display order 0), then people (1).
  assert.equal(roots.length, 2);
  assert.match(roots[0]!.label as string, /files/);
  assert.match(roots[1]!.label as string, /people/);
});

test("buildTree infers category from kind when undeclared", () => {
  const refs = [
    r({ pattern_id: "todo", pattern_kind: "url" }),
    r({ pattern_id: "docref", pattern_kind: "local" }),
    r({ pattern_id: "ic", pattern_kind: "ifchange" }),
  ];
  const c = cfg({ todo: {}, docref: { kind: "local" }, ic: { kind: "ifchange" } });
  const roots = referencesView.buildTree(refs, c, FOLDER);
  // files (local→files, order 0), coupled-change (ifchange, 5), other (url, max).
  const cats = roots.map((r) => (r.label as string).match(/[a-z-]+/)![0]);
  assert.deepEqual(cats, ["files", "coupled-change", "other"]);
});

test("buildTree sorts user-defined categories between coupled-change and other", () => {
  const refs = [
    r({ pattern_id: "slack", pattern_kind: "url" }),
    r({ pattern_id: "other-url", pattern_kind: "url" }),
    r({ pattern_id: "ifc", pattern_kind: "ifchange" }),
  ];
  const c = cfg({
    slack: { category: "slack-channels" },
    "other-url": { category: "other" },
    ifc: { kind: "ifchange" },
  });
  const roots = referencesView.buildTree(refs, c, FOLDER);
  const cats = roots.map((r) => (r.label as string).match(/[a-z-]+/)![0]);
  // coupled-change first (5), user-defined next (100), other last (MAX).
  assert.deepEqual(cats, ["coupled-change", "slack-channels", "other"]);
});

test("buildTree uses 🏷 glyph for user-defined categories", () => {
  const refs = [r({ pattern_id: "slack" })];
  const c = cfg({ slack: { category: "slack-channels" } });
  const roots = referencesView.buildTree(refs, c, FOLDER);
  assert.equal(roots.length, 1);
  // Tag glyph + name.
  assert.match(roots[0]!.label as string, /^🏷 slack-channels/);
});

test("buildTree counts refs per category in the label", () => {
  const refs = [
    r({ pattern_id: "todo", file: "src/a.rs" }),
    r({ pattern_id: "todo", file: "src/b.rs" }),
    r({ pattern_id: "todo", file: "src/c.rs" }),
  ];
  const c = cfg({ todo: { category: "people" } });
  const roots = referencesView.buildTree(refs, c, FOLDER);
  assert.equal(roots.length, 1);
  assert.match(roots[0]!.label as string, /\(3\)$/);
});

test("buildTree handles missing config gracefully (falls back to kind inference)", () => {
  const refs = [r({ pattern_kind: "url" })];
  const roots = referencesView.buildTree(refs, undefined, FOLDER);
  assert.equal(roots.length, 1);
  assert.match(roots[0]!.label as string, /other/);
});

// ---------------------------------------------------------------------
// renderReferencesAsMarkdown — pure markdown formatter for the
// Copy-as-Markdown command. Tests cover empty / single / multi-cat /
// multi-file shapes plus backtick escaping in matched_text.
// ---------------------------------------------------------------------

test("renderReferencesAsMarkdown emits an empty-stub when given zero refs", () => {
  const md = referencesView.renderReferencesAsMarkdown([], undefined);
  assert.match(md, /# coderef references/);
  assert.match(md, /No references in the current scan\./);
});

test("renderReferencesAsMarkdown groups by category then file and lists refs", () => {
  const refs = [
    r({ pattern_id: "todo", file: "src/a.rs", line: 12, matched_text: "TODO(@alice)" }),
    r({ pattern_id: "todo", file: "src/b.rs", line: 3, matched_text: "TODO(@bob)" }),
    r({ pattern_id: "jira", file: "docs/x.md", line: 47, matched_text: "JIRA(PROJ-1)" }),
  ];
  const c = cfg({ todo: { category: "people" }, jira: { category: "tickets" } });
  const md = referencesView.renderReferencesAsMarkdown(refs, c);

  // Header + summary.
  assert.match(md, /# coderef references/);
  assert.match(md, /3 references across 3 files in 2 categories/);
  // Category-section ordering: tickets (display 2) before people (display 1)?
  // Per DISPLAY_ORDER, files(0) < people(1) < tickets(2) — so people first.
  const peoplePos = md.indexOf("## 👤 people");
  const ticketsPos = md.indexOf("## 🎫 tickets");
  assert.ok(peoplePos !== -1 && ticketsPos !== -1);
  assert.ok(peoplePos < ticketsPos, `people should come before tickets; got md =\n${md}`);
  // File subheadings.
  assert.match(md, /### src\/a\.rs/);
  assert.match(md, /### src\/b\.rs/);
  assert.match(md, /### docs\/x\.md/);
  // Leaf format: `path:line` — `[pattern] match` → target.
  assert.match(md, /`src\/a\.rs:12` — `\[todo\] TODO\(@alice\)` → https:\/\/github\.com\/alice/);
});

test("renderReferencesAsMarkdown escapes backticks in matched text", () => {
  const refs = [r({ matched_text: "TODO `back` tick" })];
  const c = cfg({ "todo-user": { category: "people" } });
  const md = referencesView.renderReferencesAsMarkdown(refs, c);
  // Literal backticks in matched text would close the leaf's inline-
  // code span; escape them.
  assert.match(md, /TODO \\`back\\` tick/);
});

test("renderReferencesAsMarkdown handles a single reference cleanly (singular grammar)", () => {
  const refs = [r({})];
  const md = referencesView.renderReferencesAsMarkdown(refs, undefined);
  assert.match(md, /1 reference across 1 file in 1 category/);
});

// ---------------------------------------------------------------------
// serializeReferencesForExport — JSON export schema.
// ---------------------------------------------------------------------

test("serializeReferencesForExport produces a stable schema 1 envelope", () => {
  const refs = [r({})];
  const fixed = new Date("2026-06-15T12:00:00Z");
  const doc = referencesView.serializeReferencesForExport(refs, undefined, "coderef-core 0.3.0", fixed);
  assert.equal(doc.schema, 1);
  assert.equal(doc.engine, "coderef-core 0.3.0");
  assert.equal(doc.generated_at, "2026-06-15T12:00:00.000Z");
  assert.equal(doc.totals.references, 1);
  assert.equal(doc.totals.files, 1);
  assert.equal(doc.totals.categories, 1);
  assert.equal(doc.references.length, 1);
});

test("serializeReferencesForExport sorts entries by file then byte_start", () => {
  const refs = [
    r({ file: "b.rs", byte_start: 50 }),
    r({ file: "a.rs", byte_start: 200 }),
    r({ file: "a.rs", byte_start: 100 }),
  ];
  const doc = referencesView.serializeReferencesForExport(refs, undefined, "x");
  const order = doc.references.map((r) => `${r.file}:${r.byte_start}`);
  assert.deepEqual(order, ["a.rs:100", "a.rs:200", "b.rs:50"]);
});

test("serializeReferencesForExport includes the resolved category per entry", () => {
  const refs = [
    r({ pattern_id: "todo", file: "a.rs" }),
    r({ pattern_id: "jira", file: "b.rs" }),
  ];
  const c = cfg({ todo: { category: "people" }, jira: { category: "tickets" } });
  const doc = referencesView.serializeReferencesForExport(refs, c, "x");
  const cats = new Map(doc.references.map((r) => [r.file, r.category]));
  assert.equal(cats.get("a.rs"), "people");
  assert.equal(cats.get("b.rs"), "tickets");
  assert.equal(doc.totals.categories, 2);
});

test("serializeReferencesForExport falls back to kind-inferred category when config missing", () => {
  const refs = [r({ pattern_id: "anonymous", pattern_kind: "local" })];
  const doc = referencesView.serializeReferencesForExport(refs, undefined, "x");
  assert.equal(doc.references[0]!.category, "files");
});

// ---------------------------------------------------------------------
// isReferenceMine — pure predicate behind the Mine filter.
// ---------------------------------------------------------------------

test("isReferenceMine returns true for every ref when identities is empty", () => {
  // Unconfigured Mine treats every ref as yours — better UX than
  // returning an empty tree.
  const ref = r({ matched_text: "TODO(@alice)" });
  assert.equal(referencesView.isReferenceMine(ref, []), true);
});

test("isReferenceMine matches matched_text case-insensitively", () => {
  // ZZTOP-prefixed text so the workspace self-check's todo-user
  // pattern doesn't try to resolve `@Alice` as a github user
  // (would 404 in CI on a non-existent handle).
  const ref = r({ matched_text: "ZZTOP-fixture", captures: { user: "Alice" } });
  assert.equal(referencesView.isReferenceMine(ref, ["alice"]), true);
  assert.equal(referencesView.isReferenceMine(ref, ["BOB"]), false);
});

test("isReferenceMine matches any capture value", () => {
  // ZZTOP-prefixed string keeps the self-check happy.
  const ref = r({
    matched_text: "ZZTOP-bug-ref",
    captures: { ticket: "PROJ-1", assignee: "alice" },
  });
  assert.equal(referencesView.isReferenceMine(ref, ["alice"]), true);
});

test("isReferenceMine returns true on substring match (not full equality)", () => {
  // Use a matched_text shape the coderef self-check's `todo-user`
  // pattern (`TODO\(@(?<user>\w+)\)`) intentionally doesn't match —
  // no `@` prefix — so the self-scan doesn't try to resolve a
  // fictional github user from these fixture strings.
  const ref = r({ matched_text: "ZZTOP(test-fixture-string)", captures: {} });
  // Identifier substring inside matched_text still matches.
  assert.equal(referencesView.isReferenceMine(ref, ["fixture"]), true);
  // A different substring also matches.
  assert.equal(referencesView.isReferenceMine(ref, ["ZZTOP"]), true);
});

test("isReferenceMine OR-matches across multiple identities", () => {
  // Override captures to clear the default `user: "alice"` so the
  // negative-case identity list doesn't false-positive against it.
  // ZZTOP prefix keeps the self-check happy (no github 404).
  const ref = r({ matched_text: "ZZTOP-fixture-bob", captures: { user: "bob" } });
  assert.equal(referencesView.isReferenceMine(ref, ["alice", "bob", "charlie"]), true);
  assert.equal(referencesView.isReferenceMine(ref, ["alice", "charlie"]), false);
});

// ---------------------------------------------------------------------
// setFilterCommand — quick-pick + settings.update + rescan.
// ---------------------------------------------------------------------

test("setFilterCommand updates the setting and triggers a rescan when a filter is picked", async () => {
  let rescanCalled = 0;
  let updated: string | undefined;
  let info: string | undefined;
  const result = await referencesView.setFilterCommand(
    () => {
      rescanCalled += 1;
    },
    {
      pickFilter: async () => "mine",
      setFilter: async (m) => {
        updated = m;
      },
      showInfo: (msg) => {
        info = msg;
      },
    },
  );
  assert.equal(result, "mine");
  assert.equal(updated, "mine");
  assert.equal(rescanCalled, 1);
  assert.match(info ?? "", /mine/);
});

test("setFilterCommand returns undefined and skips update when the user cancels", async () => {
  let rescanCalled = 0;
  let updated: string | undefined;
  const result = await referencesView.setFilterCommand(
    () => {
      rescanCalled += 1;
    },
    {
      pickFilter: async () => undefined,
      setFilter: async (m) => {
        updated = m;
      },
      showInfo: (_msg) => {
        throw new Error("showInfo should not be called on cancel");
      },
    },
  );
  assert.equal(result, undefined);
  assert.equal(updated, undefined);
  assert.equal(rescanCalled, 0);
});

test("setFilterCommand persists the drifted mode and rescans", async () => {
  let rescanCalled = 0;
  let updated: string | undefined;
  let info: string | undefined;
  const result = await referencesView.setFilterCommand(
    () => {
      rescanCalled += 1;
    },
    {
      pickFilter: async () => "drifted",
      setFilter: async (m) => {
        updated = m;
      },
      showInfo: (msg) => {
        info = msg;
      },
    },
  );
  assert.equal(result, "drifted");
  assert.equal(updated, "drifted");
  assert.equal(rescanCalled, 1);
  assert.match(info ?? "", /drifted/);
});

// ---------------------------------------------------------------------
// computeOrphanLines — v0.5 Drifted-filter orphan detector.
// ---------------------------------------------------------------------

test("computeOrphanLines returns empty when the file has no markers", () => {
  const text = "fn main() {\n  let x = 1;\n}\n";
  const orphans = referencesView.computeOrphanLines(text);
  assert.deepEqual([...orphans], []);
});

test("computeOrphanLines returns empty when IfChange and ThenChange are balanced", () => {
  const text = ["// IfChange", "let a = 1;", "// ThenChange(/peer)"].join("\n");
  const orphans = referencesView.computeOrphanLines(text);
  assert.deepEqual([...orphans], []);
});

test("computeOrphanLines flags an unclosed IfChange open as drifted", () => {
  // IfChange on line 2, no closing ThenChange anywhere in the file.
  const text = ["fn main() {", "  // IfChange", "  let a = 1;", "}"].join("\n");
  const orphans = referencesView.computeOrphanLines(text);
  assert.deepEqual([...orphans], [2]);
});

test("computeOrphanLines flags a stray ThenChange close as drifted", () => {
  // ThenChange on line 1 with no preceding IfChange.
  const text = ["// ThenChange(/peer)", "let a = 1;"].join("\n");
  const orphans = referencesView.computeOrphanLines(text);
  assert.deepEqual([...orphans], [1]);
});

test("computeOrphanLines treats Label / EndLabel like IfChange / ThenChange", () => {
  // Open Label on line 2, no EndLabel — line 2 is orphan.
  const text = ["fn main() {", "  // Label(my-region)", "  let a = 1;", "}"].join("\n");
  const orphans = referencesView.computeOrphanLines(text);
  assert.deepEqual([...orphans], [2]);
});

test("computeOrphanLines pairs Label only with EndLabel (not ThenChange)", () => {
  // Label opens at line 1; a stray ThenChange at line 2 doesn't pair
  // with a label — so both lines are orphan.
  const text = ["// Label(x)", "// ThenChange(/peer)"].join("\n");
  const orphans = referencesView.computeOrphanLines(text);
  assert.deepEqual([...orphans].sort((a, b) => a - b), [1, 2]);
});

test("isReferenceDrifted is false for non-ifchange refs even when the line drifts", () => {
  const ref = r({
    pattern_kind: "url",
    file: "src/lib.rs",
    line: 5,
  });
  const drift: referencesView.DriftSites = new Map([["src/lib.rs", new Set([5])]]);
  assert.equal(referencesView.isReferenceDrifted(ref, drift), false);
});

test("isReferenceDrifted is true for an ifchange ref on a drifted line", () => {
  const ref = r({
    pattern_kind: "ifchange",
    file: "src/lib.rs",
    line: 5,
  });
  const drift: referencesView.DriftSites = new Map([["src/lib.rs", new Set([3, 5, 9])]]);
  assert.equal(referencesView.isReferenceDrifted(ref, drift), true);
});

test("isReferenceDrifted is false for an ifchange ref on a non-drifted line", () => {
  const ref = r({
    pattern_kind: "ifchange",
    file: "src/lib.rs",
    line: 5,
  });
  const drift: referencesView.DriftSites = new Map([["src/lib.rs", new Set([3, 9])]]);
  assert.equal(referencesView.isReferenceDrifted(ref, drift), false);
});

test("isReferenceDrifted handles files absent from the drift map", () => {
  const ref = r({
    pattern_kind: "ifchange",
    file: "src/lib.rs",
    line: 5,
  });
  const drift: referencesView.DriftSites = new Map();
  assert.equal(referencesView.isReferenceDrifted(ref, drift), false);
});

// ---------------------------------------------------------------------
// setScanModeCommand — quick-pick + settings.update + rescan. Uses
// the injectable `api` parameter so we exercise the pure choreography
// without the VSCode runtime.
// ---------------------------------------------------------------------

test("setScanModeCommand updates the setting and triggers a rescan when a mode is picked", async () => {
  let rescanCalled = 0;
  let updated: string | undefined;
  let info: string | undefined;
  const result = await referencesView.setScanModeCommand(
    () => {
      rescanCalled += 1;
    },
    {
      pickMode: async () => "openFiles",
      setMode: async (m) => {
        updated = m;
      },
      showInfo: (msg) => {
        info = msg;
      },
    },
  );
  assert.equal(result, "openFiles");
  assert.equal(updated, "openFiles");
  assert.equal(rescanCalled, 1);
  assert.match(info ?? "", /openFiles/);
});

test("setScanModeCommand returns undefined and skips update when the user cancels", async () => {
  let rescanCalled = 0;
  let updated: string | undefined;
  const result = await referencesView.setScanModeCommand(
    () => {
      rescanCalled += 1;
    },
    {
      pickMode: async () => undefined, // user dismissed the quick-pick
      setMode: async (m) => {
        updated = m;
      },
      showInfo: (_msg) => {
        // shouldn't be called
        throw new Error("showInfo should not be called on cancel");
      },
    },
  );
  assert.equal(result, undefined);
  assert.equal(updated, undefined, "settings.update must not be called on cancel");
  assert.equal(rescanCalled, 0, "rescan must not be triggered on cancel");
});

test("renderReferencesAsMarkdown sorts refs within a file by byte_start", () => {
  const refs = [
    r({ file: "a.rs", byte_start: 200, line: 20 }),
    r({ file: "a.rs", byte_start: 100, line: 10 }),
    r({ file: "a.rs", byte_start: 50, line: 5 }),
  ];
  const md = referencesView.renderReferencesAsMarkdown(refs, undefined);
  // The line numbers in the rendered output should appear in 5, 10, 20 order.
  const idx5 = md.indexOf("a.rs:5");
  const idx10 = md.indexOf("a.rs:10");
  const idx20 = md.indexOf("a.rs:20");
  assert.ok(idx5 !== -1 && idx10 !== -1 && idx20 !== -1);
  assert.ok(idx5 < idx10 && idx10 < idx20, `expected 5→10→20 order; got md =\n${md}`);
});

// ---------------------------------------------------------------------
// maxNodesPerLevel cap. Pure `capLevel` helper — render function +
// max -> bounded Node[] with a truncation placeholder when needed.
// ---------------------------------------------------------------------

test("capLevel returns all items when count <= cap", () => {
  const items = [1, 2, 3];
  const rendered = referencesView.capLevel(items, (n) => ({ label: `n${n}` } as never), 10);
  assert.equal(rendered.length, 3);
  assert.deepEqual(
    rendered.map((n) => (n as { label: string }).label),
    ["n1", "n2", "n3"],
  );
});

test("capLevel truncates to cap and appends a placeholder", () => {
  const items = [1, 2, 3, 4, 5];
  const rendered = referencesView.capLevel(items, (n) => ({ label: `n${n}` } as never), 3);
  assert.equal(rendered.length, 4); // 3 + 1 placeholder
  // First three are the rendered entries.
  assert.deepEqual(
    rendered.slice(0, 3).map((n) => (n as { label: string }).label),
    ["n1", "n2", "n3"],
  );
  // Last is a TruncatedNode — match its labelled summary.
  const last = rendered[rendered.length - 1] as { label: string };
  assert.match(last.label, /…and 2 more/);
  assert.match(last.label, /cap: 3/);
});

test("capLevel with cap=0 produces only the placeholder for non-empty input", () => {
  // Edge — the actual setting clamps low values to a sane minimum,
  // but capLevel itself respects whatever cap it's handed so it's
  // composable.
  const items = [1, 2];
  const rendered = referencesView.capLevel(items, (n) => ({ label: `n${n}` } as never), 0);
  assert.equal(rendered.length, 1);
  const placeholder = rendered[0] as { label: string };
  assert.match(placeholder.label, /…and 2 more/);
});
