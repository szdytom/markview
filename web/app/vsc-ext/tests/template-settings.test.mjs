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
const settings = (directory) => path.join(directory, ".vscode/settings.json");
const uri = (file) => ({ scheme: "file", fsPath: file });

async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "markview-inherit-"));
  const watchers = new Map(),
    reads = [],
    denied = new Map(),
    config = new Map();
  denied.set(settings(path.dirname(root)), "EACCES");
  const listeners = new Set();
  const vscode = {
    RelativePattern: class {
      constructor(base, pattern) {
        assert.equal(
          path.basename(pattern),
          pattern,
          "Watchers must not recurse through ancestor directories",
        );
        assert.ok(!pattern.includes("**"));
        this.file = path.join(base, pattern);
      }
    },
    EventEmitter: class {
      event = (listener) => {
        listeners.add(listener);
        return { dispose: () => listeners.delete(listener) };
      };
      fire() {
        listeners.forEach((listener) => listener());
      }
      dispose() {
        listeners.clear();
      }
    },
    workspace: {
      getConfiguration: () => ({
        get: (key, fallback) => config.get(key) ?? fallback,
      }),
      createFileSystemWatcher: ({ file }) => {
        const events = {};
        const watcher = {
          events,
          onDidChange: (fn) => {
            events.change = fn;
          },
          onDidCreate: (fn) => {
            events.create = fn;
          },
          onDidDelete: (fn) => {
            events.delete = fn;
          },
          dispose: () => watchers.delete(file),
        };
        watchers.set(file, watcher);
        return watcher;
      },
    },
  };
  const require = createRequire(import.meta.url);
  const module = { exports: {} };
  runInNewContext(bundle.outputFiles[0].text, {
    module,
    exports: module.exports,
    process,
    Buffer,
    AbortController,
    require: (id) =>
      id === "vscode"
        ? vscode
        : id === "node:fs/promises"
          ? {
              readFile: async (file, ...args) => {
                reads.push(file);
                if (denied.has(file))
                  throw Object.assign(new Error("Denied"), {
                    code: denied.get(file),
                  });
                return fs.readFile(file, ...args);
              },
            }
          : require(id),
  });
  const services = new module.exports.Services({
    asAbsolutePath: (file) => path.join(root, file),
    globalStorageUri: { fsPath: root },
  });
  t.after(async () => {
    services.dispose();
    assert.equal(watchers.size, 0);
    await fs.rm(root, { recursive: true, force: true });
  });
  async function write(directory, values) {
    const file = settings(directory);
    await fs.mkdir(path.dirname(file), { recursive: true });
    await fs.writeFile(
      file,
      typeof values === "string" ? values : JSON.stringify(values),
    );
    return file;
  }
  return { root, services, config, reads, denied, watchers, write };
}

test("template settings inherit outside the opened folder, with atomic overrides and independent destinations", async (t) => {
  const { root, services, watchers, write } = await fixture(t);
  const project = path.join(root, "project"),
    chapter = path.join(project, "chapter");
  await fs.mkdir(chapter, { recursive: true });
  const document = uri(path.join(chapter, "draft.md"));
  const parent = await write(
    root,
    `{
    // Shared across projects.
    "markview.previewStyleFile": "styles/reader.mvss.toml",
    "markview.exportTemplate": "vangogh",
  }`,
  );
  await write(project, { "markview.fontSize": 24 });
  const inherited = await services.readingTemplate(document);
  assert.equal(inherited.path, path.join(root, "styles/reader.mvss.toml"));
  assert.equal(inherited.source, parent);
  assert.equal((await services.template("pdf", document)).id, "vangogh");
  const child = await write(chapter, {
    "markview.previewTemplate": "blueprint",
  });
  watchers.get(child).events.create();
  const overridden = await services.readingTemplate(document);
  assert.equal(overridden.id, "blueprint");
  assert.equal(overridden.path, undefined);
  assert.equal((await services.template("pdf", document)).id, "vangogh");
  const sibling = path.join(root, "other-project");
  await fs.mkdir(sibling);
  const selections = await Promise.all([
    services.readingTemplate(document),
    services.readingTemplate(uri(path.join(sibling, "other.md"))),
  ]);
  assert.equal(selections[0].id, "blueprint");
  assert.equal(selections[1].path, inherited.path);
});

test("cached settings and selected stylesheets refresh on create, change and delete", async (t) => {
  const { root, services, reads, watchers, write } = await fixture(t);
  const chapter = path.join(root, "chapter");
  await fs.mkdir(chapter);
  const document = uri(path.join(chapter, "draft.md"));
  await write(root, { "markview.previewStyleFile": "reader.mvss.toml" });
  const style = path.join(root, "reader.mvss.toml");
  await fs.writeFile(style, "version=1");
  assert.equal((await services.readingTemplate(document)).path, style);
  const count = reads.length;
  await services.readingTemplate(document);
  assert.equal(
    reads.length,
    count,
    "Editing should reuse cached configuration",
  );
  let updates = 0;
  services.changed.event(() => updates++);
  const child = await write(chapter, {
    "markview.previewTemplate": "blueprint",
  });
  watchers.get(child).events.create();
  const selected = await services.readingTemplate(document);
  assert.equal(selected.id, "blueprint");
  assert.equal(
    selected.path,
    undefined,
    "A child preset replaces the parent's custom file",
  );
  await write(chapter, { "markview.previewTemplate": "default" });
  watchers.get(child).events.change();
  assert.equal((await services.readingTemplate(document)).id, "default");
  await fs.unlink(child);
  watchers.get(child).events.delete();
  const inherited = await services.readingTemplate(document);
  assert.equal(inherited.path, style);
  watchers.get(style).events.change();
  assert.equal(updates, 4);
  assert.equal(await services.previewStylesheet(inherited), "version=1");
  const folder = path.dirname(child);
  await fs.rm(folder, { recursive: true });
  watchers.get(folder).events.delete();
  assert.equal((await services.readingTemplate(document)).path, style);
  await write(chapter, { "markview.previewTemplate": "celadon" });
  watchers.get(folder).events.create();
  assert.equal((await services.readingTemplate(document)).id, "celadon");
  assert.equal(updates, 6);
});

test("permission errors stop traversal, while invalid settings and missing selected templates report errors", async (t) => {
  const { root, services, reads, denied, watchers, write, config } =
    await fixture(t);
  config.set("previewTemplate", "rosewood");
  for (const code of ["EACCES", "EPERM"]) {
    denied.set(settings(root), code);
    assert.equal(
      (await services.readingTemplate(uri(path.join(root, "draft.md")))).id,
      "rosewood",
    );
    assert.equal(reads.at(-1), settings(root));
    denied.delete(settings(root));
    watchers.get(settings(root)).events.change();
  }
  await write(root, '{ "markview.previewTemplate": ');
  await assert.rejects(
    services.readingTemplate(uri(path.join(root, "draft.md"))),
    /Invalid Markview settings/,
  );
  await write(root, { "markview.previewTemplate": 123 });
  watchers.get(settings(root)).events.change();
  await assert.rejects(
    services.readingTemplate(uri(path.join(root, "draft.md"))),
    /Expected a string/,
  );
  await write(root, { "markview.previewStyleFile": "missing.mvss.toml" });
  watchers.get(settings(root)).events.change();
  const selected = await services.readingTemplate(
    uri(path.join(root, "draft.md")),
  );
  await assert.rejects(services.previewStylesheet(selected), {
    code: "ENOENT",
  });
});

test("untitled documents use VS Code settings and upward lookup terminates at the filesystem root", async (t) => {
  const { services, config, reads, denied, watchers } = await fixture(t);
  config.set("previewTemplate", "celadon");
  assert.equal(
    (await services.readingTemplate({ scheme: "untitled" })).id,
    "celadon",
  );
  assert.equal(reads.length, 0);
  const root = path.parse(process.cwd()).root;
  denied.set(settings(root), "ENOENT");
  assert.equal(
    (await services.readingTemplate(uri(path.join(root, "draft.md")))).id,
    "celadon",
  );
  assert.deepEqual(reads, [settings(root)]);
});

test("legacy templates retain document-relative paths and explicit modern choices win", async (t) => {
  const { root, services, config, write, watchers } = await fixture(t);
  const document = uri(path.join(root, "notes", "draft.md"));
  config.set("template", "styles/paper.mvss.toml");
  assert.equal(
    (await services.template("pdf", document)).path,
    path.join(root, "notes", "styles/paper.mvss.toml"),
  );
  config.set("exportTemplate", "default");
  assert.equal((await services.template("pdf", document)).id, "default");
  assert.equal((await services.readingTemplate(document)).id, "custom");
  const modern = await write(root, { "markview.previewTemplate": "celadon" });
  watchers.get(modern).events.change();
  assert.equal((await services.readingTemplate(document)).id, "celadon");
  config.delete("exportTemplate");
  config.set("template", "");
  assert.equal((await services.template("pdf", document)).id, "default");
});
