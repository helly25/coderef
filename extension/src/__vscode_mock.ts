// Mock of the `vscode` module used only by unit tests in this folder.
// Provides just enough surface for the pure-function tests in
// providers.test.ts to run under plain Node.
//
// VSCode integration tests (running the extension under
// @vscode/test-electron in an actual editor host) are tracked in
// docs/test-plan.md.

class Position {
  constructor(public line: number, public character: number) {}
}

class Range {
  constructor(public start: Position, public end: Position) {}
}

class Uri {
  constructor(
    public scheme: string,
    public fsPath: string,
    private readonly _full: string,
  ) {}
  static parse(s: string): Uri {
    const m = /^([a-z][a-z0-9+.-]*):/i.exec(s);
    const scheme = m && m[1] ? m[1] : "file";
    // Strip `scheme://` (or `scheme:`) for fsPath; keep the original
    // for toString so callers see the canonical form back.
    const fsPath = s.replace(/^[a-z][a-z0-9+.-]*:\/?\/?/i, "");
    return new Uri(scheme, fsPath, s);
  }
  static file(p: string): Uri {
    return new Uri("file", p, `file://${p}`);
  }
  toString(): string {
    return this._full;
  }
}

class DocumentLink {
  constructor(public range: Range, public target?: Uri) {}
}

class Hover {
  constructor(public contents: unknown, public range?: Range) {}
}

class MarkdownString {
  value = "";
  appendMarkdown(s: string): this {
    this.value += s;
    return this;
  }
}

const workspace = {
  getWorkspaceFolder(_: unknown): { uri: Uri } | undefined {
    return undefined;
  },
  asRelativePath(_: unknown, _includeFolder?: boolean): string {
    return "";
  },
};

module.exports = {
  Position,
  Range,
  Uri,
  DocumentLink,
  Hover,
  MarkdownString,
  workspace,
  languages: {},
};
