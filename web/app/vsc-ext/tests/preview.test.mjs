import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { runInNewContext } from "node:vm";
import { createRequire } from "node:module";
let mockedModules = 0;
const bundle = await build({
  entryPoints: ["vscode/src/extension.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  write: false,
  external: ["vscode"],
  plugins: [
    {
      name: "services-fixture",
      setup(build) {
        build.onLoad({ filter: /shared[\\/]service\.ts$/ }, () => {
          mockedModules++;
          return {
            contents: "export const Services = globalThis.Services;",
            loader: "ts",
          };
        });
      },
    },
  ],
});
assert.equal(
  mockedModules,
  1,
  "Host dependencies must be replaced before evaluating the test bundle",
);
test("side preview publishes unsaved edits, changes theme, deduplicates panels and restores after reload", async () => {
  const commands = new Map(),
    messages = [],
    errors = [];
  const disposable = { dispose() {} };
  let receive,
    changed,
    themeChanged,
    close,
    serializer,
    created = 0,
    revealed = 0,
    lookups = 0;
  const uri = (value) => ({
    scheme: "file",
    fsPath: value.replace(/^file:\/\//, ""),
    path: value,
    toString: () => (value.startsWith("file:") ? value : `file://${value}`),
  });
  const document = {
    uri: uri("/tmp/source.md"),
    version: 1,
    languageId: "markdown",
    getText: () => document.source ?? `# version ${document.version}`,
    offsetAt: (position) => position.character,
  };
  const panel = {
    title: "",
    webview: {
      cspSource: "'self'",
      asWebviewUri: (value) => value,
      onDidReceiveMessage: (fn) => {
        receive = fn;
        return disposable;
      },
      postMessage: async (message) => messages.push(message),
    },
    reveal: () => revealed++,
    onDidChangeViewState: () => disposable,
    onDidDispose: (fn) => {
      close = fn;
      return disposable;
    },
    dispose: () => close(),
  };
  const vscode = {
    Uri: {
      file: uri,
      joinPath: (base, file) => uri(`${base.fsPath}/${file}`),
      parse: uri,
    },
    ViewColumn: { Beside: 2 },
    ColorThemeKind: { Dark: 2, HighContrast: 3 },
    workspace: {
      getConfiguration: () => ({
        get: (_key, fallback) => fallback,
        inspect: () => undefined,
      }),
      onDidChangeTextDocument: (fn) => {
        changed = fn;
        return disposable;
      },
      onDidChangeConfiguration: () => disposable,
      openTextDocument: async () => document,
    },
    window: {
      visibleTextEditors: [],
      onDidChangeTextEditorVisibleRanges: () => disposable,
      onDidChangeTextEditorSelection: () => disposable,
      activeTextEditor: { document, selection: { active: { character: 0 } } },
      activeColorTheme: { kind: 1 },
      createWebviewPanel: (_type, _title, column) => {
        assert.equal(column, 2);
        created++;
        return panel;
      },
      showErrorMessage: (message) => errors.push(message),
      onDidChangeActiveColorTheme: (fn) => {
        themeChanged = fn;
        return disposable;
      },
      registerWebviewPanelSerializer: (_type, value) => {
        serializer = value;
        return disposable;
      },
    },
    commands: {
      registerCommand: (id, action) => {
        commands.set(id, action);
        return disposable;
      },
    },
  };
  const module = { exports: {} };
  runInNewContext(bundle.outputFiles[0].text, {
    module,
    exports: module.exports,
    require: (id) =>
      id === "vscode" ? vscode : createRequire(import.meta.url)(id),
    URL,
    Buffer,
    AbortController,
    Services: class {
      readingTemplate(uri) {
        assert.equal(uri, document.uri);
        return { id: "default", name: "Default" };
      }
      async previewStylesheet() {
        return undefined;
      }
      changed = { event: () => disposable };
      directories() {
        return [];
      }
      async catalog() {
        lookups++;
        return { definitions: [] };
      }
      fonts = { refresh: async () => {}, paths: () => ["/tmp/font.ttf"] };
    },
  });
  module.exports.activate({
    subscriptions: [],
    extensionUri: uri("/extension"),
  });
  assert.ok(
    commands.has("markview.fonts"),
    "Font management remains registered",
  );
  assert.ok(commands.has("markview.export.pdf"), "Export remains registered");
  await commands.get("markview.preview.open")();
  assert.match(panel.webview.html, /wasm-unsafe-eval/);
  await receive({ type: "ready" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(messages.at(-1).markdown, "# version 1");
  document.version = 2;
  changed({ document, contentChanges: [] });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(messages.at(-1).markdown, "# version 2");
  assert.equal(lookups, 1);
  document.source = "# version 2X";
  document.version = 3;
  vscode.window.activeTextEditor.selection.active.character =
    document.source.length;
  changed({
    document,
    contentChanges: [
      { rangeOffset: document.source.length - 1, rangeLength: 0, text: "X" },
    ],
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(messages.at(-1).edit.offset, document.source.length);
  assert.equal(messages.at(-1).edit.version, 3);
  await receive({ type: "rendered", uri: document.uri.toString(), version: 3 });

  vscode.window.activeColorTheme.kind = 2;
  themeChanged();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(messages.at(-1).theme, "dark");
  await commands.get("markview.preview.open")();
  assert.equal(created, 1);
  assert.equal(revealed, 1);
  panel.dispose();
  await serializer.deserializeWebviewPanel(panel, {
    uri: "file:///tmp/source.md",
    base: "/tmp",
    offset: 42,
  });
  await receive({ type: "ready" });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(messages.at(-1).offset, 42);
  assert.equal(errors.length, 0);
  await serializer.deserializeWebviewPanel(panel, {});
  assert.equal(errors.length, 1);
});
