import { expect, test } from "@playwright/test";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const site = fileURLToPath(new URL("../dist/", import.meta.url));
const fontDir = fileURLToPath(new URL("../../crates/markview-core/tests/fonts/", import.meta.url));
const assets = readdirSync(`${site}/assets`);
const fontUrl = (name) => `/assets/${assets.find((file) => file.startsWith(`${name}-subset-`))}`;

// Exercise the published bundle in its own module instance, without demo boot.
async function hostPage(page) {
  await page.route("**/font-host.html", (route) => route.fulfill({
    contentType: "text/html",
    body: '<canvas id="view" style="width:600px;height:400px"></canvas>',
  }));
  await page.route("**/font-host/index.js", (route) => route.fulfill({
    contentType: "text/javascript",
    path: fileURLToPath(new URL("../dist/api.js", import.meta.url)),
  }));
  await page.goto("/font-host.html");
}

test("test fonts belong to the regression host and are absent from the SPA and wasm", () => {
  const wasm = readFileSync(`${site}/markview_web_bg.wasm`);
  const demo = readFileSync(`${site}/main.js`, "utf8") + readFileSync(`${site}/main.css`, "utf8");
  expect(demo).not.toMatch(/Noto[\w-]+-subset/);
  const faces = readdirSync(fontDir).filter((file) => /\.(otf|ttf)$/.test(file));
  expect(assets).toHaveLength(faces.length);
  for (const face of faces) {
    const bytes = readFileSync(`${fontDir}/${face}`);
    expect(wasm.indexOf(bytes), `${face} must not be embedded`).toBe(-1);
    const name = face.slice(0, face.lastIndexOf("."));
    const emitted = assets.find((file) => file.startsWith(`${name}-`));
    expect(readFileSync(`${site}/assets/${emitted}`).equals(bytes)).toBe(true);
  }
});

test("regression host waits for local fonts before creating readers and fetches each once", async ({ page }) => {
  const responses = [];
  page.on("response", (response) => {
    if (/\/assets\/.*\.(otf|ttf)$/.test(response.url())) responses.push(response);
  });
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  await page.route("**/assets/NotoSerif-Regular-subset-*.otf", async (route) => {
    await gate;
    await route.continue();
  });
  const request = page.waitForRequest(/\/assets\/NotoSerif-Regular-subset-.*\.otf$/);
  try {
    await page.goto("/test-reader.html");
    await request;
    expect(await page.evaluate(() => window.__markviewReady === true)).toBe(false);
    expect(await page.evaluate(() => window.mv !== undefined)).toBe(false);
  } finally {
    release();
  }
  await page.waitForFunction(() => window.__markviewReady === true);
  expect(responses).toHaveLength(16);
  for (const response of responses) {
    expect(response.status()).toBe(200);
    expect(response.headers()["content-type"]).toMatch(/^font\/(otf|ttf)$/);
  }
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    document.body.append(canvas);
    const reader = await window.mvReader.constructor.attach(canvas, { markdown: "Host fonts again." });
    reader.markview.frame();
    reader.destroy();
    canvas.remove();
  });
  expect(responses).toHaveLength(16);
});

test("init accepts URLs and byte views, shares fonts and keeps KaTeX embedded", async ({ page }) => {
  await hostPage(page);
  const requests = [];
  page.on("request", (request) => {
    if (/\.(otf|ttf)$/.test(request.url())) requests.push(request.url());
  });
  const result = await page.evaluate(async (urls) => {
    const { init, Markview } = await import("/font-host/index.js");
    const buffer = await (await fetch(urls.serif)).arrayBuffer();
    const mono = new Uint8Array(await (await fetch(urls.mono)).arrayBuffer());
    const padded = new Uint8Array(mono.length + 19);
    padded.set(mono, 7);
    const first = init({
      wasmUrl: "/markview_web_bg.wasm",
      fonts: [buffer, new Uint8Array(padded.buffer, 7, mono.length), urls.italic, new URL(urls.sans, location.href)],
    });
    const concurrent = init({ fonts: ["/missing.otf"] });
    const shared = first === concurrent;
    await Promise.all([first, concurrent]);
    await init({ fonts: ["/also-missing.otf"] });
    const canvas = document.querySelector("#view");
    const mv = await Markview.create(canvas);
    mv.resize(600, 400, 1);
    mv.setMarkdown("```math\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n```");
    const mathGlyphs = mv.frame().glyphs;
    mv.setMarkdown("A host paragraph with *italic* and `code`.\n\n$x^2 + y^2 = z^2$");
    const glyphs = mv.frame().glyphs;
    mv.setOptions({ theme: "dark" });
    const afterOptions = mv.frame().glyphs;
    mv.destroy();
    return { shared, mathGlyphs, glyphs, afterOptions };
  }, {
    serif: fontUrl("NotoSerif-Regular"), mono: fontUrl("NotoSansMono-Regular"),
    italic: fontUrl("NotoSerif-Italic"), sans: fontUrl("NotoSans-Regular"),
  });
  expect(result.shared).toBe(true);
  expect(result.mathGlyphs).toBeGreaterThan(0);
  expect(result.glyphs).toBeGreaterThan(result.mathGlyphs);
  expect(result.afterOptions).toBeGreaterThan(0);
  expect(requests).toHaveLength(4);
  expect(requests.some((url) => /KaTeX|missing/.test(url))).toBe(false);
});

test("font download and validation failures reject init and allow retry", async ({ page }) => {
  await hostPage(page);
  const wasmRequests = [];
  page.on("request", (request) => {
    if (request.url().endsWith(".wasm")) wasmRequests.push(request.url());
  });
  const result = await page.evaluate(async (url) => {
    const { init, Markview } = await import("/font-host/index.js");
    const errors = [];
    for (const fonts of [["/missing.otf"], [new TextEncoder().encode("<!doctype html>404")]]) {
      try {
        await init({ wasmUrl: "/markview_web_bg.wasm", fonts });
      } catch (error) {
        errors.push(String(error));
      }
    }
    await init({ wasmUrl: "/markview_web_bg.wasm", fonts: [url] });
    const mv = await Markview.create(document.querySelector("#view"));
    mv.resize(600, 400, 1);
    mv.setMarkdown("Host font recovery.");
    const glyphs = mv.frame().glyphs;
    mv.destroy();
    return { errors, glyphs };
  }, fontUrl("NotoSerif-Regular"));
  expect(result.errors).toHaveLength(2);
  expect(result.errors[0]).toContain("missing.otf");
  expect(result.errors[0]).toContain("HTTP 404");
  expect(result.errors[1]).toContain("invalid host font at index 0");
  expect(result.glyphs).toBeGreaterThan(0);
  expect(wasmRequests).toHaveLength(1);
});

test("retry uses the corrected wasm URL after a pending load fails", async ({ page }) => {
  await hostPage(page);
  const wasmRequests = [];
  page.on("request", (request) => {
    if (request.url().endsWith(".wasm")) wasmRequests.push(new URL(request.url()).pathname);
  });
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  await page.route("**/delayed-missing.wasm", async (route) => {
    await gate;
    await route.fulfill({ status: 404, contentType: "application/wasm", body: "Not Found" });
  });
  await page.exposeFunction("releaseWasm", () => release());
  let result;
  try {
    result = await page.evaluate(async (url) => {
      const { init, Markview } = await import("/font-host/index.js");
      let error;
      try {
        await init({ wasmUrl: "/delayed-missing.wasm", fonts: ["/missing.otf"] });
      } catch (failure) {
        error = String(failure);
      }
      const retry = init({ wasmUrl: "/markview_web_bg.wasm", fonts: [url] });
      const concurrent = init({ wasmUrl: "/also-missing.wasm" });
      await window.releaseWasm();
      await Promise.all([retry, concurrent]);
      const mv = await Markview.create(document.querySelector("#view"));
      mv.resize(600, 400, 1);
      mv.setMarkdown("Corrected host fonts and wasm URL.");
      const glyphs = mv.frame().glyphs;
      mv.destroy();
      return { error, shared: retry === concurrent, glyphs };
    }, fontUrl("NotoSerif-Regular"));
  } finally {
    release();
  }
  expect(result.error).toContain("missing.otf");
  expect(result.error).toContain("HTTP 404");
  expect(result.shared).toBe(true);
  expect(result.glyphs).toBeGreaterThan(0);
  expect(wasmRequests).toEqual(["/delayed-missing.wasm", "/markview_web_bg.wasm"]);
});
