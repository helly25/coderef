// Command handlers for the coderef extension.
//
// Currently:
//   coderef.explainReference   Right-click / command-palette: open a
//                              read-only markdown view explaining the
//                              reference under the cursor.
//
// The actual explanation logic lives in coderef-core (PR #17) and is
// reachable via the WASM binding `explain_text`. The command here is
// pure UI orchestration: find the ref, call the engine, format the
// result as markdown, open in a side editor pane.

import * as vscode from "vscode";

import { type LoadedConfig } from "./configLoader";
import { type ReferenceCache } from "./referenceCache";
import {
  type EngineReference,
  type ExplainReport,
  explainText,
} from "./wasmEngine";

/** `coderef.explainReference` — explain the reference at the active
 *  cursor position. If there isn't one under the cursor, show a
 *  friendly information message rather than failing silently. */
export async function explainReferenceCommand(
  cache: ReferenceCache,
  getConfig: () => LoadedConfig | undefined,
): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    void vscode.window.showInformationMessage(
      "coderef: no active editor — open a file first.",
    );
    return;
  }
  const document = editor.document;
  const position = editor.selection.active;
  const offset = document.offsetAt(position);

  const refs = cache.get(document);
  const ref = refs.find(
    (r) => offset >= r.byte_start && offset < r.byte_end,
  );

  const cfg = getConfig();
  if (!cfg) {
    void vscode.window.showWarningMessage(
      "coderef: no `.coderef.jsonc` loaded — `explain` needs a config to find patterns.",
    );
    return;
  }

  // If the cursor is on a known ref, explain its matched text.
  // Otherwise, prompt the user for an input to explain.
  let input: string | undefined;
  if (ref) {
    input = ref.matched_text;
  } else {
    input = await vscode.window.showInputBox({
      prompt: "coderef: text to explain (e.g. TODO(@alice) or DOCREF(/docs/x.md))",
      placeHolder: "TODO(@alice)",
    });
    if (!input) return; // user cancelled
  }

  let report: ExplainReport;
  try {
    report = explainText(cfg.config, input);
  } catch (err) {
    void vscode.window.showErrorMessage(
      `coderef explain failed: ${err instanceof Error ? err.message : String(err)}`,
    );
    return;
  }

  const markdown = renderExplainReportAsMarkdown(report, ref);
  const doc = await vscode.workspace.openTextDocument({
    language: "markdown",
    content: markdown,
  });
  await vscode.window.showTextDocument(doc, {
    viewColumn: vscode.ViewColumn.Beside,
    preview: true,
  });
}

/** Render an `ExplainReport` as a human-readable markdown document.
 *  Pure function — testable without the VSCode runtime. The `ref`
 *  argument adds editor-context info (file location) when the user
 *  triggered the command from a known ref. */
export function renderExplainReportAsMarkdown(
  report: ExplainReport,
  ref: EngineReference | undefined,
): string {
  const lines: string[] = [];
  lines.push(`# coderef explain`);
  lines.push("");
  lines.push(`**Input:** \`${escapeBackticks(report.input)}\``);
  if (ref) {
    lines.push("");
    lines.push(
      `_From_ \`${ref.file}\` _line_ ${ref.line}, _column_ ${ref.column}.`,
    );
  }
  lines.push("");
  if (report.matches.length === 0) {
    lines.push("## No matches");
    lines.push("");
    lines.push("No configured pattern matches this input.");
  } else {
    const n = report.matches.length;
    lines.push(`## ${n} match${n === 1 ? "" : "es"}`);
    lines.push("");
    for (const m of report.matches) {
      lines.push(`### \`${m.pattern_id}\` (${m.pattern_kind})`);
      lines.push("");
      if (m.description) {
        lines.push(m.description);
        lines.push("");
      }
      lines.push("```");
      lines.push(`matched:   ${m.matched_text}`);
      const caps = Object.entries(m.captures);
      if (caps.length > 0) {
        const fmt = caps.map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(", ");
        lines.push(`captures:  ${fmt}`);
      }
      lines.push(`target:    ${m.target}`);
      if (m.title) lines.push(`title:     ${m.title}`);
      if (m.priority !== 0) lines.push(`priority:  ${m.priority}`);
      lines.push("```");
      if (m.scope_notes.length > 0) {
        lines.push("");
        lines.push("**Scope filters that would apply at scan time:**");
        for (const note of m.scope_notes) {
          lines.push(`- ${note}`);
        }
      }
      if (m.resolution_warnings.length > 0) {
        lines.push("");
        lines.push("**Warnings:**");
        for (const w of m.resolution_warnings) {
          lines.push(`- ${w}`);
        }
      }
      lines.push("");
    }
  }
  if (report.non_matching_pattern_ids.length > 0) {
    lines.push("---");
    lines.push("");
    const n = report.non_matching_pattern_ids.length;
    lines.push(
      `_Did **not** match ${n} pattern${n === 1 ? "" : "s"}:_ ${report.non_matching_pattern_ids.map((p) => `\`${p}\``).join(", ")}`,
    );
    lines.push("");
  }
  return lines.join("\n");
}

function escapeBackticks(s: string): string {
  return s.replace(/`/g, "\\`");
}
