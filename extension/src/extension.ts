// VSCode extension entry point. v0.0.0 is a no-op activate/deactivate so the
// extension package is buildable and packageable; providers (DocumentLink,
// Hover, CodeActions, References Browser) land per the v0.1 roadmap in
// DESIGN.md §14 and §19.

import * as vscode from "vscode";

export function activate(_context: vscode.ExtensionContext): void {
  console.log("coderef: v0.0.0 scaffold — no providers registered yet");
}

export function deactivate(): void {
  // Nothing to clean up at v0.0.0.
}
