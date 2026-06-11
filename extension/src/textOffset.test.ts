// Unit tests for the UTF-8 byte ↔ UTF-16 position bridge.
//
// All cases run against a small fake `TextDocument` so the helpers can
// be exercised under plain Node without `@vscode/test-electron`. The
// real `TextDocument.offsetAt` / `positionAt` are documented as
// UTF-16-code-unit-based, which is what the fake reproduces.

import assert from "node:assert/strict";
import { test } from "node:test";

import { registerVscodeMock } from "./__test_setup";

registerVscodeMock();

// eslint-disable-next-line @typescript-eslint/no-require-imports
const textOffset: typeof import("./textOffset") = require("./textOffset");
// eslint-disable-next-line @typescript-eslint/no-require-imports
const vscodeMock: typeof import("vscode") = require("vscode");

/** Build a `TextDocument`-like over a fixed string. `offsetAt` /
 *  `positionAt` use UTF-16 code units (matching VSCode's contract). */
function fakeDoc(text: string): import("vscode").TextDocument {
  const lineStarts: number[] = [0];
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10 /* \n */) lineStarts.push(i + 1);
  }
  const doc = {
    getText: () => text,
    offsetAt(pos: import("vscode").Position): number {
      // Clamp to the line length so trailing positions on the last
      // line don't wander past `text.length`.
      const lineStart = lineStarts[pos.line] ?? 0;
      const lineEnd =
        pos.line + 1 < lineStarts.length ? lineStarts[pos.line + 1] - 1 : text.length;
      return Math.min(lineStart + pos.character, lineEnd);
    },
    positionAt(offset: number): import("vscode").Position {
      const o = Math.max(0, Math.min(offset, text.length));
      let lo = 0;
      let hi = lineStarts.length - 1;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (lineStarts[mid] <= o) lo = mid;
        else hi = mid - 1;
      }
      return new vscodeMock.Position(lo, o - lineStarts[lo]);
    },
  };
  return doc as unknown as import("vscode").TextDocument;
}

test("positionToByteOffset: ASCII string — UTF-8 and UTF-16 offsets coincide", () => {
  const doc = fakeDoc("hello world");
  const pos = new vscodeMock.Position(0, 6); // before 'w'
  assert.equal(textOffset.positionToByteOffset(doc, pos), 6);
});

test("positionToByteOffset: em-dash before the cursor adds 2 UTF-8 bytes", () => {
  // U+2014 EM DASH: 1 UTF-16 code unit, 3 UTF-8 bytes.
  const doc = fakeDoc("a—b");
  // UTF-16 offset 2 points at 'b'. UTF-8 bytes consumed: 'a'(1) + '—'(3) = 4.
  const pos = new vscodeMock.Position(0, 2);
  assert.equal(textOffset.positionToByteOffset(doc, pos), 4);
});

test("positionToByteOffset: emoji surrogate pair advances UTF-16 by 2 but UTF-8 by 4", () => {
  // U+1F389 PARTY POPPER: surrogate pair (2 UTF-16 units), 4 UTF-8 bytes.
  const doc = fakeDoc("x🎉y");
  // UTF-16 offset 3 points at 'y'. Bytes: 'x'(1) + 🎉(4) = 5.
  const pos = new vscodeMock.Position(0, 3);
  assert.equal(textOffset.positionToByteOffset(doc, pos), 5);
});

test("positionToByteOffset: CJK characters each cost 3 UTF-8 bytes", () => {
  // 日本語 — three U+xxxx codepoints in the BMP, 3 UTF-16 units, 9 UTF-8 bytes.
  const doc = fakeDoc("a日本語b");
  const pos = new vscodeMock.Position(0, 4); // before 'b'
  assert.equal(textOffset.positionToByteOffset(doc, pos), 10); // 'a'(1) + 日(3) + 本(3) + 語(3)
});

test("byteOffsetToPosition: ASCII string — UTF-8 byte offset maps to identical UTF-16 column", () => {
  const doc = fakeDoc("hello world");
  const p = textOffset.byteOffsetToPosition(doc, 6);
  assert.equal(p.line, 0);
  assert.equal(p.character, 6);
});

test("byteOffsetToPosition: em-dash exposes the UTF-16 / UTF-8 drift", () => {
  // Source: "a—b". Engine emits byte offset 4 for the 'b'. With the
  // old bug (UTF-16 == UTF-8), positionAt(4) lands at the end of the
  // string; the helper must return UTF-16 character 2 instead.
  const doc = fakeDoc("a—b");
  const p = textOffset.byteOffsetToPosition(doc, 4);
  assert.equal(p.line, 0);
  assert.equal(p.character, 2);
});

test("byteOffsetToPosition: emoji surrogate pair maps to 2 UTF-16 units", () => {
  const doc = fakeDoc("x🎉y");
  // Engine byte offset 5 = after 🎉 = before 'y'. UTF-16 character 3.
  const p = textOffset.byteOffsetToPosition(doc, 5);
  assert.equal(p.line, 0);
  assert.equal(p.character, 3);
});

test("byteOffsetToPosition: CJK round-trips with positionToByteOffset", () => {
  const doc = fakeDoc("a日本語b");
  for (const byteOffset of [0, 1, 4, 7, 10, 11]) {
    const p = textOffset.byteOffsetToPosition(doc, byteOffset);
    assert.equal(
      textOffset.positionToByteOffset(doc, p),
      byteOffset,
      `round-trip should be lossless for byte offset ${byteOffset}`,
    );
  }
});

test("byteOffsetToPosition: byte offset 0 returns the start", () => {
  const doc = fakeDoc("xyz");
  const p = textOffset.byteOffsetToPosition(doc, 0);
  assert.equal(p.line, 0);
  assert.equal(p.character, 0);
});

test("byteOffsetToPosition: byte offset past the end clamps to the end", () => {
  const doc = fakeDoc("xyz");
  const p = textOffset.byteOffsetToPosition(doc, 999);
  assert.equal(p.line, 0);
  assert.equal(p.character, 3);
});

test("byteOffsetToPosition: an offset that would split a codepoint stops just before it", () => {
  // 'é' = U+00E9 = 2 UTF-8 bytes. A byte offset of 1 lands inside it.
  // The helper must stop before the codepoint (position 0), not split it.
  const doc = fakeDoc("éx");
  const p = textOffset.byteOffsetToPosition(doc, 1);
  assert.equal(p.character, 0);
});

test("byteRangeToRange: spans a multi-byte region correctly", () => {
  const doc = fakeDoc("a—b—c");
  // Bytes: a(0..1), —(1..4), b(4..5), —(5..8), c(8..9).
  // A range over the second em-dash + 'c' is bytes [5, 9).
  const r = textOffset.byteRangeToRange(doc, 5, 9);
  // UTF-16: a(0), —(1), b(2), —(3), c(4). Range covers [3, 5).
  assert.equal(r.start.character, 3);
  assert.equal(r.end.character, 5);
});
