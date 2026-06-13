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

// TreeItem and ThemeIcon stubs for the references-view unit tests.
class TreeItem {
  description?: string;
  tooltip?: string;
  iconPath?: unknown;
  resourceUri?: Uri;
  contextValue?: string;
  command?: unknown;
  constructor(
    public label: string,
    public collapsibleState?: number,
  ) {}
}

enum TreeItemCollapsibleState {
  None = 0,
  Collapsed = 1,
  Expanded = 2,
}

class ThemeIcon {
  static readonly File = new ThemeIcon("file");
  static readonly Folder = new ThemeIcon("folder");
  constructor(public id: string) {}
}

class EventEmitter<T> {
  private listeners: ((e: T) => void)[] = [];
  event = (listener: (e: T) => void): { dispose(): void } => {
    this.listeners.push(listener);
    return { dispose: () => {} };
  };
  fire(e: T): void {
    for (const l of this.listeners) l(e);
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
  TreeItem,
  TreeItemCollapsibleState,
  ThemeIcon,
  EventEmitter,
  workspace,
  languages: {},
};
