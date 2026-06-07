// Unit tests for the pure helpers in commands.ts (the
// renderExplainReportAsMarkdown formatter). The command's
// VSCode-runtime side (window.activeTextEditor, openTextDocument,
// etc.) is exercised by the manual VSIX install + reload — and
// will be covered by @vscode/test-electron in a follow-up PR
// per docs/test-plan.md.

import assert from "node:assert/strict";
import { test } from "node:test";

import { registerVscodeMock } from "./__test_setup";
registerVscodeMock();

// eslint-disable-next-line @typescript-eslint/no-require-imports
const commands: typeof import("./commands") = require("./commands");
import { type EngineReference, type ExplainReport } from "./wasmEngine";

function ref(overrides: Partial<EngineReference> = {}): EngineReference {
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

function report(overrides: Partial<ExplainReport> = {}): ExplainReport {
  return {
    input: "TODO(@alice)",
    matches: [],
    non_matching_pattern_ids: [],
    ...overrides,
  };
}

test("renderExplainReportAsMarkdown: no matches → 'No matches' section", () => {
  const md = commands.renderExplainReportAsMarkdown(
    report({ non_matching_pattern_ids: ["todo", "docref"] }),
    undefined,
  );
  assert.match(md, /^# coderef explain/m);
  assert.match(md, /\*\*Input:\*\*/);
  assert.match(md, /## No matches/);
  // Both non-matching pattern ids surfaced.
  assert.match(md, /`todo`/);
  assert.match(md, /`docref`/);
});

test("renderExplainReportAsMarkdown: matches are listed with description first", () => {
  const md = commands.renderExplainReportAsMarkdown(
    report({
      matches: [
        {
          pattern_id: "todo-user",
          pattern_kind: "url",
          description: "GitHub @user marker",
          matched_text: "TODO(@alice)",
          captures: { user: "alice" },
          target: "https://github.com/alice",
          title: "GitHub profile: alice",
          priority: 0,
          scope_notes: [],
          resolution_warnings: [],
        },
      ],
    }),
    undefined,
  );
  assert.match(md, /### `todo-user` \(url\)/);
  // Description appears before the matched/target block.
  const descIdx = md.indexOf("GitHub @user marker");
  const matchedIdx = md.indexOf("matched:   TODO(@alice)");
  assert.ok(descIdx > 0 && descIdx < matchedIdx, "description should precede matched");
  // Capture line uses JSON-style quoting.
  assert.match(md, /captures:\s+user="alice"/);
  // Title line included.
  assert.match(md, /title:\s+GitHub profile: alice/);
});

test("renderExplainReportAsMarkdown: includes file location when ref provided", () => {
  const md = commands.renderExplainReportAsMarkdown(
    report(),
    ref({ file: "tests/x.rs", line: 42, column: 7 }),
  );
  assert.match(md, /tests\/x\.rs/);
  assert.match(md, /line.*42/);
  assert.match(md, /column.*7/);
});

test("renderExplainReportAsMarkdown: scope notes + warnings appear in their own subsections", () => {
  const md = commands.renderExplainReportAsMarkdown(
    report({
      matches: [
        {
          pattern_id: "x",
          pattern_kind: "url",
          description: null,
          matched_text: "X",
          captures: {},
          target: "x",
          title: null,
          priority: 0,
          scope_notes: ["commentsOnly = true", "scope.exclude = [\"docs/**\"]"],
          resolution_warnings: ["target resolution failed: missing var"],
        },
      ],
    }),
    undefined,
  );
  assert.match(md, /Scope filters that would apply/);
  assert.match(md, /- commentsOnly = true/);
  assert.match(md, /Warnings/);
  assert.match(md, /target resolution failed/);
});

test("renderExplainReportAsMarkdown: omits priority line when priority == 0", () => {
  const md = commands.renderExplainReportAsMarkdown(
    report({
      matches: [
        {
          pattern_id: "x",
          pattern_kind: "url",
          description: null,
          matched_text: "X",
          captures: {},
          target: "x",
          title: null,
          priority: 0,
          scope_notes: [],
          resolution_warnings: [],
        },
      ],
    }),
    undefined,
  );
  assert.ok(!md.includes("priority:"), `priority should be omitted: ${md}`);
});
