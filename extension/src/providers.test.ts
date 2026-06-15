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
import { registerVscodeMock } from "./__test_setup";

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

// ---------------------------------------------------------------------
// buildHoverMarkdown — multi-target alternates (v0.4 long tail).
// ---------------------------------------------------------------------

test("buildHoverMarkdown omits the alternates section when none are passed", () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  const md = providers.buildHoverMarkdown(r({}), undefined, vscode.Uri.parse("https://a/"), []);
  const value: string = (md as { value: string }).value;
  assert.doesNotMatch(value, /Alternative target/);
});

test("buildHoverMarkdown lists each alternate target with its kind, link, and optional title", () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  const primary = vscode.Uri.parse("https://primary.example/A");
  const md = providers.buildHoverMarkdown(
    r({ pattern_id: "primary", pattern_kind: "url", target: "https://primary.example/A" }),
    undefined,
    primary,
    [
      {
        pattern_id: "alt-jira",
        pattern_kind: "url",
        target: "https://jira.example/A",
        uri: vscode.Uri.parse("https://jira.example/A"),
        title: "JIRA ticket A",
      },
      {
        pattern_id: "alt-github",
        pattern_kind: "url",
        target: "https://github.com/issues/A",
        uri: vscode.Uri.parse("https://github.com/issues/A"),
        title: null,
      },
    ],
  );
  const value: string = (md as { value: string }).value;
  // Section header pluralisation: 2 alternates → "Alternative targets (2):".
  assert.match(value, /\*\*Alternative targets \(2\):\*\*/);
  // Each alternate appears as a bullet with pattern id + link. The
  // visible-text portion of the link goes through escapeMarkdown so
  // dots are escaped (`jira\.example/A`); the URL in `(...)` stays
  // unescaped. Match on the inner-link URL since it's the
  // reformatting-stable bit.
  assert.match(value, /`alt-jira` \(url\) → .*?\]\(https:\/\/jira\.example\/A\)/);
  assert.match(value, /`alt-github` \(url\) → .*?\]\(https:\/\/github\.com\/issues\/A\)/);
  // Title shows on the alt that has one; not on the one without.
  assert.match(value, /JIRA ticket A/);
  // Primary still rendered above.
  assert.match(value, /`primary`/);
});

test("buildHoverMarkdown uses singular grammar when exactly one alternate is present", () => {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const vscode = require("vscode");
  const md = providers.buildHoverMarkdown(
    r({}),
    undefined,
    vscode.Uri.parse("https://a/"),
    [
      {
        pattern_id: "alt-only",
        pattern_kind: "url",
        target: "https://b/",
        uri: vscode.Uri.parse("https://b/"),
        title: null,
      },
    ],
  );
  const value: string = (md as { value: string }).value;
  // Singular header (no parenthetical count for a single alt).
  assert.match(value, /\*\*Alternative target:\*\*/);
  assert.doesNotMatch(value, /Alternative target \(/);
});
