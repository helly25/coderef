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
