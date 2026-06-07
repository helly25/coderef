// Unit tests for the pure parts of providers.ts.
//
// VSCode integration tests (running the extension in an Extension
// Host) are tracked in docs/test-plan.md; @vscode/test-electron is
// the established path and is a follow-up PR.

import assert from "node:assert/strict";
import { test } from "node:test";

// Mock the `vscode` module for tests. Node-test runs in plain Node
// where `vscode` isn't on the resolver path; we override before our
// own modules import it.
import { type EngineReference } from "./wasmEngine";

// --- Minimal `vscode` API surface used by providers.ts -------------
// We can't import the real `vscode` module outside the extension host;
// mock the bits used at runtime so the pure-function paths still run.

declare const globalThis: { mockVscodeRegistered?: boolean };

function registerVscodeMock(): void {
  if (globalThis.mockVscodeRegistered) return;
  globalThis.mockVscodeRegistered = true;
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const Module = require("node:module");
  const orig = Module._resolveFilename;
  Module._resolveFilename = function (request: string, ...rest: unknown[]): string {
    if (request === "vscode") return require.resolve("./__vscode_mock");
    return orig.call(this, request, ...rest);
  };
}

registerVscodeMock();

// Import after the mock is registered so the require() inside resolves.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const providers: typeof import("./providers") = require("./providers");

function r(overrides: Partial<EngineReference> = {}): EngineReference {
  return {
    pattern_id: "todo",
    pattern_kind: "url",
    file: "src/x.rs",
    line: 1,
    column: 4,
    byte_start: 0,
    byte_end: 10,
    matched_text: "TODO(@a)",
    captures: { user: "a" },
    target: "https://example.com/a",
    title: null,
    in_comment: true,
    ...overrides,
  };
}

test("linkTargetFor url kind parses as Uri", () => {
  const fakeDoc = { uri: { scheme: "file", fsPath: "/repo/src/x.rs" } } as unknown as import("vscode").TextDocument;
  const uri = providers.linkTargetFor(fakeDoc, r({ pattern_kind: "url", target: "https://example.com/a" }));
  assert.equal(uri.toString(), "https://example.com/a");
});

test("linkTargetFor local kind anchors at workspace root via leading slash", () => {
  const fakeDoc = { uri: { scheme: "file", fsPath: "/repo/src/x.rs" } } as unknown as import("vscode").TextDocument;
  // Mock workspace folder.
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  vscode.workspace.getWorkspaceFolder = () => ({ uri: { fsPath: "/repo" } });
  const uri = providers.linkTargetFor(
    fakeDoc,
    r({ pattern_kind: "local", target: "/docs/foo.md" }),
  );
  assert.ok(uri.toString().endsWith("/repo/docs/foo.md"), `got: ${uri.toString()}`);
});

test("linkTargetFor local kind without leading slash also resolves under root", () => {
  const fakeDoc = { uri: { scheme: "file", fsPath: "/repo/src/x.rs" } } as unknown as import("vscode").TextDocument;
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  vscode.workspace.getWorkspaceFolder = () => ({ uri: { fsPath: "/repo" } });
  const uri = providers.linkTargetFor(
    fakeDoc,
    r({ pattern_kind: "local", target: "docs/bar.md" }),
  );
  assert.ok(uri.toString().endsWith("/repo/docs/bar.md"), `got: ${uri.toString()}`);
});

test("linkTargetFor local kind without workspace folder falls back to file dir", () => {
  const fakeDoc = { uri: { scheme: "file", fsPath: "/lone/x.rs" } } as unknown as import("vscode").TextDocument;
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  vscode.workspace.getWorkspaceFolder = () => undefined;
  const uri = providers.linkTargetFor(
    fakeDoc,
    r({ pattern_kind: "local", target: "buddy.md" }),
  );
  assert.ok(uri.toString().endsWith("/lone/buddy.md"), `got: ${uri.toString()}`);
});

test("buildHoverMarkdown includes pattern id, kind, title, and target", () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  const target = vscode.Uri.parse("https://example.com/a");
  const md = providers.buildHoverMarkdown(
    r({ pattern_id: "todo-user", pattern_kind: "url", title: "User: alice", target: "https://example.com/a" }),
    undefined,
    target,
  );
  const value: string = (md as { value: string }).value;
  assert.match(value, /`todo-user`/);
  assert.match(value, /\(url\)/);
  assert.match(value, /User: alice/);
  assert.match(value, /example\.com/);
});

test("buildHoverMarkdown renders description above title when provided", () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  const target = vscode.Uri.parse("https://example.com/a");
  const md = providers.buildHoverMarkdown(
    r({ pattern_id: "todo-user", title: "User: alice" }),
    "GitHub @user mention inside a TODO marker.",
    target,
  );
  const value: string = (md as { value: string }).value;
  // Description text should appear before the title text in the
  // rendered markdown.
  const descIdx = value.indexOf("GitHub @user mention");
  const titleIdx = value.indexOf("User: alice");
  assert.ok(descIdx >= 0, `description missing: ${value}`);
  assert.ok(titleIdx >= 0, `title missing: ${value}`);
  assert.ok(descIdx < titleIdx, `description should precede title: ${value}`);
});

test("buildHoverMarkdown omits description and title when neither is set", () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  const target = vscode.Uri.parse("https://example.com/a");
  const md = providers.buildHoverMarkdown(
    r({ pattern_id: "x", title: null }),
    undefined,
    target,
  );
  const value: string = (md as { value: string }).value;
  // Only the header + target line should be there.
  assert.match(value, /`x`/);
  assert.match(value, /→/);
});
