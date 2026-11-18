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
    await fs.writeFile(custom, 'format_version=3\nversion=1\ntargets=["ui"]');
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
    const readerTemplates = [
      { id: "celadon", name: "Celadon" },
      { id: "qibaishi", name: "Qi Baishi" },
      { id: "mondrian", name: "Mondrian" },
      { id: "vangogh", name: "Van Gogh" },
      { id: "future-theme", name: "Future Theme" },
      { id: "installed-theme", name: "Installed Theme", path: custom },
    ];
    const exportTemplates = [
      { id: "print", name: "Print" },
      { id: "vangogh", name: "Van Gogh" },
    ];
    services.catalog = async (_template, destination) => {
      target = destination;
      return {
        templates: destination === "ui" ? readerTemplates : exportTemplates,
      };
    };
    assert.equal((await services.readingTemplate()).id, "celadon");
    assert.equal((await services.pickTemplate("ui")).id, "celadon");
    assert.equal(target, "ui");
    assert.deepEqual(
      Array.from(items.slice(1, -1), (item) => ({
        id: item.template.id,
        name: item.label,
        ...(item.template.path ? { path: item.template.path } : {}),
      })),
      readerTemplates,
    );
    assert.equal(items[0].template.id, "default");
    for (const template of readerTemplates) {
      choice = template.id;
      const selected = await services.pickTemplate("ui");
      assert.equal(selected.id, template.id);
      assert.equal(selected.path, template.path);
      if (selected.path)
        assert.match(
          await services.previewStylesheet(selected),
          /targets=\["ui"\]/,
        );
    }
    assert.match(
      await services.previewStylesheet(
        { id: "installed-theme", name: "Installed Theme" },
        readerTemplates,
      ),
      /targets=\["ui"\]/,
    );
    const installedDefault = { id: "default", name: "Installed", path: custom };
    assert.equal(
      await services.previewStylesheet({ id: "default", name: "Default" }, [
        installedDefault,
      ]),
      undefined,
    );
    assert.match(
      await services.previewStylesheet(installedDefault),
      /targets=\["ui"\]/,
    );
    choice = "custom";
    assert.equal((await services.pickTemplate("ui")).path, custom);
    config.set("previewStyleFile", custom);
    assert.equal((await services.readingTemplate()).path, custom);
    assert.match(
      await services.previewStylesheet(await services.readingTemplate()),
      /targets=\["ui"\]/,
    );
    choice = "print";
    assert.equal((await services.pickTemplate()).id, "print");
    assert.equal(target, "pdf");
    assert.deepEqual(
      Array.from(items.slice(1, -1), (item) => ({
        id: item.template.id,
        name: item.label,
      })),
      exportTemplates,
    );
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test("reading template settings accept IDs from the engine catalog", async () => {
  const manifest = JSON.parse(
    await fs.readFile(
      new URL("../vscode/package.json", import.meta.url),
      "utf8",
    ),
  );
  const setting =
    manifest.contributes.configuration.properties["markview.previewTemplate"];
  assert.equal(setting.type, "string");
  assert.equal(setting.default, "default");
  assert.equal(setting.enum, undefined);
});
