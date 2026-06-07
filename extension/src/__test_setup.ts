// Shared setup for the .test.ts files. Intercepts `require('vscode')`
// at the node:module layer and re-routes it to `__vscode_mock` so the
// production source under test can `import * as vscode from 'vscode'`
// without an Extension Host runtime.
//
// Idempotent: safe to call from every test file's top level.

declare const globalThis: { __coderefMockVscodeRegistered?: boolean };

export function registerVscodeMock(): void {
  if (globalThis.__coderefMockVscodeRegistered) return;
  globalThis.__coderefMockVscodeRegistered = true;
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const Module = require("node:module");
  const orig = Module._resolveFilename;
  Module._resolveFilename = function (request: string, ...rest: unknown[]): string {
    if (request === "vscode") return require.resolve("./__vscode_mock");
    return orig.call(this, request, ...rest);
  };
}
