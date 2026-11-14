import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, readFile, writeFile, rm, readdir } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { FontClient } from "../dist/font-client.js";
import { FontIndex } from "../dist/fonts.js";

test("native catalog and downloads feed the editor cache and font status", async () => {
  assert.ok(
    process.env.MARKVIEW_BINARY,
    "Set MARKVIEW_BINARY to the font CLI build",
  );
  const dir = await mkdtemp(path.join(os.tmpdir(), "markview-native-fonts-"));
  const bytes = await readFile(
    new URL(
      "../../../../crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf",
      import.meta.url,
    ),
  );
  let valid = false;
  let cancelTransfer = false;
  const controller = new AbortController();
  const server = createServer((_request, response) => {
    response.writeHead(200, { "Content-Length": bytes.length });
    if (cancelTransfer && _request.method !== "HEAD") {
      response.write(bytes.subarray(0, 10));
      controller.abort();
    } else response.end(valid ? bytes : Buffer.alloc(bytes.length));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const file = path.join(dir, "custom.mvss.toml");
    await writeFile(
      file,
      `format_version=3
version=1
targets=['ui','pdf']
[[fontdef]]
id='fixture-face'
lookfor=['Noto Serif']
[[font-family]]
id='fixture-family'
lookfor=['Noto Serif']
license='OFL-1.1'
[[font-family.source]]
files=['http://127.0.0.1:${server.address().port}/fixture.otf']
`,
    );
    const template = { id: "custom", name: "Fixture", path: file };
    const cache = path.join(dir, "fonts");
    const client = new FontClient(process.env.MARKVIEW_BINARY, cache);
    const index = new FontIndex(cache, path.join(dir, "index.json"));
    const progress = [];
    await assert.rejects(
      client.downloadFamily(template, "ui", "fixture-family"),
    );
    assert.deepEqual(await readdir(cache), []);
    valid = true;
    await client.withTemplate(template, (snapshot) =>
      client.downloadFamily(
        snapshot,
        "ui",
        "fixture-family",
        undefined,
        (event) => progress.push(event),
      ),
    );
    const catalog = await client.catalog(template, "ui");
    await index.refresh([]);
    const status = index
      .statuses(catalog)
      .find((row) => row.definition.id === "fixture-face");
    assert.equal(status.source, "cache");
    assert.ok(status.match);
    assert.deepEqual(await readFile(status.match.path), bytes);
    assert.ok(progress.some((event) => event.type === "progress"));
    cancelTransfer = true;
    await assert.rejects(
      client.downloadFamily(
        template,
        "ui",
        "fixture-family",
        controller.signal,
      ),
      { name: "AbortError" },
    );
    assert.deepEqual(await readFile(status.match.path), bytes);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    await rm(dir, { recursive: true, force: true });
  }
});
