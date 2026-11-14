import { expect, test } from "@playwright/test";
import { build } from "esbuild";
import { runInNewContext } from "node:vm";
import { readFile } from "node:fs/promises";
import path from "node:path";
const root = process.cwd();
async function open(page, restoredOffset = 0) {
  await page.route("http://markview.test/**", async (route) => {
    const name = new URL(route.request().url()).pathname.slice(1);
    if (!name)
      return route.fulfill({
        contentType: "text/html",
        body: `<!doctype html><link rel="stylesheet" href="/preview.css"><span id="status"></span><main id="reader"></main><script>window.messages=[];window.markviewHost={postMessage:message=>{messages.push(message);window.scrollHost?.(message)},setState:state=>window.saved=state};</script><script type="module" src="/preview.js" data-wasm="/markview_web_bg.wasm"></script>`,
      });
    const file =
      name === "font.otf"
        ? path.resolve(
            root,
            "../../../crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf",
          )
        : path.resolve(root, "vscode/media", name);
    await route.fulfill({
      body: await readFile(file),
      contentType: name.endsWith(".js")
        ? "text/javascript"
        : name.endsWith(".wasm")
          ? "application/wasm"
          : name.endsWith(".css")
            ? "text/css"
            : "application/octet-stream",
    });
  });
  await page.goto("http://markview.test/");
  await expect
    .poll(() =>
      page.evaluate(() => messages.some((message) => message.type === "ready")),
    )
    .toBe(true);
  const message = {
    type: "document",
    uri: "file:///test.md",
    version: 1,
    markdown: "# Preview\n\n" + "A paragraph of readable text.\n\n".repeat(60),
    fonts: ["http://markview.test/font.otf"],
    theme: "light",
    base: "/tmp",
    offset: restoredOffset,
    maxLine: 100,
  };
  await publish(page, message);
  return message;
}
async function publish(page, message) {
  await page.evaluate(
    (data) => window.dispatchEvent(new MessageEvent("message", { data })),
    message,
  );
}
async function rendered(page, version) {
  await expect
    .poll(() =>
      page.evaluate(
        (version) =>
          messages.some(
            (message) =>
              message.type === "rendered" && message.version === version,
          ),
        version,
      ),
    )
    .toBe(true);
  expect(
    await page.evaluate(() =>
      messages.filter((message) => message.type === "error"),
    ),
  ).toEqual([]);
}
test("real MVaaC renders edits, keeps balanced margins and supports scrollbar/theme updates", async ({
  page,
}) => {
  const message = await open(page);
  await rendered(page, 1);
  await expect(page.locator("canvas")).toBeVisible();
  const padding = await page.locator("#reader").evaluate((element) => {
    const style = getComputedStyle(element);
    return [style.paddingLeft, style.paddingRight];
  });
  expect(padding[0]).toEqual(padding[1]);
  await page.locator("canvas").focus();
  expect(
    await page
      .locator("canvas")
      .evaluate((element) => getComputedStyle(element).outlineStyle),
  ).toBe("none");
  await page.getByRole("scrollbar").focus();
  await page.keyboard.press("End");
  await expect.poll(() => page.evaluate(() => saved.offset)).toBeGreaterThan(0);
  await publish(page, {
    ...message,
    version: 2,
    markdown: "# Updated\n\nUnsaved changes",
    theme: "dark",
  });
  await rendered(page, 2);
  await expect(page.getByRole("scrollbar")).toBeHidden();
  await page.screenshot({ path: "test-results/preview-dark.png" });
});
test("latest update wins during initialization and saved reading position restores", async ({
  page,
}) => {
  const message = await open(page, 700);
  await rendered(page, 1);
  await expect.poll(() => page.evaluate(() => saved.offset)).toBeGreaterThan(0);
  for (let version = 2; version <= 12; version++)
    await publish(page, {
      ...message,
      version,
      markdown: `# Version ${version}\n\nLatest text.`,
    });
  await rendered(page, 12);
  expect(await page.evaluate(() => saved.uri)).toBe("file:///test.md");
});

test("source scroll reaches both ends without feedback and preview gestures can reverse immediately", async ({
  page,
}) => {
  const message = await open(page);
  await rendered(page, 1);
  async function source(line, generation) {
    await publish(page, {
      type: "source-scroll",
      uri: message.uri,
      version: 1,
      line,
      maxLine: 100,
      inputTime: Date.now(),
      request: { origin: "source", documentVersion: 1, generation },
    });
  }
  const bar = page.getByRole("scrollbar");
  await source(100, 1);
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuenow")))
    .toBeGreaterThan(100);
  await expect
    .poll(async () =>
      Math.abs(
        Number(await bar.getAttribute("aria-valuenow")) -
          Number(await bar.getAttribute("aria-valuemax")),
      ),
    )
    .toBeLessThanOrEqual(1);
  expect(
    await page.evaluate(
      () =>
        messages.filter((message) => message.type === "preview-scroll").length,
    ),
  ).toBe(0);
  await source(0, 2);
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuenow")))
    .toBe(0);
  await page.locator("canvas").hover();
  for (let i = 0; i < 8; i++) await page.mouse.wheel(0, 80);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          messages.filter((message) => message.type === "preview-scroll")
            .length,
      ),
    )
    .toBeGreaterThan(0);
  const before = await page.evaluate(
    () =>
      messages.filter((message) => message.type === "preview-scroll").at(-1)
        .line,
  );
  for (let i = 0; i < 8; i++) await page.mouse.wheel(0, -80);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          messages.filter((message) => message.type === "preview-scroll").at(-1)
            .line,
      ),
    )
    .toBeLessThan(before);
  await bar.focus();
  await page.keyboard.press("End");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          messages.filter((message) => message.type === "preview-scroll").at(-1)
            .line,
      ),
    )
    .toBeCloseTo(100, 0);
  await page.keyboard.press("Home");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          messages.filter((message) => message.type === "preview-scroll").at(-1)
            .line,
      ),
    )
    .toBe(0);
});

test("code-block positions and stale source requests stay in the same mapping", async ({
  page,
}) => {
  const message = await open(page);
  await rendered(page, 1);
  const markdown =
    "# Code\n\n```js\n" +
    Array.from({ length: 100 }, (_, i) => `console.log('line ${i}');`).join(
      "\n",
    ) +
    "\n```\n\nAfter the code.\n" +
    "More reading.\n\n".repeat(30);
  await publish(page, { ...message, version: 2, markdown });
  await rendered(page, 2);
  await publish(page, {
    type: "source-scroll",
    uri: message.uri,
    version: 2,
    line: 50,
    maxLine: 140,
    inputTime: Date.now(),
    request: { origin: "source", documentVersion: 2, generation: 20 },
  });
  const bar = page.getByRole("scrollbar");
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuenow")))
    .toBeGreaterThan(100);
  const y = Number(await bar.getAttribute("aria-valuenow"));
  await publish(page, {
    type: "source-scroll",
    uri: message.uri,
    version: 1,
    line: 0,
    maxLine: 100,
    inputTime: Date.now() + 10,
    request: { origin: "source", documentVersion: 1, generation: 21 },
  });
  await publish(page, {
    type: "source-scroll",
    uri: message.uri,
    version: 2,
    line: 0,
    maxLine: 140,
    inputTime: Date.now() + 20,
    request: { origin: "source", documentVersion: 2, generation: 19 },
  });
  await page.locator("canvas").hover();
  await page.mouse.wheel(0, 20);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          messages.filter(
            (message) =>
              message.type === "preview-scroll" && message.version === 2,
          ).length,
      ),
    )
    .toBeGreaterThan(0);
  const line = await page.evaluate(
    () =>
      messages
        .filter(
          (message) =>
            message.type === "preview-scroll" && message.version === 2,
        )
        .at(-1).line,
  );
  expect(line).toBeGreaterThan(48);
  expect(line).toBeLessThan(60);
  expect(
    Number(await bar.getAttribute("aria-valuenow")),
  ).toBeGreaterThanOrEqual(y);
});

test("fast preview swipes retain their full delta despite asynchronous editor reveal feedback", async ({
  page,
}) => {
  const bundle = await build({
    entryPoints: ["vscode/src/preview/scroll-sync.ts"],
    bundle: true,
    platform: "node",
    format: "cjs",
    write: false,
    external: ["vscode"],
  });
  class Position {
    constructor(line, character = 0) {
      this.line = line;
      this.character = character;
    }
    isEqual(other) {
      return this.line === other.line && this.character === other.character;
    }
  }
  class Range {
    constructor(start, end) {
      this.start = start;
      this.end = end;
    }
  }
  const text = "# Preview\n\n" + "A paragraph of readable text.\n\n".repeat(60);
  const lines = text.split("\n");
  const document = {
    uri: { toString: () => "file:///test.md" },
    version: 1,
    lineCount: lines.length,
    lineAt: (line) => ({ text: lines[line] }),
  };
  let visible,
    revealCount = 0;
  const sourceMessages = [];
  const editor = {
    document,
    visibleRanges: [new Range(new Position(0), new Position(19))],
    revealRange(range) {
      revealCount++;
      for (const top of [Math.max(0, range.start.line - 1), range.start.line]) {
        editor.visibleRanges = [
          new Range(
            new Position(top),
            new Position(Math.min(lines.length - 1, top + 19)),
          ),
        ];
        visible({ textEditor: editor, visibleRanges: editor.visibleRanges });
      }
    },
  };
  const disposable = { dispose() {} };
  const vscode = {
    Position,
    Range,
    TextEditorRevealType: { AtTop: 1 },
    window: {
      visibleTextEditors: [editor],
      onDidChangeTextEditorSelection: () => disposable,
      onDidChangeTextEditorVisibleRanges: (fn) => {
        visible = fn;
        return disposable;
      },
    },
  };
  const module = { exports: {} };
  runInNewContext(bundle.outputFiles[0].text, {
    module,
    exports: module.exports,
    require: () => vscode,
  });
  const bridge = new module.exports.EditorScrollSync(
    {
      webview: {
        postMessage: async (message) => {
          sourceMessages.push(message);
          await publish(page, message);
          return true;
        },
      },
    },
    document,
    [],
    false,
  );
  await page.exposeFunction("scrollHost", (message) => bridge.receive(message));
  await open(page);
  await rendered(page, 1);
  await expect.poll(() => sourceMessages.length).toBe(1);
  await page.locator("canvas").hover();
  const bar = page.getByRole("scrollbar");
  async function swipe(deltaY, count) {
    await page.locator("canvas").evaluate(
      (canvas, { deltaY, count }) => {
        for (let i = 0; i < count; i++)
          canvas.dispatchEvent(
            new WheelEvent("wheel", {
              deltaY,
              deltaMode: 0,
              bubbles: true,
              cancelable: true,
            }),
          );
      },
      { deltaY, count },
    );
  }
  await swipe(50, 12);
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuenow")))
    .toBe(600);
  await expect.poll(() => revealCount).toBeGreaterThan(0);
  await page.evaluate(
    () =>
      new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      ),
  );
  expect(Number(await bar.getAttribute("aria-valuenow"))).toBe(600);
  expect(sourceMessages.length).toBe(1);
  await swipe(-50, 10);
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuenow")))
    .toBe(100);
  expect(sourceMessages.length).toBe(1);
});

test("editing after a long code block follows the wrapped caret only when it leaves the viewport", async ({
  page,
}) => {
  const message = await open(page);
  await rendered(page, 1);
  const source =
    "# Writing\n\n```js\n" + "console.log('code');\n".repeat(120) + "```\n\n";
  await publish(page, { ...message, version: 2, markdown: source });
  await rendered(page, 2);
  const bar = page.getByRole("scrollbar");
  const markdown = source + "New wrapped paragraph ".repeat(250);
  await publish(page, {
    ...message,
    version: 3,
    markdown,
    edit: { version: 3, offset: markdown.length, inputTime: Date.now() },
  });
  await rendered(page, 3);
  await expect
    .poll(async () =>
      Math.abs(
        Number(await bar.getAttribute("aria-valuenow")) -
          Number(await bar.getAttribute("aria-valuemax")),
      ),
    )
    .toBeLessThanOrEqual(1);
  const current = Number(await bar.getAttribute("aria-valuenow"));
  const updated = markdown + " a";
  await publish(page, {
    ...message,
    version: 4,
    markdown: updated,
    edit: { version: 4, offset: updated.length, inputTime: Date.now() },
  });
  await rendered(page, 4);
  await expect
    .poll(async () =>
      Math.abs(
        Number(await bar.getAttribute("aria-valuenow")) -
          Number(await bar.getAttribute("aria-valuemax")),
      ),
    )
    .toBeLessThanOrEqual(1);
  expect(
    Number(await bar.getAttribute("aria-valuenow")) - current,
  ).toBeLessThan(100);
  await page.screenshot({ path: "test-results/edit-follow-wrapped.png" });
  await publish(page, {
    ...message,
    version: 5,
    markdown:
      "# Writing\n\nVisible paragraph.\n\n" + "A paragraph.\n\n".repeat(80),
    edit: { version: 5, offset: 18, inputTime: Date.now() },
  });
  await rendered(page, 5);
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuenow")))
    .toBeLessThan(100);
  // Wait for the preceding edit's animation before checking the next edit.
  await expect
    .poll(async () => {
      const start = Number(await bar.getAttribute("aria-valuenow"));
      await page.waitForTimeout(150);
      return Number(await bar.getAttribute("aria-valuenow")) - start;
    })
    .toBe(0);
  const before = Number(await bar.getAttribute("aria-valuenow"));
  await publish(page, {
    ...message,
    version: 6,
    markdown:
      "# Writing\n\nVisible paragraph changed.\n\n" +
      "A paragraph.\n\n".repeat(80),
    edit: { version: 6, offset: 20, inputTime: Date.now() },
  });
  await rendered(page, 6);
  expect(Number(await bar.getAttribute("aria-valuenow"))).toBe(before);
});

test("reading options and templates update the existing viewer, including code wrapping", async ({
  page,
}) => {
  const { decodePng } = await import("../../../../tests/png.mjs");
  const message = await open(page);
  await rendered(page, 1);
  const bar = page.locator(".preview-scrollbar");
  const initial = Number(await bar.getAttribute("aria-valuemax"));
  await publish(page, {
    ...message,
    version: 2,
    options: {
      fontSize: 30,
      width: 300,
      justify: false,
      hyphenate: false,
      paragraphIndent: 1,
    },
  });
  await rendered(page, 2);
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuemax")))
    .toBeGreaterThan(initial);
  async function background() {
    const png = decodePng(await page.locator("canvas").screenshot());
    const offset = (2 * png.width + 2) * png.channels;
    return Array.from(png.data.subarray(offset, offset + 3));
  }
  const custom =
    'format_version=3\nversion=1\ntargets=["ui"]\n[[rule]]\nwhen=["body"]\nbackground="#123456"\ncolor="#ffffff"\nline_height=2.0';
  await publish(page, {
    ...message,
    version: 3,
    template: "custom",
    stylesheet: custom,
  });
  await rendered(page, 3);
  await expect.poll(background).toEqual([18, 52, 86]);
  await publish(page, {
    ...message,
    version: 4,
    template: "custom",
    stylesheet: custom.replace('["ui"]', '["pdf"]'),
  });
  await expect
    .poll(() =>
      page.evaluate(
        () => messages.filter((message) => message.type === "error").length,
      ),
    )
    .toBeGreaterThan(0);
  expect(await background()).toEqual([18, 52, 86]);
  await publish(page, { ...message, version: 5, template: "celadon" });
  await expect.poll(background).not.toEqual([18, 52, 86]);
  const markdown = "```text\n" + "a long code line ".repeat(100) + "\n```\n";
  await publish(page, {
    ...message,
    version: 6,
    markdown,
    options: { width: 300 },
    codeWrap: false,
  });
  await expect
    .poll(() =>
      page.evaluate(() =>
        messages.some(
          (message) => message.type === "rendered" && message.version === 6,
        ),
      ),
    )
    .toBe(true);
  const flat = Number(await bar.getAttribute("aria-valuemax"));
  await publish(page, {
    ...message,
    version: 7,
    markdown,
    options: { width: 300 },
    codeWrap: true,
  });
  await expect
    .poll(async () => Number(await bar.getAttribute("aria-valuemax")))
    .toBeGreaterThan(flat);
  await page.screenshot({ path: "test-results/reading-options.png" });
});
