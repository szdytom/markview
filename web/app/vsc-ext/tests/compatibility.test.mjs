import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { runInNewContext } from "node:vm";
import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
const bundle = await build({
  entryPoints: ["vscode/src/shared/compatibility.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  write: false,
  external: ["vscode"],
});
const module = { exports: {} };
const require = createRequire(import.meta.url);
runInNewContext(bundle.outputFiles[0].text, {
  module,
  exports: module.exports,
  require: (id) =>
    id === "vscode"
      ? {
          workspace: {
            workspaceFolders: [{ uri: { fsPath: "/workspace" } }],
            getWorkspaceFolder: () => undefined,
          },
        }
      : require(id),
});
const { readingSetting, legacyTemplate } = module.exports;
test("legacy reading values never override an explicit modern value, including false", () => {
  const values = { columnWidth: 600, codeblockWrap: true };
  const config = {
    inspect: (key) => ({
      defaultValue: key === "width" ? 760 : false,
      workspaceFolderLanguageValue: values[key],
    }),
    get: (key, fallback) => values[key] ?? fallback,
  };
  assert.equal(readingSetting(config, "width", "columnWidth", 760), 600);
  values.width = 760;
  assert.equal(readingSetting(config, "width", "columnWidth", 760), 760);
  values.codeWrap = false;
  assert.equal(
    readingSetting(config, "codeWrap", "codeblockWrap", false),
    false,
  );
  assert.match(
    legacyTemplate("styles/paper.mvss.toml", { scheme: "untitled" }).path,
    /workspace.*styles.*paper.mvss.toml/,
  );
});
test("upgrade manifest retains Marketplace identity and hidden command aliases", async () => {
  const p = JSON.parse(await readFile("vscode/package.json", "utf8"));
  assert.equal(p.publisher, "Stevvven");
  assert.equal(p.name, "markview-export");
  assert.equal(p.displayName, "Markview Integration");
  for (const prefix of ["markviewExport", "markview"])
    for (const name of ["exportPdf", "exportPng", "exportWithTemplate"]) {
      const id = `${prefix}.${name}`;
      assert.ok(p.contributes.commands.some((c) => c.command === id));
      assert.ok(
        p.contributes.menus.commandPalette.some(
          (c) => c.command === id && c.when === "false",
        ),
      );
    }
});
