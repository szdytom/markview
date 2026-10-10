import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { decodePng, inkPixels } from "./png.mjs";

test.setTimeout(120_000);
const fonts = readdirSync(fileURLToPath(new URL("../dist/assets/", import.meta.url)))
  .filter(name => /\.(otf|ttf)$/.test(name)).map(name => `/assets/${name}`);

async function host(page) {
  await page.route("**/navigation.html", route => route.fulfill({ contentType: "text/html",
    body: '<input id="focus"><div id="host" style="width:640px;height:400px"></div>' }));
  await page.route("**/viewer.js", route => route.fulfill({ contentType: "text/javascript",
    path: fileURLToPath(new URL("../dist/api.js", import.meta.url)) }));
  await page.goto("/navigation.html");
  await page.evaluate(async fonts => {
    window.api = await import("/viewer.js");
    await window.api.init({ wasmUrl: "/markview_web_bg.wasm", fonts });
    window.events = [];
    window.sections = [];
    window.errors = [];
  }, fonts);
}

async function mount(page, markdown, options = {}) {
  await page.evaluate(async ({ markdown, options }) => {
    window.viewer = await window.api.Viewer.mount(document.querySelector("#host"), {
      ...options, markdown, onReadingPosition: p => window.events.push(p),
      onSectionChange: h => window.sections.push(h?.anchor ?? null),
      onError: e => window.errors.push(String(e)),
    });
  }, { markdown, options });
}

async function settled(page) {
  await page.waitForFunction(() => !window.viewer.reader.markview.stats().pending
    && window.viewer.readingPosition() !== null);
}

test("public mapping and complete TOC use UTF-16 and isolate replacement versions", async ({ page }) => {
  await host(page);
  await mount(page, "# old\n\n" + "old text\n\n".repeat(100));
  await settled(page);
  const result = await page.evaluate(() => {
    const mv = window.viewer.reader.markview;
    const before = mv.stats().documentVersion;
    const source = "# 中文😀\r\n\r\ne\u0301 **strong**\r\n\r\n" + "word ".repeat(500)
      + "\r\n\r\n```rust\r\n" + Array.from({length: 100}, (_, i) => `line ${i}`).join("\r\n")
      + "\r\n```\r\n\r\n<details>\n<summary>closed</summary>\n\n### 中文😀\n\nhidden\n\n</details>\n\n# 中文😀";
    const update = mv.beginLayout(source);
    const toc = mv.outline();
    const unpublished = mv.sourceToPreview(source.length - 3);
    update.finish();
    const offset = source.indexOf("line 80");
    const mapped = mv.sourceToPreview(offset);
    const reversed = mv.previewToSource(mapped.rect.y + 1);
    const headingText = toc.entries.map(h => source.slice(h.source.start,h.source.end));
    const revision = mv.stats().revision;
    mv.resize(350,400,1);
    while (mv.stepPending(1000)) {}
    return { before, toc, unpublished, mapped, reversed, offset, headingText,
      after: mv.stats().documentVersion, revisionChanged: mv.stats().revision > revision };
  });
  expect(result.toc.entries.map(h => h.level)).toEqual([1,3,1]);
  expect(new Set(result.toc.entries.map(h => h.anchor)).size).toBe(3);
  expect(result.headingText).toEqual(["# 中文😀", "### 中文😀", "# 中文😀"]);
  expect(result.unpublished).toBeNull();
  expect(result.mapped.source.start).toBe(result.offset);
  expect(result.reversed.source.start).toBe(result.offset);
  expect(result.toc.documentVersion).toBe(result.before+1);
  expect(result.after).toBe(result.toc.documentVersion);
  expect(result.revisionChanged).toBe(true);
});

test("scroll anchors retain original CR, CRLF and mixed Unicode source lines", async ({ page }) => {
  await host(page);
  await mount(page, "Start");
  for (const source of [
    "# One\r\rParagraph\r\r# Two",
    "# 中文😀\r\rParagraph e\u0301\r\n\r\n# Two\n\nLast 😀\r",
  ]) {
    await page.evaluate(source => window.viewer.setMarkdown(source, 0), source);
    await settled(page);
    const anchors = await page.evaluate(() => window.viewer.scrollAnchors().anchors);
    const lines = source.split(/\r\n|[\r\n]/).filter(line => line.length);
    expect(anchors.map(anchor => anchor.source)).toEqual(lines.map(line => ({
      start: source.indexOf(line), end: source.indexOf(line) + line.length,
    })));
    expect(anchors.map(anchor => source.slice(anchor.source.start, anchor.source.end))).toEqual(lines);
    for (let i = 1; i < anchors.length; i++)
      expect(anchors[i].top).toBeGreaterThan(anchors[i - 1].bottom);
  }
});

test("text-free collapsed container anchors exclude hidden lines across expansion", async ({ page }) => {
  await host(page);
  await mount(page, "Start");
  for (const source of [
    "---\r\ntitle: hidden 中文😀\r\nother: body\r\n---\r\n\r\n# After",
    "<details>\n<summary></summary>\n\nhidden 中文😀 body\n\n</details>\n\n# After",
    "> <details>\r> <summary></summary>\r>\r> hidden 中文😀 body\r>\r> </details>\r\r# After",
  ]) {
    await page.evaluate(source => window.viewer.setMarkdown(source, 0), source);
    await settled(page);
    const closed = await page.evaluate(() => window.viewer.scrollAnchors());
    expect(closed.anchors.map(anchor => source.slice(anchor.source.start, anchor.source.end)))
      .toEqual([source.split(/[\r\n]/)[0], "# After"]);
    const hidden = source.indexOf("hidden");
    for (const expanded of [true, false]) {
      const result = await page.evaluate(() => {
        const mv = window.viewer.reader.markview;
        let point;
        for (let y = 1; y < 100 && !point; y += 2)
          for (let x = 1; x < 100; x += 2) {
            mv.pointerMove(x, y);
            if (mv.cursor() === "pointer") {
              point = { x, y };
              break;
            }
          }
        if (!point) throw new Error("Missing disclosure header");
        mv.cancelPointer();
        const before = mv.stats();
        mv.pointerDown(point.x, point.y);
        const action = mv.pointerUp(point.x, point.y);
        return { action, before, after: mv.stats() };
      });
      expect(result.action).toMatchObject({ kind: "document", reflowed: false });
      expect(result.after.pending).toBe(false);
      expect(result.after.documentVersion).toBe(result.before.documentVersion);
      expect(result.after.revision).toBeGreaterThan(result.before.revision);
      await settled(page);
      const batch = await page.evaluate(previous => window.viewer.scrollAnchors(previous), closed);
      expect(batch.fromBlock).toBe(0);
      expect(batch.anchors.some(anchor => anchor.source.start <= hidden && hidden < anchor.source.end))
        .toBe(expanded);
    }
  }
});

test("viewer navigation waits for layout, expands TOC targets and preserves focus", async ({ page }) => {
  await host(page);
  const source = "# first\n\n" + "body text\n\n".repeat(1500)
    + "<details>\n<summary>closed</summary>\n\n## hidden\n\ninside\n\n</details>\n\n" + "end\n\n".repeat(100);
  await mount(page, source, { stepBudgetMs: 0.1 });
  const initial = await page.evaluate(() => {
    document.querySelector("#focus").focus();
    const toc = window.viewer.outline();
    const pending = window.viewer.reader.markview.stats().pending;
    const ok = window.viewer.navigateHeading(toc.entries[1].anchor);
    return { pending, ok };
  });
  expect(initial.pending).toBe(true);
  expect(initial.ok).toBe(true);
  await settled(page);
  const result = await page.evaluate(() => ({
    position: window.viewer.readingPosition(), section: window.viewer.currentSection(),
    focus: document.activeElement.id, errors: window.errors,
  }));
  expect(result.section.anchor).toBe("hidden");
  expect(result.position.reason).toBe("programmatic");
  expect(result.focus).toBe("focus");
  expect(result.errors).toEqual([]);
  await page.evaluate(() => window.viewer.scrollToSource(0));
  await expect.poll(() => page.evaluate(() => window.viewer.reader.markview.scroll())).toBeLessThan(100);
  await page.evaluate(() => window.viewer.setMarkdown("# fresh\n\n" + "fresh text\n\n".repeat(300), 0));
  await settled(page);
  expect(await page.evaluate(() => window.viewer.outline().entries[0].text)).toBe("fresh");
});

test("source reading anchors survive width and image reflow; user motion takes ownership", async ({ page }) => {
  await host(page);
  const source = "# first\n\n![image](late.png)\n\n" + "word ".repeat(1800) + "\n\n# last\n\n" + "end\n\n".repeat(100);
  await page.evaluate(() => { window.requests = []; });
  await page.evaluate(async source => {
    window.viewer = await window.api.Viewer.mount(document.querySelector("#host"), {
      markdown: source, resources: { onResources: events => {
        for (const event of events) if (event.kind === "request") window.requests.push(event.request);
      } }, onReadingPosition: p => window.events.push(p),
    });
  }, source);
  await settled(page);
  await page.evaluate(() => window.viewer.scrollToSource(4000, 0.2));
  await expect.poll(() => page.evaluate(() => window.viewer.readingPosition()?.offset ?? 0)).toBeGreaterThan(3500);
  const before = await page.evaluate(() => window.viewer.readingPosition().offset);
  await page.evaluate(() => {
    document.querySelector("#host").style.width = "400px";
    const rgba = new Uint8Array(4*20*500).fill(255);
    window.requests[0].resolve({ width:20, height:500, rgba });
  });
  await settled(page);
  await expect.poll(() => page.evaluate(() => Math.abs(window.viewer.readingPosition().offset-window.events.findLast(p => p.reason === "programmatic").offset))).toBeLessThan(100);
  const after = await page.evaluate(() => window.viewer.readingPosition().offset);
  expect(Math.abs(before-after)).toBeLessThan(100);
  await page.locator("canvas").hover();
  await page.mouse.wheel(0,450);
  await expect.poll(() => page.evaluate(() => window.viewer.readingPosition().reason)).toBe("user");
  await expect.poll(() => page.evaluate(() => window.viewer.readingPosition().offset)).toBeGreaterThan(after);
  await page.evaluate(() => { window.viewer.destroy(); window.viewer.destroy(); });
  expect(await page.locator("canvas").count()).toBe(0);
  await mount(page,"# remounted\n\nReadable again.");
  await settled(page);
  expect(await page.evaluate(() => window.viewer.outline().entries[0].text)).toBe("remounted");
});


test("new user input cancels an unpublished source target and replacement drops stale events", async ({ page }) => {
  await host(page);
  const source = "# start\n\n" + "body text\n\n".repeat(1000) + "# far\n\nend";
  await mount(page, source, { stepBudgetMs: 0 });
  const state = await page.evaluate(() => {
    window.viewer.scrollToSource(window.viewer.getMarkdown().indexOf("# far"));
    window.viewer.canvas.dispatchEvent(new WheelEvent("wheel", { deltaY: 100, bubbles:true, cancelable:true }));
    return { pending:window.viewer.reader.markview.stats().pending };
  });
  expect(state.pending).toBe(true);
  await settled(page);
  expect(await page.evaluate(() => window.viewer.readingPosition().offset)).toBeLessThan(1000);
  await page.evaluate(() => {
    window.viewer.setMarkdown("# latest\n\nUpdated document.",0);
    window.events = [];
  });
  await settled(page);
  const versions = await page.evaluate(() => ({ current:window.viewer.outline().documentVersion,
    seen:window.events.map(p => p.documentVersion) }));
  expect(versions.seen.length).toBeGreaterThan(0);
  expect(versions.seen.every(version => version === versions.current)).toBe(true);
  const buffer = await page.locator("canvas").screenshot({ path: test.info().outputPath("viewer.png") });
  const png = decodePng(buffer);
  expect(inkPixels(png, {r:png.data[0],g:png.data[1],b:png.data[2]})).toBeGreaterThan(500);
});

test("source queries follow horizontally panned code", async ({ page }) => {
  await host(page);
  const source = "```text\n" + "0123456789 ".repeat(200) + "\n```";
  await mount(page, source);
  await settled(page);
  const result = await page.evaluate(source => {
    const mv = window.viewer.reader.markview;
    const start = source.indexOf("0123456789");
    const before = mv.sourceToPreview(start);
    mv.pointerMove(100,30);
    mv.scrollInput(700,0,"external");
    const reverse = mv.previewToSource(before.rect.y + 1);
    const hidden = mv.sourceToPreview(start);
    const offset = start + 100;
    const visible = mv.sourceToPreview(offset);
    return { before, reverse, hidden, visible, offset };
  }, source);
  expect(result.reverse.source.start).toBeGreaterThan(result.before.source.start + 50);
  expect(result.hidden.source).toEqual(result.reverse.source);
  expect(result.visible.source.start).toBeLessThanOrEqual(result.offset);
  expect(result.visible.source.end).toBeGreaterThan(result.offset);
  expect(result.visible.rect.x).toBeGreaterThan(0);
  expect(result.visible.rect.x + result.visible.rect.width).toBeLessThan(640);
});
