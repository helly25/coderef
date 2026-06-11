// Translate between VSCode UTF-16 positions/offsets and the engine's
// UTF-8 byte offsets.
//
// Why this exists: `coderef-core` emits `byte_start` / `byte_end` as
// UTF-8 byte counts (the scanner runs against `&str`, which is UTF-8).
// VSCode strings are UTF-16, so `document.offsetAt()` and
// `document.positionAt()` work in UTF-16 code units. For ASCII the
// two coincide; the moment a file contains an em-dash, accented
// letter, CJK character, or emoji, the offsets drift apart and
// DocumentLinks/Hovers end up off by N (where N is the count of
// multi-byte characters before the reference).
//
// Both directions of conversion live here, pure-function over a
// `TextDocument`-like object that exposes `getText`, `offsetAt`,
// `positionAt`. That keeps the helpers testable under plain Node
// (no `@vscode/test-electron` required) and lets the providers stay
// thin.

import * as vscode from "vscode";

type DocumentLike = Pick<vscode.TextDocument, "getText" | "offsetAt" | "positionAt">;

/** UTF-16 position → UTF-8 byte offset.
 *
 *  Uses `Buffer.byteLength` for the UTF-8 byte count; that handles
 *  surrogate pairs and every BMP codepoint correctly without us
 *  walking the string ourselves. */
export function positionToByteOffset(
  document: DocumentLike,
  position: vscode.Position,
): number {
  const utf16Offset = document.offsetAt(position);
  const text = document.getText();
  return Buffer.byteLength(text.slice(0, utf16Offset), "utf8");
}

/** UTF-8 byte offset → UTF-16 `vscode.Position`.
 *
 *  Walks the string codepoint-by-codepoint, summing UTF-8 byte
 *  widths until the requested byte count is reached. The engine
 *  always emits byte offsets on character boundaries; if a caller
 *  passes a byte offset that would fall *inside* a codepoint we stop
 *  just before that codepoint (so the resulting position never
 *  splits a character).
 *
 *  UTF-8 byte widths per codepoint:
 *  - U+0000 .. U+007F   → 1 byte  (ASCII)
 *  - U+0080 .. U+07FF   → 2 bytes
 *  - U+0800 .. U+FFFF   → 3 bytes (em-dash, CJK BMP)
 *  - U+10000 .. U+10FFFF → 4 bytes (emoji, supplementary planes)
 *
 *  UTF-16 code-unit widths per codepoint:
 *  - BMP (≤ U+FFFF)     → 1 code unit
 *  - supplementary      → 2 code units (surrogate pair) */
export function byteOffsetToPosition(
  document: DocumentLike,
  byteOffset: number,
): vscode.Position {
  if (byteOffset <= 0) return document.positionAt(0);

  const text = document.getText();
  let bytesSeen = 0;
  let utf16Idx = 0;

  while (utf16Idx < text.length && bytesSeen < byteOffset) {
    const cp = text.codePointAt(utf16Idx);
    if (cp === undefined) break;
    const utf8Width = cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    // Don't split a codepoint: if consuming this character would
    // overshoot `byteOffset`, stop here.
    if (bytesSeen + utf8Width > byteOffset) break;
    bytesSeen += utf8Width;
    utf16Idx += cp >= 0x10000 ? 2 : 1;
  }

  return document.positionAt(utf16Idx);
}

/** Convenience: a `vscode.Range` spanning a `[byte_start, byte_end)`
 *  pair from the engine. */
export function byteRangeToRange(
  document: DocumentLike,
  byteStart: number,
  byteEnd: number,
): vscode.Range {
  return new vscode.Range(
    byteOffsetToPosition(document, byteStart),
    byteOffsetToPosition(document, byteEnd),
  );
}
