// @vscode/test-cli config — runtime tests for the coderef extension.
//
// Boots a real VSCode instance with the extension under test, opens
// the fixture workspace, and runs the compiled mocha tests in the
// Extension Host. Closes the open `docs/test-plan.md` ledger entry
// for "VSCode extension end-to-end (DocumentLink + Hover under
// Extension Host)".

import { defineConfig } from "@vscode/test-cli";

export default defineConfig({
  files: "out/test/runtime/*.test.js",
  workspaceFolder: "src/test/runtime/fixtures/workspace",
  mocha: {
    ui: "tdd",
    timeout: 30_000, // VSCode binary boot is slow on cold CI runners
  },
});
