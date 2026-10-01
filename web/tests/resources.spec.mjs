import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { decodePng } from "./png.mjs";

test.setTimeout(120_000);
const assets = readdirSync(fileURLToPath(new URL("../dist/assets/", import.meta.url)));
const fonts = assets.filter((name) => /\.(otf|ttf)$/.test(name)).map((name) => `/assets/${name}`);

async function host(page) {
  await page.route("**/resource-host.html", (route) => route.fulfill({
    contentType: "text/html",
    body: '<canvas id="view" style="width:600px;height:400px"></canvas>',
  }));
  await page.route("**/resource-host.js", (route) => route.fulfill({
    contentType: "text/javascript",
    path: fileURLToPath(new URL("../dist/api.js", import.meta.url)),
  }));
  await page.goto("/resource-host.html");
  await page.evaluate(async (fonts) => {
    window.api = await import("/resource-host.js");
    await window.api.init({ wasmUrl: "/markview_web_bg.wasm", fonts });
    window.requests = [];
    window.events = [];
    window.record = (events) => {
      for (const event of events) {
        window.events.push({ kind: event.kind, src: event.request.src, priority: { ...event.request.priority } });
        if (event.kind === "request") window.requests.push(event.request);
      }
    };
    window.mv = await window.api.Markview.create(document.querySelector("canvas"), undefined, { onResources: window.record });
    window.mv.resize(600, 400, 1);
    window.finish = () => { while (window.mv.stepPending(1000)) {} window.mv.frame(); };
    window.red = (width, height, color = [240, 20, 30, 255]) => {
      const rgba = new Uint8Array(width * height * 4);
      for (let i = 0; i < rgba.length; i += 4) rgba.set(color, i);
      return { width, height, rgba };
    };
  }, fonts);
}

function coloredPixels(png, matches) {
  let count = 0;
  for (let i = 0; i < png.data.length; i += png.channels) {
    if (matches(png.data[i], png.data[i + 1], png.data[i + 2])) count++;
  }
  return count;
}

test("all sources are emitted without fetching, with priority updates after requests", async ({ page }) => {
  await host(page);
  let fetched = 0;
  await page.route("**/unrequested.png", (route) => { fetched++; return route.abort(); });
  const defaults = await page.evaluate(async () => {
    window.mv.destroy();
    window.mv = await window.api.Markview.create(document.querySelector("canvas"));
    window.mv.setMarkdown("![default](/unrequested.png)");
    window.mv.frame();
    await Promise.resolve();
    return window.requests.length;
  });
  expect(defaults).toBe(0);
  expect(fetched).toBe(0);
  const result = await page.evaluate(async () => {
    window.mv.destroy();
    window.mv = await window.api.Markview.create(document.querySelector("canvas"), undefined, { onResources: window.record });
    window.mv.resize(600, 400, 1);
    const source = '![one](one) ![same](one)\n\n' + 'Reading text.\n\n'.repeat(150)
      + '![last](last)\n\n<details>\n<summary>Closed</summary>\n\n![hidden](hidden)\n\n</details>\n\n```mermaid\ngraph TD\nA-->B\n```';
    const update = window.mv.beginLayout(source);
    window.mv.frame();
    const beforeDelivery = window.events.length;
    await Promise.resolve();
    const initial = window.events.map((event) => ({ ...event }));
    update.step(0);
    window.mv.frame();
    const prefix = window.requests.map((request) => ({ src: request.src, ...request.priority }));
    update.finish();
    window.mv.frame();
    const complete = window.requests.map((request) => ({ src: request.src, ...request.priority }));
    window.mv.setScroll(window.mv.maxScroll());
    window.mv.frame();
    const scrolled = window.requests.map((request) => ({ src: request.src, ...request.priority }));
    return { beforeDelivery, initial, prefix, complete, scrolled, count: window.requests.length };
  });
  expect(result.beforeDelivery).toBe(0);
  expect(result.initial.map((event) => event.src)).toEqual(["one", "last", "hidden"]);
  expect(result.initial.every((event) => event.kind === "request" && event.priority.region === "unknown")).toBe(true);
  expect(result.prefix[0].region).toBe("visible");
  expect(result.prefix[1].region).toBe("unknown");
  expect(result.complete[1].region).toBe("offscreen");
  expect(result.complete[2].region).toBe("unknown");
  expect(result.scrolled[0].region).toBe("offscreen");
  expect(result.scrolled[1].region).toBe("visible");
  expect(result.count).toBe(3);
});

test("host pixels are copied, batched, painted and versioned across documents", async ({ page }) => {
  await host(page);
  const result = await page.evaluate(async () => {
    const source = "Selectable text. 😀\n\n![a](a)\n\n![b](b)\n\n" + "Reading text.\n\n".repeat(100);
    const update = window.mv.beginLayout(source);
    update.finish();
    await Promise.resolve();
    for (let i = 0; i < 3; i++) {
      window.mv.pointerDown(40, 25);
      window.mv.pointerUp(40, 25);
    }
    window.mv.setScroll(500);
    const selected = window.mv.selectedText();
    const initial = window.mv.contentHeight();
    const pixels = window.red(300, 180);
    window.requests[0].resolve(pixels);
    pixels.rgba.fill(0);
    window.requests[0].reject("duplicate completion");
    window.requests[1].resolve(window.red(60, 40));
    const queued = window.mv.stats().pending;
    const flushed = window.mv.frame().pending;
    const stale = update.stale;
    window.finish();
    const final = window.mv.contentHeight();
    const scroll = window.mv.scroll();
    const selectionKept = window.mv.selectedText() === selected;
    const utf16 = window.mv.stats().selectionLength === window.mv.selectedText().length;
    window.mv.setScroll(0);
    window.mv.clearSelection();
    window.mv.frame();
    return { initial, final, queued, flushed, stale, scroll, selectionKept, utf16, selected };
  });
  expect(result.queued).toBe(false);
  expect(result.flushed).toBe(true);
  expect(result.stale).toBe(true);
  expect(result.final).not.toBe(result.initial);
  expect(result.scroll).toBe(500);
  expect(result.selected).toContain("😀");
  expect(result.selectionKept).toBe(true);
  expect(result.utf16).toBe(true);
  const red = decodePng(await page.locator("canvas").screenshot());
  expect(coloredPixels(red, (r, g, b) => r > 200 && g < 60 && b < 60)).toBeGreaterThan(30_000);
  await page.evaluate(async () => {
    window.mv.setMarkdown("![a](a)");
    await Promise.resolve();
    window.requests.at(-1).resolve(window.red(300, 180, [20, 220, 30, 255]));
    window.finish();
  });
  const green = decodePng(await page.locator("canvas").screenshot());
  expect(coloredPixels(green, (r, g, b) => r < 60 && g > 180 && b < 60)).toBeGreaterThan(30_000);
});

test("arrival during a progressive replacement keeps the newest text and invalidates its handle", async ({ page }) => {
  await host(page);
  const result = await page.evaluate(async () => {
    window.mv.setMarkdown("Old document.");
    const update = window.mv.beginLayout("Newest document. 😀\n\n![a](a)\n\n" + "New content.\n\n".repeat(150));
    update.step(0);
    await Promise.resolve();
    window.requests.at(-1).resolve(window.red(20, 30));
    const done = update.step(0);
    const stale = update.stale;
    update.finish();
    window.finish();
    window.mv.selectAll();
    return { done, stale, text: window.mv.selectedText(), pending: window.mv.stats().pending };
  });
  expect(result.done).toBe(true);
  expect(result.stale).toBe(true);
  expect(result.text).toContain("Newest document.");
  expect(result.text).not.toContain("Old document.");
  expect(result.pending).toBe(false);
});

test("replacement and destruction cancel requests and ignore late or duplicate completion", async ({ page }) => {
  await host(page);
  const result = await page.evaluate(async () => {
    window.mv.setMarkdown("![old](same)");
    await Promise.resolve();
    const old = window.requests[0];
    window.mv.setMarkdown("![new](same)");
    await Promise.resolve();
    const fresh = window.requests[1];
    old.resolve(window.red(300, 180));
    old.reject("late");
    fresh.reject("Image unavailable");
    fresh.resolve(window.red(300, 180));
    window.finish();
    const placeholder = window.mv.contentHeight();
    const revision = window.mv.stats().revision;
    window.mv.setOptions({ width: 500 });
    window.mv.resize(400, 400, 1);
    window.finish();
    const preserved = !fresh.signal.aborted && window.requests.length === 2;
    window.mv.destroy();
    fresh.resolve(window.red(1, 1));
    fresh.reject("after destroy");
    return { oldCancelled: old.signal.aborted, unique: old.id !== fresh.id, cancelled: fresh.signal.aborted, placeholder, revision, preserved };
  });
  expect(result.oldCancelled).toBe(true);
  expect(result.cancelled).toBe(true);
  expect(result.unique).toBe(true);
  expect(result.preserved).toBe(true);
  expect(result.placeholder).toBeLessThan(180);
});

test("URL helpers support relative PNG, SVG and byte views, and report fetch failures", async ({ page }) => {
  await host(page);
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="90" height="40"><rect width="90" height="40" fill="#f0141e"/></svg>';
  await page.route("**/images/test.svg", (route) => route.fulfill({ contentType: "image/svg+xml", body: svg }));
  const result = await page.evaluate(async (svg) => {
    const canvas = document.createElement("canvas");
    canvas.width = 16;
    canvas.height = 12;
    canvas.getContext("2d").fillRect(0, 0, 16, 12);
    const png = await (await fetch(canvas.toDataURL())).arrayBuffer();
    const padded = new Uint8Array(png.byteLength + 12);
    padded.set(new Uint8Array(png), 5);
    const decoded = await window.api.decodeImage(new Uint8Array(padded.buffer, 5, png.byteLength));
    const diagram = await window.api.decodeImage(new Blob([svg], { type: "image/svg+xml" }));
    const svgBytes = new TextEncoder().encode('\ufeff<?xml version="1.0"?>\n<!-- diagram -->\n' + svg);
    const paddedSvg = new Uint8Array(svgBytes.length + 19);
    paddedSvg.fill(255);
    paddedSvg.set(svgBytes, 7);
    const svgArray = await window.api.decodeImage(svgBytes.slice().buffer);
    const svgView = await window.api.decodeImage(new Uint8Array(paddedSvg.buffer, 7, svgBytes.length));
    const svgBlob = await window.api.decodeImage(new Blob([svgBytes]));
    const results = [];
    const controller = new AbortController();
    const request = (src) => ({ src, signal: controller.signal,
      resolve: (pixels) => results.push({ src, width: pixels.width, height: pixels.height }),
      reject: (error) => results.push({ src, error }),
    });
    await window.api.loadImageUrl(request("test.svg"), { baseUrl: new URL("/images/", location.href) });
    await window.api.loadImageUrl(request(canvas.toDataURL()));
    await window.api.loadImageUrl(request("/images/missing.png"));
    await window.api.loadImageUrl(request("file:///tmp/image.png"));
    controller.abort();
    await window.api.loadImageUrl(request("/images/cancelled.png"));
    let aborted = false;
    try { await window.api.decodeImage(png, controller.signal); } catch { aborted = true; }
    return { decoded: [decoded.width, decoded.height], diagram: [diagram.width, diagram.height], svgInputs: [svgArray, svgView, svgBlob].map((image) => [image.width, image.height, ...image.rgba.slice(0, 4)]), results, aborted };
  }, svg);
  expect(result.decoded).toEqual([16, 12]);
  expect(result.diagram).toEqual([90, 40]);
  expect(result.svgInputs).toEqual(Array(3).fill([90, 40, 240, 20, 30, 255]));
  expect(result.results[0]).toMatchObject({ width: 90, height: 40 });
  expect(result.results[1]).toMatchObject({ width: 16, height: 12 });
  expect(result.results[2].error).toContain("404");
  expect(result.results[3].error).toContain("Unsupported");
  expect(result.results).toHaveLength(4);
  expect(result.aborted).toBe(true);
});

test("reader resource callback errors are recoverable and invalid pixels become placeholders", async ({ page }) => {
  await host(page);
  const result = await page.evaluate(async () => {
    window.mv.destroy();
    const errors = [];
    const reader = await window.api.CanvasReader.attach(document.querySelector("canvas"), {
      markdown: "![a](a)",
      resources: { onResources() { throw new Error("Host unavailable"); } },
      onError: (error) => errors.push(String(error)),
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const frames = reader.markview.stats().frames;
    reader.destroy();
    window.mv = await window.api.Markview.create(document.querySelector("canvas"), undefined, { onResources: window.record });
    window.mv.setMarkdown("![invalid](invalid)");
    await Promise.resolve();
    window.requests.at(-1).resolve({ width: Infinity, height: 1, rgba: new Uint8Array(4) });
    window.finish();
    const badDimensions = window.mv.contentHeight();
    window.mv.setMarkdown("![oversized](oversized)");
    await Promise.resolve();
    window.requests.at(-1).resolve(window.red(100_000, 1));
    window.finish();
    return { errors, frames, badDimensions, oversized: window.mv.contentHeight() };
  });
  expect(result.errors).toHaveLength(1);
  expect(result.errors[0]).toContain("Host unavailable");
  expect(result.frames).toBeGreaterThan(0);
  expect(result.badDimensions).toBeGreaterThan(90);
  expect(result.oversized).toBeGreaterThan(90);
});

test("budgeted image reflow retains a selection beyond its first published prefix", async ({ page }) => {
  await host(page);
  const result = await page.evaluate(async () => {
    window.mv.setMarkdown("![a](a)\n\n" + "Selected body. 😀\n\n".repeat(100));
    await Promise.resolve();
    window.mv.setScroll(500);
    for (let i = 0; i < 3; i++) {
      window.mv.pointerDown(40, 250);
      window.mv.pointerUp(40, 250);
    }
    const selected = window.mv.selectedText();
    window.requests.at(-1).resolve(window.red(200, 150));
    const samples = [];
    while (window.mv.stepPending(0)) {
      window.mv.frame();
      samples.push(window.mv.selectedText());
    }
    window.mv.frame();
    return { selected, kept: samples.every((text) => text === selected), final: window.mv.selectedText() };
  });
  expect(result.selected).toContain("Selected body.");
  expect(result.kept).toBe(true);
  expect(result.final).toBe(result.selected);
});


test("image priorities follow table panning and overflow clipping", async ({ page }) => {
  await host(page);
  const result = await page.evaluate(async () => {
    const cells = Array.from({ length: 8 }, (_, i) => `![image${i}](image${i})`);
    window.mv.setMarkdown(`| ${cells.join(" | ")} |\n| ${cells.map(() => "---").join(" | ")} |`);
    window.mv.setImagesClickable(true);
    await Promise.resolve();
    window.mv.frame();
    const initial = window.requests.map((request) => request.priority.region);
    const before = window.events.length;
    window.mv.pointerMove(300, 60);
    window.mv.scrollInput(100_000, 0, "external");
    window.mv.frame();
    let target = null;
    for (let y = 20; y < 200 && !target; y += 5) {
      for (let x = 25; x < 575; x += 5) {
        window.mv.pointerDown(x, y);
        const action = window.mv.pointerUp(x, y);
        if (action?.kind === "image" && action.target === "image7") { target = action.target; break; }
      }
    }
    return {
      initial, target,
      panned: window.requests.map((request) => request.priority.region),
      changes: window.events.slice(before),
    };
  });
  expect(result.initial[0]).toBe("visible");
  expect(result.initial[7]).not.toBe("visible");
  expect(result.target).toBe("image7");
  expect(result.panned[0]).not.toBe("visible");
  expect(result.panned[7]).toBe("visible");
  expect(result.changes).toContainEqual({ kind: "priority", src: "image7", priority: { region: "visible", distance: 0 } });
});
