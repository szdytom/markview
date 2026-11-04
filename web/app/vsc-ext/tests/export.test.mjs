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
      name: "host-fixture",
      setup(build) {
        build.onLoad({ filter: /shared[\\/]service\.ts$/ }, () => {
          mockedModules++;
          return {
            contents: "export const Services = globalThis.Services;",
            loader: "ts",
          };
        });
        build.onLoad({ filter: /shared[\\/]export-snapshot\.ts$/ }, () => {
          mockedModules++;
          return {
            contents: "export const exportSnapshot = globalThis.snapshot;",
            loader: "ts",
          };
        });
      },
    },
  ],
});
assert.equal(
  mockedModules,
  2,
  "Host dependencies must be replaced before evaluating the test bundle",
);
test("export commands snapshot unsaved text, select templates, reveal success and handle cancellation/errors", async () => {
  const commands = new Map(),
    calls = [],
    messages = [],
    reveals = [];
  const disposable = { dispose() {} };
  let text = "# unsaved",
    cancelSave = false,
    fail = false;
  const uri = (file) => ({ scheme: "file", fsPath: file });
  const document = {
    uri: uri("/tmp/example.md"),
    languageId: "markdown",
    isUntitled: false,
    getText: () => text,
  };
  const config = { get: (_key, fallback) => fallback };
  const vscode = {
    Uri: { file: uri },
    ProgressLocation: { Notification: 1 },
    workspace: { getConfiguration: () => config, isTrusted: true },
    window: {
      visibleTextEditors: [],
      onDidChangeTextEditorVisibleRanges: () => disposable,
      onDidChangeTextEditorSelection: () => disposable,
      registerWebviewPanelSerializer: () => disposable,
      activeTextEditor: { document },
      showQuickPick: async () => "PNG",
      showSaveDialog: async () => {
        text = "# newer edits";
        return cancelSave ? undefined : uri("/tmp/result.pdf");
      },
      showWarningMessage: async (message) => messages.push(message),
      showErrorMessage: async (message) => messages.push(message),
      showInformationMessage: async (message) => messages.push(message),
      withProgress: async (_options, action) =>
        action({ report() {} }, { onCancellationRequested: () => disposable }),
    },
    commands: {
      registerCommand: (id, action) => {
        commands.set(id, action);
        return disposable;
      },
      executeCommand: async (...args) =>
        commands.has(args[0])
          ? commands.get(args[0])(...args.slice(1))
          : reveals.push(args),
    },
  };
  const module = { exports: {} };
  runInNewContext(bundle.outputFiles[0].text, {
    module,
    exports: module.exports,
    require: (id) =>
      id === "vscode" ? vscode : createRequire(import.meta.url)(id),
    AbortController,
    Services: class {
      async template(_destination, uri) {
        assert.equal(uri, document.uri);
        return { id: "celadon", name: "Celadon" };
      }
      async pickTemplate() {
        return { id: "van-gogh", name: "Van Gogh" };
      }
      async withTemplate(template, action) {
        return action(template);
      }
    },
    snapshot: async (...args) => {
      if (fail) throw new Error("native failure");
      calls.push(args);
    },
  });
  module.exports.activate({ subscriptions: [] });
  assert.ok(
    commands.has("markview.fonts"),
    "Font management remains registered",
  );
  assert.ok(
    commands.has("markview.preview.open"),
    "Preview remains registered",
  );
  await commands.get("markview.export.pdf")();
  assert.equal(calls[0][1], "# unsaved");
  assert.equal(calls[0][3], "pdf");
  assert.equal(calls[0][5].id, "celadon");
  assert.equal(reveals[0][0], "revealFileInOS");
  document.isUntitled = true;
  document.uri = { scheme: "untitled", fsPath: "Untitled-1" };
  vscode.workspace.workspaceFolders = [{ uri: uri("/tmp") }];
  await commands.get("markview.export.template")();
  assert.equal(calls[1][3], "png");
  assert.equal(calls[1][5].id, "van-gogh");
  cancelSave = true;
  await commands.get("markview.export.pdf")();
  assert.equal(calls.length, 2);
  cancelSave = false;
  fail = true;
  await commands.get("markview.export.pdf")();
  assert.match(messages.at(-1), /native failure/);
  assert.equal(reveals.length, 2);
  fail = false;
  document.isUntitled = false;
  document.uri = uri("/tmp/right-click.md");
  vscode.workspace.openTextDocument = async (source) => {
    assert.equal(source, document.uri);
    return document;
  };
  vscode.window.activeTextEditor = undefined;
  for (const prefix of ["markviewExport", "markview"])
    for (const name of ["exportPdf", "exportPng", "exportWithTemplate"])
      await commands.get(`${prefix}.${name}`)(document.uri);
  assert.equal(
    calls.length,
    8,
    "legacy aliases preserve the context-menu source URI",
  );
});
