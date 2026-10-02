// Playwright acceptance harness for the MVaaC web canvas demo.
// The checks follow the frozen contract: `docs/mvaac-web-demo.md`,
// section "Playwright acceptance".

import { expect, test } from "@playwright/test";
import { createHash } from "node:crypto";
import { decodePng, inkPixels } from "./png.mjs";

// The engine boots through wasm compilation plus a SwiftShader surface, which
// can be slow on the first run; give every check room without sleeps.
test.setTimeout(120_000);

// `boot` diagnostics are collected per page so a stalled `__markviewReady`
// can be reported with the engine's own words.
async function openDemo(page) {
  const diag = { console: [], pageErrors: [] };
  page.on("console", (msg) => {
    if (msg.type() === "error" || msg.type() === "warning") {
      diag.console.push(`[${msg.type()}] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => diag.pageErrors.push(String(err)));
  await page.goto("/test-reader.html");
  return diag;
}

function report(diag) {
  return [
    diag.pageErrors.length ? `pageerrors:\n${diag.pageErrors.join("\n")}` : "pageerrors: (none)",
    diag.console.length ? `console:\n${diag.console.join("\n")}` : "console: (clean)",
  ].join("\n");
}

// `expect.poll(...).toSatisfy` is not in this Playwright build, so the stats
// polls go through a `waitForFunction` expression over the live stats object.
function pollStats(page, condition, timeout = 30_000) {
  return page.waitForFunction(
    `(() => { const s = window.mvStats?.() ?? null; return !!s && (${condition}); })()`,
    null,
    { timeout },
  );
}

async function waitForReady(page, diag) {
  try {
    await page.waitForFunction(() => window.__markviewReady === true, null, { timeout: 90_000 });
  } catch {
    const error = await page
      .evaluate(() => window.__markviewError ?? null)
      .catch(() => "page unavailable");
    throw new Error(
      `window.__markviewReady never appeared.\n__markviewError: ${error}\n${report(diag)}`,
    );
  }
}

async function canvasBackground(page) {
  const rgb = await page.evaluate(() => {
    const style = getComputedStyle(document.querySelector("#view"));
    const m = style.backgroundColor.match(/(\d+),\s*(\d+),\s*(\d+)/);
    return m ? { r: +m[1], g: +m[2], b: +m[3] } : null;
  });
  if (!rgb) throw new Error("cannot read the canvas background color");
  return rgb;
}

// Reading text joins block text with whitespace that differs from the raw
// Markdown, so both sides are normalized before the substring check.
function normalize(text) {
  return text.replace(/\s+/g, " ").trim();
}

test.beforeEach(async ({ context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
});

// --- 1. The harness serves the demo over the fixed port ----------------------

test("serve.mjs serves index.html on the fixed port", async ({ page }) => {
  const response = await page.goto("/test-reader.html");
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toContain("text/html");
  await expect(page).toHaveTitle(/Markview/);
});

// --- 2. The page reports readiness, with captured diagnostics on failure -----

test("window.__markviewReady appears after create() and one frame", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  const error = await page.evaluate(() => window.__markviewError ?? null);
  expect(error, `__markviewError must stay unset\n${report(diag)}`).toBeNull();
});

// --- 3. The engine runs on the GPU path and the page has no errors -----------

test("stats report the Gl backend and the page logs no errors", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "true");
  const stats = await page.evaluate(() => window.mvStats());
  // Printed so a run says which device it really rendered on: the default
  // headless project is SwiftShader, `MV_GPU=1` asks for the real device.
  const device = await page.evaluate(() => {
    const gl = document.createElement("canvas").getContext("webgl2");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    return info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "unknown";
  });
  console.log(`[device] wgpu adapter: ${stats.adapter}\n[device] WebGL renderer: ${device}`);
  expect(stats.backend).toBe("Gl");
  expect(stats.adapter).not.toBe("");
  expect(diag.pageErrors, report(diag)).toEqual([]);
  expect(diag.console.filter((line) => line.startsWith("[error]")), report(diag)).toEqual([]);
});

// --- 4. The initial document publishes blocks and height ---------------------

test("the initial document has blocks > 0 and contentHeight > 0", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0 && s.contentHeight > 0");
});

// --- 5. Typing into #source re-renders ---------------------------------------

test("typing into #source re-renders the canvas", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  const before = (await page.evaluate(() => window.mvStats())).contentHeight;
  await page.fill("#source", [
    "# Replaced by the harness",
    "",
    "This document arrived through `page.fill` and must be laid out again.",
    "",
    "- alpha",
    "- beta",
    "- gamma",
    "",
  ].join("\n"));
  await pollStats(page, `s.blocks > 0 && s.contentHeight > 0 && Math.abs(s.contentHeight - ${before}) > 1`);
});

// --- 6. The rAF loop rasterizes glyphs and presents frames -------------------

test("glyphs > 0 and frames > 0 after the rAF loop runs", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.glyphs > 0 && s.frames > 0");
});

// --- 7. The canvas screenshot contains ink -----------------------------------

test("the canvas screenshot contains ink", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await expect
    .poll(() => page.evaluate(() => window.mvStats?.()?.glyphs ?? 0), { timeout: 30_000 })
    .toBeGreaterThan(0);

  const canvas = page.locator("#view");
  const buffer = await canvas.screenshot();
  const png = decodePng(buffer);
  const bg = await canvasBackground(page);
  const count = inkPixels(png, bg);
  console.log(`[ink] canvas ${png.width}x${png.height}, background rgb(${bg.r},${bg.g},${bg.b}), ink pixels: ${count}`);
  expect(count, "the canvas must show more than a trivial amount of ink").toBeGreaterThan(500);
});

// --- 8. Drag-select on the canvas, then copy ---------------------------------

test("drag-select across canvas text selects it and copies it", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await expect
    .poll(() => page.evaluate(() => window.mvStats?.()?.glyphs ?? 0), { timeout: 30_000 })
    .toBeGreaterThan(0);

  const canvas = page.locator("#view");
  const box = await canvas.boundingBox();
  const source = await page.inputValue("#source");

  // Drag horizontally across a line of text; several heights are tried because
  // the first line's y position is the engine's choice, not ours.
  const startX = box.x + 16;
  const endX = box.x + Math.min(box.width * 0.6, 420);
  let selected = "";
  for (const y of [30, 42, 20, 56, 72, 92, 116]) {
    const py = box.y + y;
    await page.mouse.move(startX, py);
    await page.mouse.down();
    await page.mouse.move(endX, py, { steps: 10 });
    await page.mouse.up();
    selected = await page.evaluate(() => window.mv.selectedText());
    if (selected.trim()) break;
  }
  expect(selected.trim(), "the drag must select some canvas text").not.toBe("");
  expect(normalize(source)).toContain(normalize(selected));
  console.log(`[selection] selected: ${JSON.stringify(selected.trim().slice(0, 80))}`);

  // Copy through Ctrl+C on the focused canvas.
  let path = "clipboard API after Ctrl+C";
  await page.keyboard.press("Control+c");
  let clipboard = null;
  try {
    clipboard = await page.evaluate(() => navigator.clipboard.readText());
  } catch {
    // The clipboard read itself can be unavailable; fall through to the button.
  }

  // Fallback: the toolbar Copy button must not error either.
  if (clipboard === null || normalize(clipboard) !== normalize(selected)) {
    path = "toolbar Copy button";
    await page.click("#copy");
    try {
      clipboard = await page.evaluate(() => navigator.clipboard.readText());
    } catch {
      clipboard = null;
    }
  }

  if (clipboard !== null) {
    expect(normalize(clipboard), `clipboard via ${path}`).toContain(normalize(selected));
    console.log(`[copy] path used: ${path}; clipboard holds ${clipboard.length} characters`);
  } else {
    // Clipboard reads are unavailable in this environment: assert the copy
    // path still ran without erroring (the notice confirms success).
    path += " (clipboard read unavailable, asserted the button path instead)";
    await expect(page.locator("#notice")).toContainText("Copied");
    console.log(`[copy] path used: ${path}`);
  }
  expect(diag.pageErrors, report(diag)).toEqual([]);
});

// --- 9. An edit reuses blocks and changes the height -------------------------

test("a second edit reuses blocks and changes contentHeight", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.contentHeight > 0");
  const before = (await page.evaluate(() => window.mvStats())).contentHeight;

  const current = await page.inputValue("#source");
  await page.fill("#source", `${current}\n\nAn appended paragraph proves incremental reuse.`);

  // `reused > 0` is visible right after the update is accepted; the height may
  // take one more debounce cycle, so the two polls stay separate.
  await pollStats(page, "s.reused > 0");
  await pollStats(page, `Math.abs(s.contentHeight - ${before}) > 0.5`);
});

// --- 10. A step stays inside its budget on a same-size replacement ------------

test("a step honours its budget when the replacement has as many blocks", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  // One synchronous script, so the page's rAF loop cannot consume the pending
  // layout between the `beginLayout` and the `step` under test.
  const result = await page.evaluate(() => {
    const section = (n, tag) =>
      `## Section ${tag}-${n}\n\nParagraph ${n} with **bold** text and \`code\`.\n`;
    const doc = (count, tag) =>
      Array.from({ length: count }, (_, i) => section(i, tag)).join("\n");

    window.mv.setMarkdown(doc(2000, "alpha"));
    const layout = window.mv.beginLayout(doc(2000, "beta"));
    const started = performance.now();
    layout.step(8);
    return {
      elapsed: performance.now() - started,
      blocks: layout.blocks,
      done: layout.done,
    };
  });

  console.log(
    `[budget] step(8) on a 2000-block replacement: ${result.elapsed.toFixed(1)} ms, ` +
      `blocks=${result.blocks}, done=${result.done}`,
  );
  // The previous snapshot also had 2000 blocks; a budget measured against it
  // instead of against this pass would finish the whole document.
  expect(result.done, "one 8 ms step must not finish a 2000-block replacement").toBe(false);
  expect(result.blocks, "a single step must not publish the whole document").toBeLessThan(1800);
  expect(result.blocks, "a step must publish the prefix it laid out").toBeGreaterThan(0);
  expect(result.elapsed, "one step must stay near its budget").toBeLessThan(600);
});

// --- 11. Non-finite arguments cannot poison the handle ------------------------

test("non-finite arguments do not break the page", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(() => {
    const attempt = (call) => {
      try {
        call();
        return "ok";
      } catch (error) {
        return `threw: ${error}`;
      }
    };
    const layout = window.mv.beginLayout("# non-finite probe\n");
    const attempts = {
      step: attempt(() => layout.step(Infinity)),
      setScroll: attempt(() => window.mv.setScroll(NaN)),
      scrollBy: attempt(() => window.mv.scrollBy(NaN)),
      resize: attempt(() => window.mv.resize(Infinity, Infinity, 2)),
    };
    // A poisoned handle throws on every later call; a healthy one still answers.
    let alive = true;
    try {
      window.mv.resize(820, 563, 1);
      window.mv.setScroll(0);
      layout.finish();
      alive = Number.isFinite(window.mv.stats().contentHeight);
    } catch (error) {
      alive = `dead: ${error}`;
    }
    return { attempts, alive };
  });

  await page.waitForTimeout(300);
  const error = await page.evaluate(() => window.__markviewError ?? null);
  console.log(`[robustness] ${JSON.stringify(result.attempts)} alive=${result.alive}`);
  expect(result.alive, "the handle must survive non-finite arguments").toBe(true);
  expect(error, `non-finite arguments must not break the page\n${report(diag)}`).toBeNull();
  await pollStats(page, "s.contentHeight > 0");
});

// --- 12. The resumable layout: no step re-scans the laid-out prefix -----------

test("the layout is resumable: monotone blocks and flat step cost", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  // The whole drive runs in one script: interleaving `page.evaluate` calls
  // would let the page's rAF loop consume the pending layout between steps.
  // 2000 sections is far above the contract's "several hundred blocks" floor
  // on purpose: this engine lays out hundreds of blocks per 8 ms step, and a
  // two-step drive cannot see a re-scanning implementation — its last step
  // would grow by too little to clear any factor worth asserting on.
  const samples = await page.evaluate(() => {
    const section = (n) =>
      `## Section ${n}\n\nParagraph ${n} with *emphasis*, **strong** and \`code spans\`.\n`;
    const document = Array.from({ length: 2000 }, (_, i) => section(i)).join("\n");

    // A published baseline, then a replacement of the same size, so every
    // `blocks` figure below describes the pending pass, not a warm-up document.
    window.mv.setMarkdown(document);
    const layout = window.mv.beginLayout(document.replaceAll("Section", "Part"));

    const samples = [];
    let previous = 0;
    let done = false;
    for (let i = 0; !done && i < 5000; i++) {
      const started = performance.now();
      layout.step(8);
      const ms = performance.now() - started;
      const blocks = layout.blocks;
      done = layout.done;
      samples.push({ step: i, blocks, ms, done });
      // Monotone: `blocks` never goes down across steps.
      if (blocks < previous) {
        throw new Error(`blocks went backwards: ${previous} -> ${blocks}`);
      }
      // Every step either strictly increases `blocks` or finishes the pass.
      if (!done && blocks === previous) {
        throw new Error(`step ${i} made no progress: blocks stayed at ${previous}`);
      }
      previous = blocks;
    }
    return samples;
  });

  expect(samples.length, "the document must finish within the step cap").toBeGreaterThan(0);
  // The step cap must not be what ends the drive; the layout must reach done.
  expect(samples.length, "the drive must end through the layout, not the cap")
    .toBeLessThan(4900);
  expect(samples.at(-1).done, "the last step must complete the layout").toBe(true);
  // `blocks` never exceeds the document's total, which the finished pass states.
  const total = samples.at(-1).blocks;
  expect(total, "a multi-thousand-block document must have been used").toBeGreaterThan(3000);
  const over = samples.find((s) => s.blocks > total || s.blocks < 0);
  expect(over, `a step exceeded the total ${total}`).toBeUndefined();

  const first = samples[0].ms;
  const last = samples.at(-1).ms;
  for (const s of samples) {
    console.log(`[resumable] step ${s.step}: blocks=${s.blocks} ${s.ms.toFixed(1)} ms${s.done ? " done" : ""}`);
  }
  console.log(`[resumable] total=${total} blocks in ${samples.length} steps; first=${first.toFixed(1)} ms, last=${last.toFixed(1)} ms`);

  // A re-scanning implementation charges the whole prefix into every step, so
  // its last step grows with the document. 4x is generous: a step includes
  // jitter, but rescan scales with all `total` blocks.
  expect(last, "the last step must not be materially slower than the first").toBeLessThan(first * 4);
});

// --- 13. A fenced block is colored on the pass that first lays it out --------

test("syntax colors land on the first pass, not one update later", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  // A document whose fenced block is on screen, so its colors are painted.
  const document = [
    "# Code",
    "",
    "```rust",
    "pub fn layout(doc: &Document) -> Snapshot {",
    "    blocks.reuse(doc)",
    "}",
    "```",
    "",
    "After the block.",
  ].join("\n");

  const paint = async () => {
    await page.evaluate((text) => window.mv.setMarkdown(text), document);
    await page.waitForFunction(() => window.mvStats().pending === false, null, { timeout: 30_000 });
    // Two frames: one to publish, one to present it.
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(null)))));
    const png = decodePng(await page.locator("#view").screenshot());
    return createHash("sha256").update(png.data).digest("hex");
  };

  const first = await paint();
  const second = await paint();
  console.log(`[code-color] first=${first.slice(0, 16)} second=${second.slice(0, 16)} identical=${first === second}`);

  // A block measured before its colors arrived renders uncolored, and only the
  // update after it picks them up — so an identical second pass would differ.
  expect(
    second,
    "an identical re-layout must paint identical pixels; a difference means the first pass missed the syntax colors",
  ).toBe(first);
});

// --- 14. Reader lifecycle: teardown, supersession and disposal ---------------

test("a reader detaches its canvas, and a superseded layout is inert", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const Markview = window.mv.constructor;
    const CanvasReader = window.mvReader.constructor;
    const mount = () => {
      const canvas = document.createElement("canvas");
      canvas.style.cssText = "width:400px;height:300px";
      document.body.append(canvas);
      return canvas;
    };
    const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)));

    // Teardown: after `destroy()` the canvas must not still feed a freed handle.
    const tornDown = mount();
    const reader = await CanvasReader.attach(tornDown, { markdown: "# Second reader\n\nText.\n" });
    reader.destroy();
    for (const type of ["pointermove", "pointerdown", "pointerup", "wheel"]) {
      tornDown.dispatchEvent(
        new PointerEvent(type, { clientX: 12, clientY: 12, bubbles: true }),
      );
    }

    // Supersession: the second `beginLayout` owns the pass; the first is inert.
    const mv = await Markview.create(mount(), {});
    const first = mv.beginLayout("# one\n\nA paragraph.\n");
    const second = mv.beginLayout("# two\n\nAnother paragraph.\n");
    const staleBefore = { stale: first.stale, done: first.done };
    const stepped = first.step(8);
    const secondAfterStaleStep = second.done;
    first.finish();
    const secondAfterStaleFinish = second.done;
    const finished = second.step(1000);
    const blocks = second.blocks;

    // A full replacement cancels the pending pass too, so a handle for it must
    // stop owning anything rather than reading the replacement's numbers.
    const replaced = await Markview.create(mount(), {});
    const abandoned = replaced.beginLayout("# pending\n\nSome text.\n");
    abandoned.step(0);
    const abandonedBefore = { stale: abandoned.stale, blocks: abandoned.blocks };
    replaced.setMarkdown("# replaced\n\nDifferent text, with more of it.\n");
    const abandonedAfter = { stale: abandoned.stale, blocks: abandoned.blocks };
    abandoned.step(1000);
    abandoned.finish();
    const abandonedSettled = abandoned.blocks;

    // Disposal after an error: the loop stops, and `destroy()` must still free.
    let errored = null;
    const failing = await CanvasReader.attach(mount(), {
      markdown: "# boom\n",
      onStats: () => { throw new Error("stats handler failed"); },
      onError: (error) => { errored = String(error); },
    });
    await frame();
    await frame();
    const stoppedOnError = errored !== null;
    failing.destroy();
    let disposed = false;
    try {
      failing.markview.stats();
    } catch (error) {
      disposed = String(error).includes("destroyed");
    }
    return { staleBefore, stepped, secondAfterStaleStep, secondAfterStaleFinish, finished, blocks, stoppedOnError, disposed, abandonedBefore, abandonedAfter, abandonedSettled };
  });

  console.log(`[lifecycle] ${JSON.stringify(result)}`);
  expect(diag.pageErrors, `torn-down listeners must not reach a freed handle\n${report(diag)}`).toEqual([]);
  expect(result.staleBefore, "the earlier layout must know it was replaced").toEqual({ stale: true, done: true });
  expect(result.stepped, "a superseded step must be a no-op that reports done").toBe(true);
  expect(result.secondAfterStaleStep, "a superseded step must not drive the new pass").toBe(false);
  expect(result.secondAfterStaleFinish, "a superseded finish must not complete the new pass").toBe(false);
  expect(result.finished, "the owning update must be able to finish its own pass").toBe(true);
  expect(result.blocks, "the new pass must publish its blocks").toBeGreaterThan(0);
  expect(result.stoppedOnError, "the loop must stop on an error").toBe(true);
  expect(result.disposed, "destroy() must free the handle even after the loop stopped").toBe(true);
  expect(result.abandonedBefore.stale, "the handle owns the pass before the replacement").toBe(false);
  expect(
    result.abandonedAfter.stale,
    "setMarkdown() must supersede the pending pass, not just cancel it in wasm",
  ).toBe(true);
  expect(
    result.abandonedSettled,
    "an abandoned handle must keep reporting what it published, not the replacement's blocks",
  ).toBe(result.abandonedAfter.blocks);
});

// --- 15. The whole document is reachable by scrolling ------------------------

test("the last line can be scrolled into view", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const reach = await page.evaluate(async () => {
    const paragraph = (n) => `## Section ${n}\n\nParagraph ${n} with enough text to wrap onto a second line in the reading column.\n`;
    const markdown = Array.from({ length: 120 }, (_, i) => paragraph(i)).join("\n");
    window.mv.setMarkdown(markdown);
    await new Promise((done) => requestAnimationFrame(() => done(null)));

    const cssHeight = document.querySelector("#view").getBoundingClientRect().height;
    window.mv.setScroll(Number.MAX_SAFE_INTEGER);
    return {
      // The clip region is the canvas box minus the two 10 px insets, so the
      // document bottom must be reachable by exactly that much.
      maxScroll: window.mv.maxScroll(),
      contentHeight: window.mv.contentHeight(),
      visible: cssHeight - 20,
    };
  });

  const bottom = reach.maxScroll + reach.visible;
  console.log(
    `[scroll] maxScroll=${reach.maxScroll.toFixed(1)} + visible=${reach.visible.toFixed(1)} ` +
      `= ${bottom.toFixed(1)} vs contentHeight=${reach.contentHeight.toFixed(1)}`,
  );
  expect(reach.contentHeight, "the document must be taller than the viewport").toBeGreaterThan(reach.visible);
  expect(
    bottom + 0.5,
    "the scroll range must reach the document's last line, not stop 2 insets short",
  ).toBeGreaterThan(reach.contentHeight);
});

// --- 16. A multi-click drag survives a progressive publication --------------

test("a word drag keeps selecting across a published prefix", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(() => {
    const text =
      "# Drag\n\nThe quick brown fox jumps over the lazy dog and keeps running through the field.\n\nSecond paragraph so the document has more than one block.\n";
    window.mv.setMarkdown(text);

    // A row of plausible first-line positions; the engine owns the layout, so
    // the drag is retried until it lands on text.
    for (const y of [40, 30, 52, 64, 78, 96, 116]) {
      const x = 24;
      // Two rapid presses on the same point: the machine turns the second into
      // a word grain, whose drag base must follow every later publication.
      window.mv.pointerDown(x, y, {});
      window.mv.pointerDown(x, y, {});
      window.mv.pointerMove(x + 30, y);
      const before = window.mv.selectedText();
      if (!before) continue;

      // Publish a prefix of a replacement pass: the revision moves under the
      // gesture while the drag is still in flight.
      const layout = window.mv.beginLayout(text.replace("quick", "slow"));
      layout.step(8);
      window.mv.pointerMove(x + 90, y);
      const after = window.mv.selectedText();
      window.mv.pointerUp(x + 90, y);
      return { before, after, stale: layout.stale };
    }
    return { before: "", after: "", stale: false };
  });

  console.log(`[drag] before=${JSON.stringify(result.before)} after=${JSON.stringify(result.after)}`);
  expect(result.before, "the multi-click press must select a word").not.toBe("");
  expect(
    result.after,
    "extending a word drag after a publication must still yield reading text; a stale base makes extract_text reject it",
  ).not.toBe("");
});

// --- 17. A narrower canvas reflows instead of clipping -----------------------

test("a narrower canvas reflows the document instead of clipping it", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const markdown = "# Wide column\n\n"
    + "A sentence that has to wrap at whatever width the reading column is given. ".repeat(16);
  await page.evaluate((text) => window.mv.setMarkdown(text), markdown);
  await page.waitForFunction(() => window.mvStats().pending === false, null, { timeout: 30_000 });
  const wide = await page.evaluate(() => {
    const s = window.mvStats();
    return { width: s.width, height: s.contentHeight };
  });

  // The reader measures the canvas box every frame and resizes the engine.
  await page.evaluate(() => {
    document.querySelector("#view").style.width = "420px";
  });
  await page.waitForFunction(
    (first) => window.mvStats().width < first - 200 && window.mvStats().pending === false,
    wide.width,
    { timeout: 30_000 },
  );
  const narrow = await page.evaluate(() => {
    const s = window.mvStats();
    return { width: s.width, height: s.contentHeight };
  });

  console.log(
    `[reflow] wide=${wide.width.toFixed(0)}x${wide.height.toFixed(0)} `
      + `narrow=${narrow.width.toFixed(0)}x${narrow.height.toFixed(0)}`,
  );
  expect(
    narrow.width,
    "the column must fit a 420 px canvas with its 20 px margins on both sides",
  ).toBeLessThanOrEqual(420 - 40 + 0.5);
  expect(
    narrow.height,
    "narrowing the column must re-wrap the text, not merely shrink the surface",
  ).toBeGreaterThan(wide.height);
});

// --- 18. A replacement pass keeps the reader where they were ----------------

test("a replacement pass keeps the reader's position", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const markdown = Array.from(
      { length: 200 },
      (_, i) => `## Section ${i}\n\nBody text for section ${i}, long enough to fill a line or two.\n`,
    ).join("\n");
    window.mv.setMarkdown(markdown);
    await new Promise((done) => requestAnimationFrame(() => done(null)));

    // Both offsets matter, for different reasons. Far past the first prefix the
    // prefix must not replace the snapshot at all; a few hundred pixels in it
    // will be published while still shorter than a page, and only the request
    // surviving that keeps the reader in place.
    const run = (offset) => {
      window.mv.setScroll(offset);
      const before = window.mv.scroll();
      const layout = window.mv.beginLayout(markdown);
      layout.step(0);
      const during = window.mv.scroll();
      const blocks = window.mvStats().blocks;
      layout.finish();
      return { before, during, after: window.mv.scroll(), blocks, published: null };
    };

    // A few hundred pixels in, the prefix *is* published — it reaches the
    // request — while still shorter than a page, so only holding the request
    // keeps the reader in place.
    const near = (() => {
      window.mv.setScroll(200);
      const before = window.mv.scroll();
      const layout = window.mv.beginLayout(markdown);
      let published = null;
      let during = null;
      for (let i = 0; i < 80 && published === null; i += 1) {
        layout.step(0);
        const s = window.mvStats();
        // The replacement's prefix replaces the 400-block snapshot once it
        // reaches the request; before that the old one is still on screen.
        if (s.blocks !== 400) {
          published = s.blocks;
          during = window.mv.scroll();
        } else if (!s.pending) {
          break;
        }
      }
      layout.finish();
      return { before, during, after: window.mv.scroll(), blocks: 400, published };
    })();

    return {
      far: run(window.mv.maxScroll() * 0.7),
      near,
      pending: window.mvStats().pending,
    };
  });

  console.log(`[scroll-keep] ${JSON.stringify(result)}`);
  for (const [name, leg] of [["far", result.far], ["near", result.near]]) {
    expect(leg.before, `${name}: the document must be scrollable`).toBeGreaterThan(0);
    // `scroll()` reports what is drawn, and a prefix shorter than a page has
    // nothing beyond its top to show, so it may lag the request — never pass it.
    expect(leg.during, `${name}: a prefix must not scroll past the request`).toBeLessThanOrEqual(leg.before + 0.5);
    expect(leg.during, `${name}: a prefix must not report a negative offset`).toBeGreaterThanOrEqual(0);
    expect(
      leg.after,
      `${name}: the position must survive the replacement, not be lost to the prefix height`,
    ).toBeCloseTo(leg.before, 0);
  }
  expect(result.near.published, "the near case must actually publish a replacement prefix").not.toBeNull();
});

// --- 19. An attached canvas is focusable ------------------------------------

test("an attached canvas can take keyboard focus", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const CanvasReader = window.mvReader.constructor;
    const mount = (tabindex) => {
      const canvas = document.createElement("canvas");
      canvas.style.cssText = "width:500px;height:300px";
      if (tabindex !== undefined) canvas.setAttribute("tabindex", tabindex);
      document.body.append(canvas);
      return canvas;
    };

    // No tabindex: the reader must supply one, and take it away again.
    const plain = mount();
    const reader = await CanvasReader.attach(plain, { markdown: "# Keys\n\nSelectable text here.\n" });
    const added = plain.getAttribute("tabindex");
    plain.focus();
    const focused = document.activeElement === plain;
    reader.destroy();
    const removed = plain.getAttribute("tabindex");

    // A caller's own tabindex is left exactly as it was.
    const owned = mount("3");
    const second = await CanvasReader.attach(owned, { markdown: "# Keys\n" });
    const kept = owned.getAttribute("tabindex");
    second.destroy();

    return { added, focused, removed, kept, keptAfter: owned.getAttribute("tabindex") };
  });

  console.log(`[focus] ${JSON.stringify(result)}`);
  expect(result.added, "attachment must make the canvas focusable").not.toBeNull();
  expect(result.focused, "canvas.focus() must land once it is focusable").toBe(true);
  expect(result.removed, "teardown must take back what it added, and nothing else").toBeNull();
  expect(result.kept, "a caller's tabindex must survive attachment").toBe("3");
  expect(result.keptAfter, "and teardown must not strip it").toBe("3");
});

// --- 21. A reflowing resize supersedes an outstanding layout handle ----------

test("a reflowing resize supersedes an outstanding layout handle", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const Markview = window.mv.constructor;
    const canvas = document.createElement("canvas");
    canvas.style.cssText = "width:900px;height:400px";
    document.body.append(canvas);
    const mv = await Markview.create(canvas, {});
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap a line.\n`;
    const markdown = Array.from({ length: 200 }, (_, i) => section(i)).join("\n");
    mv.setMarkdown(markdown);

    // A zero budget lays out exactly one block, so the pass stays pending.
    const pending = mv.beginLayout(markdown.replaceAll("Section", "Part"));
    pending.step(0);
    const before = { stale: pending.stale, done: pending.done, blocks: pending.blocks };

    // Narrowing the canvas narrows the reading column, which replaces the
    // pending pass with a reflow of its own; the handle must stop owning it.
    const narrowed = mv.resize(420, 400, 1);
    const after = { stale: pending.stale, done: pending.done, blocks: pending.blocks };

    // An inert handle must not drive the reflow, and a full step must not move
    // the progress it reports.
    const stepped = pending.step(1000);
    pending.finish();
    const settled = pending.blocks;

    // The reflow itself is still drivable, through the handle's own API.
    let guard = 0;
    while (mv.stepPending(8) && guard < 10_000) guard += 1;
    const stats = mv.stats();

    return {
      before,
      narrowed,
      after,
      stepped,
      settled,
      pending: stats.pending,
      width: stats.width,
      blocks: stats.blocks,
    };
  });

  console.log(`[resize-supersede] ${JSON.stringify(result)}`);
  expect(result.before.stale, "the handle must own its pass before the resize").toBe(false);
  expect(result.before.done, "a single block must not finish a 400-block document").toBe(false);
  expect(result.narrowed, "narrowing the column must start a reflow").toBe(true);
  expect(result.after.stale, "a reflowing resize must supersede the outstanding handle").toBe(true);
  expect(result.after.done, "a superseded handle must report done").toBe(true);
  expect(result.stepped, "a superseded step must be a no-op that reports done").toBe(true);
  expect(result.settled, "a superseded handle must not adopt the reflow's progress").toBe(result.after.blocks);
  expect(result.pending, "the reflow must still reach completion through stepPending").toBe(false);
  expect(result.width, "the reflow must lay out the narrower column").toBeLessThanOrEqual(420 - 40 + 0.5);
  expect(result.blocks, "the reflow must publish the document").toBeGreaterThan(0);
  expect(diag.pageErrors, `${report(diag)}`).toEqual([]);
});

// --- 22. A layout handle reports only its own pass's progress ----------------

test("a scrolled replacement reports its own progress, not the old document's", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap a line.\n`;
    const markdown = Array.from({ length: 200 }, (_, i) => section(i)).join("\n");
    window.mv.setMarkdown(markdown);
    await new Promise((done) => requestAnimationFrame(() => done(null)));
    const published = window.mvStats().blocks;

    // Far past the first prefix: the one-block pass publishes nothing, so the
    // previous snapshot deliberately stays on screen.
    window.mv.setScroll(window.mv.maxScroll() * 0.7);
    const held = window.mv.beginLayout(markdown);
    held.step(0);
    const during = {
      blocks: held.blocks,
      statsBlocks: window.mvStats().blocks,
      stale: held.stale,
      done: held.done,
    };
    held.finish();
    return {
      published,
      during,
      after: { blocks: held.blocks, statsBlocks: window.mvStats().blocks },
    };
  });

  console.log(`[progress] ${JSON.stringify(result)}`);
  expect(result.published, "the baseline document must be published").toBeGreaterThan(0);
  expect(result.during.stale, "the handle owns the pass it started").toBe(false);
  expect(result.during.done, "one block of a 400-block document is not the whole pass").toBe(false);
  expect(
    result.during.statsBlocks,
    "a held prefix must leave the previous snapshot on screen",
  ).toBe(result.published);
  expect(
    result.during.blocks,
    "a pass that has published nothing must report zero blocks, not the old document's count",
  ).toBe(0);
  expect(
    result.after.blocks,
    "finishing must report the whole replacement document",
  ).toBe(result.after.statsBlocks);
});

// --- 23. Stats report the selection length -----------------------------------
// The cache's hit and invalidation behaviour is covered by native tests in
// `crates/markview-web/src/state/tests.rs`; what a browser must prove here is
// only that the exported numbers agree with the page's own view of them.

test("the selection length tracks the selection and clears with it", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap a line.\n`;
    window.mv.setMarkdown(Array.from({ length: 400 }, (_, i) => section(i)).join("\n"));
    await new Promise((done) => requestAnimationFrame(() => done(null)));

    window.mv.selectAll();
    const selected = window.mv.selectedText().length;
    const reported = window.mv.stats().selectionLength;

    window.mv.clearSelection();
    const cleared = window.mv.stats().selectionLength;
    return { selected, reported, cleared };
  });

  console.log(`[selection-length] ${JSON.stringify(result)}`);
  expect(result.selected, "the document must have selectable text").toBeGreaterThan(10_000);
  expect(result.reported, "the reported count must match the selection").toBe(result.selected);
  expect(result.cleared, "clearing the selection must report zero").toBe(0);
});

// --- 24. Initial sizing and resize stay inside the device's limits -----------

test("initial sizing and a high-DPR resize stay inside the device limits", async ({ browser }) => {
  // The WebGL2 device is created with downlevel defaults, whose texture edge is
  // 2048; a DPR-2 context is what makes a 1200-px canvas exceed it.
  const context = await browser.newContext({
    deviceScaleFactor: 2,
    viewport: { width: 1200, height: 900 },
  });
  const page = await context.newPage();
  const diag = { console: [], pageErrors: [] };
  page.on("console", (msg) => {
    if (msg.type() === "error" || msg.type() === "warning") {
      diag.console.push(`[${msg.type()}] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => diag.pageErrors.push(String(err)));
  const baseURL = test.info().project.use.baseURL ?? "http://127.0.0.1:4173";
  await page.goto(`${baseURL}/test-reader.html`);
  await page.waitForFunction(() => window.__markviewReady === true, null, { timeout: 90_000 });

  const result = await page.evaluate(async () => {
    const Markview = window.mv.constructor;
    const canvas = document.createElement("canvas");
    canvas.style.cssText = "width:1200px;height:900px";
    document.body.append(canvas);
    const dpr = window.devicePixelRatio;
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}.\n`;
    const markdown = Array.from({ length: 120 }, (_, i) => section(i)).join("\n");

    let created = null;
    let createError = null;
    let mv = null;
    try {
      mv = await Markview.create(canvas, {});
      mv.setMarkdown(markdown);
      created = { width: canvas.width, height: canvas.height, dpr, blocks: mv.stats().blocks };
    } catch (error) {
      createError = String(error);
    }
    if (!created) return { created, createError, resized: null };

    const resized = { narrowed: mv.resize(1200, 900, 2), width: canvas.width, height: canvas.height };
    let guard = 0;
    while (mv.stepPending(8) && guard < 5000) guard += 1;
    resized.pending = mv.stats().pending;
    return { created, createError, resized };
  });

  await context.close();
  console.log(`[device-limits] ${JSON.stringify(result)}`);
  expect(
    result.createError,
    `create() must survive a DPR-2 canvas wider than the device texture limit\n${report(diag)}`,
  ).toBeNull();
  expect(result.created, "the handle must be created and lay the document out").not.toBeNull();
  expect(result.created.dpr, "the context must really run at DPR 2").toBe(2);
  expect(result.created.width, "the initial backing store must fit the device").toBeLessThanOrEqual(2048);
  expect(result.created.height, "the initial backing store must fit the device").toBeLessThanOrEqual(2048);
  expect(result.created.blocks, "the handle must still lay the document out").toBeGreaterThan(0);
  expect(result.resized.width, "a resize must cap the backing store at the device limit").toBeLessThanOrEqual(2048);
  expect(result.resized.height, "a resize must cap the backing store at the device limit").toBeLessThanOrEqual(2048);
  expect(
    Math.abs(result.resized.width / result.resized.height - 1200 / 900),
    "capping one edge independently distorts the backing store and desyncs the renderer's scale",
  ).toBeLessThan(0.02);
  expect(result.resized.pending, "the reflow must finish").toBe(false);
  expect(diag.pageErrors, `a high-DPR canvas must not fault the device\n${report(diag)}`).toEqual([]);
});

// --- 25. A pending replacement keeps the requested scroll through input ------

test("a wheel during a pending replacement keeps the requested scroll", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap a line.\n`;
    const markdown = Array.from({ length: 2000 }, (_, i) => section(i)).join("\n");
    window.mv.setMarkdown(markdown);
    await new Promise((done) => requestAnimationFrame(() => done(null)));
    const published = window.mvStats().blocks;

    const before = window.mv.maxScroll() * 0.7;
    window.mv.setScroll(before);
    const layout = window.mv.beginLayout(markdown);

    // Step one block at a time: a warm cache would otherwise finish the whole
    // replacement inside a single budgeted step and hide the partial prefix.
    // The window under test is a prefix that has reached the request but is
    // still shorter than a full viewport beyond it.
    let replaced = false;
    let during = null;
    for (let i = 0; i < 20_000 && !replaced; i += 1) {
      layout.step(0);
      const stats = window.mvStats();
      if (!stats.pending) break;
      if (stats.blocks !== published) {
        replaced = true;
        during = window.mv.scroll();
      }
    }

    // External motion is immediate; its request must survive a shorter prefix.
    window.mv.setScrollMode("external");
    document.querySelector("#view").dispatchEvent(
      new WheelEvent("wheel", { deltaY: 50, bubbles: true, cancelable: true }),
    );
    const afterWheel = window.mv.scroll();
    layout.finish();
    return { before, replaced, during, afterWheel, after: window.mv.scroll() };
  });

  console.log(`[pending-wheel] ${JSON.stringify(result)}`);
  expect(result.replaced, "the replacement prefix must reach the request and publish").toBe(true);
  expect(
    result.during,
    "a shorter prefix must not scroll past the request",
  ).toBeLessThanOrEqual(result.before + 0.5);
  expect(
    result.after,
    `the wheel must not discard the held request `
      + `(before=${result.before.toFixed(1)}, during=${result.during.toFixed(1)}, `
      + `afterWheel=${result.afterWheel.toFixed(1)}, after=${result.after.toFixed(1)})`,
  ).toBeCloseTo(result.before + 50, 0);
});

// --- 26. A cancelled pass keeps the warm block cache -------------------------

test("a cancelled pass does not flush the completed block cache", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap a line.\n`;
    const markdown = Array.from({ length: 80 }, (_, i) => section(i)).join("\n");
    const other = "# Other\n\nA different document entirely.\n";
    const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)));

    window.mv.setMarkdown(markdown);
    await frame();
    const blocks = window.mvStats().blocks;

    // Cancelled before the pass touches a single cached block.
    window.mv.beginLayout(other);
    window.mv.setMarkdown(markdown);
    await frame();
    const afterIdle = window.mvStats().reused;

    // Cancelled after laying out exactly one block.
    const one = window.mv.beginLayout(other);
    one.step(0);
    window.mv.setMarkdown(markdown);
    await frame();
    return { blocks, afterIdle, afterOneBlock: window.mvStats().reused };
  });

  console.log(`[cache-keep] ${JSON.stringify(result)}`);
  expect(result.blocks, "the document must have blocks to reuse").toBeGreaterThan(10);
  expect(
    result.afterIdle,
    "a pass cancelled before its first block must not flush the completed cache",
  ).toBeGreaterThanOrEqual(result.blocks);
  expect(
    result.afterOneBlock,
    "a pass cancelled after one block must not flush the completed cache",
  ).toBeGreaterThanOrEqual(result.blocks);
});

// --- 27. selectionLength counts JavaScript string units ----------------------

test("selectionLength counts UTF-16 units like the JavaScript string", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    window.mv.setMarkdown("# Emoji\n\nHello 😀 world 🎉 done, with 𝄞 a clef.\n");
    await new Promise((done) => requestAnimationFrame(() => done(null)));
    window.mv.selectAll();
    const text = window.mv.selectedText();
    return { units: text.length, points: [...text].length, reported: window.mv.stats().selectionLength };
  });

  console.log(`[utf16] units=${result.units} codePoints=${result.points} reported=${result.reported}`);
  expect(
    result.points,
    "the selected text must contain supplementary-plane characters for this check to mean anything",
  ).toBeLessThan(result.units);
  expect(
    result.reported,
    "selectionLength must match selectedText().length, which counts UTF-16 code units",
  ).toBe(result.units);
});

// --- 28. A bare canvas keeps its logical CSS size ---------------------------

test("a canvas without CSS dimensions keeps its box across frames", async ({ browser }) => {
  // Attribute-driven canvases only misbehave once the backing store doubles, so
  // this runs at DPR 2 like the device-limit check.
  const context = await browser.newContext({
    deviceScaleFactor: 2,
    viewport: { width: 1200, height: 900 },
  });
  const page = await context.newPage();
  const diag = { console: [], pageErrors: [] };
  page.on("console", (msg) => {
    if (msg.type() === "error" || msg.type() === "warning") {
      diag.console.push(`[${msg.type()}] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => diag.pageErrors.push(String(err)));
  const baseURL = test.info().project.use.baseURL ?? "http://127.0.0.1:4173";
  await page.goto(`${baseURL}/test-reader.html`);
  await page.waitForFunction(() => window.__markviewReady === true, null, { timeout: 90_000 });

  const result = await page.evaluate(async () => {
    const CanvasReader = window.mvReader.constructor;
    const canvas = document.createElement("canvas"); // no CSS dimensions at all
    document.body.append(canvas);
    const before = canvas.getBoundingClientRect();
    const reader = await CanvasReader.attach(canvas, { markdown: "# Bare\n\nSome text to lay out here.\n" });
    const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)));
    for (let i = 0; i < 8; i += 1) await frame();
    const after = canvas.getBoundingClientRect();
    const backing = { width: canvas.width, height: canvas.height };
    const pinned = { width: canvas.style.width, height: canvas.style.height };
    reader.destroy();
    const afterDestroy = canvas.getBoundingClientRect();
    return {
      before: { width: before.width, height: before.height },
      after: { width: after.width, height: after.height },
      afterDestroy: { width: afterDestroy.width, height: afterDestroy.height },
      backing,
      pinned,
      restored: { width: canvas.style.width, height: canvas.style.height },
      dpr: window.devicePixelRatio,
    };
  });

  await context.close();
  console.log(`[bare-canvas] ${JSON.stringify(result)}`);
  expect(result.dpr, "the context must really run at DPR 2").toBe(2);
  expect(
    result.after.width,
    `an attribute-sized canvas must not grow with its backing store\n${report(diag)}`,
  ).toBeCloseTo(result.before.width, 0);
  expect(result.after.height, "and its height must stay put too").toBeCloseTo(result.before.height, 0);
  expect(result.backing.width, "the backing store must fit the box and the device").toBeLessThanOrEqual(2048);
  expect(result.backing.height, "the backing store must fit the box and the device").toBeLessThanOrEqual(2048);
  expect(result.backing.width, "the box must still scale by the device ratio").toBeCloseTo(result.before.width * 2, 0);
  expect(result.restored.width, "teardown must take back the pinned CSS size").toBe("");
  expect(result.restored.height, "teardown must take back the pinned CSS size").toBe("");
  expect(
    result.afterDestroy.width,
    `teardown must not resize the element, or repeated attach/destroy cycles grow it\n${report(diag)}`,
  ).toBeCloseTo(result.before.width, 0);
  expect(
    result.afterDestroy.height,
    "and teardown must leave its height alone too",
  ).toBeCloseTo(result.before.height, 0);
  expect(diag.pageErrors, `${report(diag)}`).toEqual([]);
});

// --- 29. Scrolling under a held drag re-hit-tests the pointer ---------------

test("a held drag follows the wheel scroll", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap onto a second line in the reading column.\n`;
    window.mv.setMarkdown(Array.from({ length: 120 }, (_, i) => section(i)).join("\n"));
    await new Promise((done) => requestAnimationFrame(() => done(null)));

    // The first line's y is the engine's choice, so several heights are tried.
    let attempt = null;
    for (const y of [30, 42, 20, 56, 72, 92]) {
      window.mv.clearSelection();
      window.mv.pointerDown(40, y);
      window.mv.pointerMove(320, y);
      const before = window.mv.selectedText();
      if (before.trim()) {
        attempt = { y, before };
        break;
      }
      window.mv.pointerUp(320, y);
    }
    if (!attempt) return { error: "no drag started on any probed line" };

    // The pointer does not move; only the viewport does.
    window.mv.scrollBy(300);
    const during = window.mv.selectedText();
    window.mv.pointerUp(320, attempt.y);
    return {
      y: attempt.y,
      before: attempt.before,
      during,
      after: window.mv.selectedText(),
      scroll: window.mv.scroll(),
    };
  });

  console.log(`[drag-scroll] ${JSON.stringify(result).slice(0, 400)}`);
  expect(result.error, result.error ?? "").toBeUndefined();
  expect(result.scroll, "the wheel must have scrolled the view").toBeGreaterThan(0);
  expect(
    result.during.length,
    "a held drag must re-hit-test the pointer when the view scrolls",
  ).toBeGreaterThan(result.before.length);
  expect(
    result.after.length,
    "releasing the drag must keep the range the scroll revealed",
  ).toBeGreaterThanOrEqual(result.during.length);
});

// --- 30. Repeated cancellations keep the completed cache --------------------

test("repeated cancellations keep the completed block cache", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const section = (n) => `## Section ${n}\n\nBody text for section ${n}, long enough to wrap onto a second line.\n`;
    const markdown = Array.from({ length: 80 }, (_, i) => section(i)).join("\n");
    const other = "# Other\n\nA different document entirely.\n";
    const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)));

    window.mv.setMarkdown(markdown);
    await frame();
    const blocks = window.mvStats().blocks;

    // A pass that reuses half the geometry is abandoned, so those entries now
    // carry the abandoned pass's stamp.
    const resumed = window.mv.beginLayout(markdown);
    for (let i = 0; i < Math.floor(blocks / 2); i += 1) resumed.step(0);

    // A further pass is cancelled before it touches anything.
    window.mv.beginLayout(other);

    window.mv.setMarkdown(markdown);
    await frame();
    return { blocks, reused: window.mvStats().reused };
  });

  console.log(`[cache-resume] ${JSON.stringify(result)}`);
  expect(result.blocks, "the document must have blocks to reuse").toBeGreaterThan(10);
  expect(
    result.reused,
    "reusing geometry in an abandoned pass must not cost it its completed-pass membership",
  ).toBeGreaterThanOrEqual(result.blocks);
});

// --- 31. The legacy clipboard path restores focus ---------------------------

test("the legacy clipboard fallback gives the canvas its focus back", async ({ page }) => {
  const diag = await openDemo(page);
  await waitForReady(page, diag);
  await pollStats(page, "s.blocks > 0");

  const result = await page.evaluate(async () => {
    const canvas = document.querySelector("#view");
    canvas.focus();
    const focusedBefore = document.activeElement === canvas;
    window.mv.selectAll();

    // Force the async clipboard to reject so `legacyCopy` runs.
    const clipboard = navigator.clipboard;
    const original = clipboard.writeText;
    Object.defineProperty(clipboard, "writeText", {
      value: () => Promise.reject(new Error("clipboard blocked for the test")),
      configurable: true,
    });
    let copied = null;
    try {
      copied = await window.mv.copy();
    } finally {
      Object.defineProperty(clipboard, "writeText", { value: original, configurable: true });
    }
    return {
      focusedBefore,
      copied,
      focusedAfter: document.activeElement === canvas,
      active: document.activeElement?.id || document.activeElement?.tagName || null,
    };
  });

  console.log(`[legacy-copy] ${JSON.stringify(result)}`);
  expect(result.focusedBefore, "the canvas must be focusable and focused").toBe(true);
  expect(
    result.active,
    "the fallback must hand focus back to the canvas, or Ctrl+C and Ctrl+A stop reaching it",
  ).toBe("view");
  expect(result.focusedAfter).toBe(true);
});
