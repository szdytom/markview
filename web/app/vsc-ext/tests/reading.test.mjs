import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { runInNewContext } from "node:vm";
import { createRequire } from "node:module";
import { promises as fs } from "node:fs";
import path from "node:path";
import os from "node:os";
const bundle = await build({
  entryPoints: ["vscode/src/shared/service.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  write: false,
  external: ["vscode"],
  plugins: [
    {
      name: "font-index-fixture",
      setup(build) {
        build.onLoad({ filter: /shared[\\/]fonts\.ts$/ }, () => ({
          loader: "ts",
          contents: "export class FontIndex {}",
        }));
      },
    },
  ],
});

test("reading template selection and font catalog use the same UI template", async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "markview-reading-"));
  try {
    const custom = path.join(root, "custom.mvss.toml");
    await fs.writeFile(custom, 'format_version=2\nversion=1\ntargets=["ui"]');
    const config = new Map([
      ["previewTemplate", "celadon"],
      ["previewStyleFile", ""],
    ]);
    let items,
      choice = "celadon",
      target;
    const module = { exports: {} };
    const vscode = {
      EventEmitter: class {
        fire() {}
        dispose() {}
      },
      RelativePattern: class {
        constructor() {}
      },
      workspace: {
        createFileSystemWatcher: () => ({
          onDidChange() {},
          onDidCreate() {},
          onDidDelete() {},
          dispose() {},
        }),
        getConfiguration: () => ({
          get: (key, fallback) => config.get(key) ?? fallback,
          inspect: (key) => ({ workspaceFolderValue: config.get(key) }),
        }),
      },
      window: {
        showQuickPick: async (choices) => {
          items = choices;
          return choice === "custom"
            ? choices.at(-1)
            : choices.find((item) => item.template?.id === choice);
        },
        showOpenDialog: async () => [{ fsPath: custom }],
      },
    };
    runInNewContext(bundle.outputFiles[0].text, {
      module,
      exports: module.exports,
      require: (id) =>
        id === "vscode" ? vscode : createRequire(import.meta.url)(id),
      process,
      Buffer,
      AbortController,
    });
    const services = new module.exports.Services({
      asAbsolutePath: (file) => path.join(root, file),
      globalStorageUri: { fsPath: root },
    });
    services.catalog = async (_template, destination) => {
      target = destination;
      return {
        templates: [
          { id: "celadon", name: "Celadon" },
          { id: "vangogh", name: "PDF only" },
        ],
      };
    };
    assert.equal((await services.readingTemplate()).id, "celadon");
    assert.equal((await services.pickTemplate("ui")).id, "celadon");
    assert.equal(target, "ui");
    assert.equal(
      items.some((item) => item.template?.id === "vangogh"),
      false,
    );
    choice = "custom";
    assert.equal((await services.pickTemplate("ui")).path, custom);
    config.set("previewStyleFile", custom);
    assert.equal((await services.readingTemplate()).path, custom);
    assert.match(
      await services.previewStylesheet(await services.readingTemplate()),
      /targets=\["ui"\]/,
    );
    await services.pickTemplate();
    assert.equal(target, "pdf");
    assert.equal(
      items.some((item) => item.template?.id === "vangogh"),
      true,
    );
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});
