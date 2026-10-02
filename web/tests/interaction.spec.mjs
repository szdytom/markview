import { expect, test } from "@playwright/test";

test.setTimeout(120_000);

async function ready(page) {
  await page.goto("/test-reader.html");
  await page.waitForFunction(() => window.__markviewReady === true, null, { timeout: 90_000 });
}

// Discover the rendered target through the public cursor API, avoiding font-specific positions.
async function target(page) {
  return page.evaluate(() => {
    const box = document.querySelector("#view").getBoundingClientRect();
    for (let y = 12; y < Math.min(box.height, 180); y += 4) {
      for (let x = 20; x < box.width; x += 4) {
        window.mv.pointerMove(x,y);
        if (window.mv.cursor() === "pointer") return { x,y };
      }
    }
    throw new Error("no document target under the pointer");
  });
}

test("hover follows scrolling and pointer cancellation never activates", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => window.mv.setMarkdown("[A link](https://example.com)\n\n" + "Reading text.\n\n".repeat(100)));
  const point = await target(page);
  const action = await page.evaluate(({x,y}) => {
    window.mv.pointerDown(x,y);
    window.mv.cancelPointer();
    const cancelled = window.mv.pointerUp(x,y);
    window.mv.pointerDown(x,y);
    const activated = window.mv.pointerUp(x,y);
    window.mv.pointerMove(x,y);
    const before = window.mv.cursor();
    window.mv.setScroll(300);
    const after = window.mv.cursor();
    window.mv.pointerLeave();
    return {cancelled,activated,before,after,left:window.mv.cursor()};
  },point);
  expect(action.cancelled).toBeNull();
  expect(action.activated).toMatchObject({kind:"link",target:"https://example.com"});
  expect(action.before).toBe("pointer");
  expect(action.after).not.toBe("pointer");
  expect(action.left).toBe("default");
  await page.evaluate(() => { window.mv.setScroll(0); window.mv.cancelPointer(); });
  const box = await page.locator("#view").boundingBox();
  await page.mouse.click(box.x+point.x,box.y+point.y);
  await expect(page.locator("#notice")).toHaveText("Link: https://example.com");
  await page.evaluate(() => window.mv.setMarkdown('![Demo](demo.png "Image title")'));
  const image = await target(page);
  const imageResult = await page.evaluate(({x,y}) => {
    window.mv.pointerDown(x,y);
    const action = window.mv.pointerUp(x,y);
    window.mv.setImagesClickable(false);
    window.mv.pointerMove(x,y);
    return {action,cursor:window.mv.cursor()};
  },image);
  expect(imageResult.action).toMatchObject({kind:"image",target:"demo.png"});
  expect(imageResult.cursor).not.toBe("pointer");
});

test("details reflow supersedes a layout and anchors expand hidden content", async ({ page }) => {
  await ready(page);
  const source = "<details>\n<summary>Open section</summary>\n\n## Hidden\n\nHidden body.\n\n</details>\n\n" + "Reading text.\n\n".repeat(100);
  await page.evaluate(source => window.mv.setMarkdown(source),source);
  const point = await target(page);
  const result = await page.evaluate(({point,source}) => {
    const initial = window.mv.contentHeight();
    const update = window.mv.beginLayout(source);
    window.mv.pointerDown(point.x,point.y);
    const action = window.mv.pointerUp(point.x,point.y);
    const stale = update.stale;
    while (window.mv.stepPending(1000)) {}
    return {initial,expanded:window.mv.contentHeight(),stale,action};
  },{point,source});
  expect(result.action.kind).toBe("document");
  expect(result.stale).toBe(true);
  expect(result.expanded).toBeGreaterThan(result.initial);

  await page.evaluate(() => window.mv.setMarkdown("[Jump](#hidd%65n)\n\n" + "Before heading.\n\n".repeat(30) + "<details>\n<summary>Hidden section</summary>\n\n## Hidden\n\nBody.\n\n</details>\n\n" + "After heading.\n\n".repeat(30)));
  const anchor = await target(page);
  await page.evaluate(({x,y}) => { window.mv.pointerDown(x,y); window.mv.pointerUp(x,y); },anchor);
  await expect.poll(() => page.evaluate(() => window.mv.scroll())).toBeGreaterThan(300);
});

test("scroll modes take over cleanly and held selection auto-scrolls", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => {
    window.mv.setMarkdown("A paragraph to select.\n\n".repeat(100));
    window.mv.setScroll(0);
    window.mv.setScrollMode("internal");
    window.mv.scrollInput(0,500,"step");
  });
  await expect.poll(() => page.evaluate(() => window.mv.scroll())).toBeGreaterThan(20);
  const result = await page.evaluate(() => {
    const before = window.mv.scroll();
    window.mv.setScrollMode("external");
    window.mv.scrollInput(0,10,"step");
    return {before,after:window.mv.scroll()};
  });
  expect(result.after).toBeCloseTo(result.before+10,2);
  await expect.poll(() => page.evaluate(() => window.mv.scroll())).toBeCloseTo(result.after,2);
  await page.evaluate(() => {
    window.mv.setScroll(0);
    const canvas = document.querySelector("#view");
    window.mv.pointerDown(canvas.getBoundingClientRect().width/2,30);
    window.mv.pointerMove(canvas.getBoundingClientRect().width/2,canvas.getBoundingClientRect().height+30);
  });
  await expect.poll(() => page.evaluate(() => window.mv.scroll())).toBeGreaterThan(0);
  await page.evaluate(() => window.mv.cancelPointer());
});

test("wide blocks pan and their scrollbars can be dragged", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => {
    window.mv.setMarkdown("```text\nSTART " + "abcdefghijklmnopqrstuvwxyz 0123456789 ".repeat(50) + " END\n```\n\nParagraph after the wide block.");
    window.mv.pointerMove(document.querySelector("#view").getBoundingClientRect().width/2,30);
    window.mv.frame();
  });
  const before = await page.locator("#view").screenshot();
  await page.evaluate(() => { window.mv.scrollInput(160,0,"external"); window.mv.frame(); });
  const after = await page.locator("#view").screenshot();
  expect(before.equals(after), "horizontal input must change the rendered wide block").toBe(false);
  await page.evaluate(() => {
    // The overflow gutter follows the first code line; locate it by the visible rail band.
    const canvas = document.querySelector("#view");
    const x = canvas.getBoundingClientRect().width/2;
    for (let y=35;y<100;y+=2) {
      window.mv.pointerDown(x,y);
      window.mv.pointerMove(x+120,y);
      window.mv.pointerUp(x+120,y);
    }
    window.mv.clearSelection();
    window.mv.frame();
  });
  const dragged = await page.locator("#view").screenshot();
  expect(after.equals(dragged), "dragging across the scrollbar band must pan the content").toBe(false);
});

test("external scroll takes over from the displayed animation offset", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(() => {
    window.mv.setMarkdown("A scrolling paragraph.\n\n".repeat(200));
    window.mv.setScroll(0);
    window.mv.setScrollMode("internal");
    window.mv.scrollInput(0,500,"step");
    const before = window.mv.scroll();
    window.mv.scrollInput(0,-10,"external");
    const reversed = window.mv.scroll();
    window.mv.scrollInput(0,500,"step");
    window.mv.scrollBy(10);
    const direct = window.mv.scroll();
    window.mv.frame();
    return {before,reversed,direct,afterFrame:window.mv.scroll()};
  });
  expect(result).toEqual({before:0,reversed:0,direct:10,afterFrame:10});
});

test("absolute requests follow growing prefixes and retain relative travel", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(() => {
    window.mv.setMarkdown("");
    window.mv.setScroll(0);
    const layout = window.mv.beginLayout("A scrolling paragraph.\n\n".repeat(1000));
    layout.step(0);
    window.mv.setScroll(5000);
    const prefixes = [];
    while (!layout.done && window.mv.maxScroll() < 2500) {
      layout.step(0);
      prefixes.push({max:window.mv.maxScroll(),scroll:window.mv.scroll()});
    }
    window.mv.scrollBy(10);
    layout.finish();
    const final = window.mv.scroll();
    window.mv.setMarkdown("");
    window.mv.setScroll(0);
    const handoff = window.mv.beginLayout("A scrolling paragraph.\n\n".repeat(1000));
    while (!handoff.done && window.mv.maxScroll() < 2500) handoff.step(0);
    window.mv.setScroll(5000);
    const displayed = window.mv.scroll();
    window.mv.scrollInput(0,500,"step");
    window.mv.scrollInput(0,-10,"external");
    handoff.finish();
    return {prefixes,final,displayed,afterHandoff:window.mv.scroll()};
  });
  expect(result.prefixes.some(prefix => prefix.max >= 2500)).toBe(true);
  for (const prefix of result.prefixes) {
    expect(prefix.scroll).toBeCloseTo(Math.min(5000,prefix.max),2);
  }
  expect(result.final).toBeCloseTo(5010,2);
  expect(result.afterHandoff).toBeCloseTo(result.displayed-10,2);
});

test("End waits for the completed document and direct input cancels it", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(() => {
    const start = () => {
      window.mv.setMarkdown("");
      window.mv.setScroll(0);
      const layout = window.mv.beginLayout("A scrolling paragraph.\n\n".repeat(1000));
      while (!layout.done && window.mv.maxScroll() < 2500) layout.step(0);
      const prefixMax = window.mv.maxScroll();
      window.mv.setScroll(prefixMax);
      document.querySelector("#view").dispatchEvent(new KeyboardEvent("keydown",{key:"End",bubbles:true,cancelable:true}));
      return {layout,prefixMax};
    };
    const first = start();
    first.layout.finish();
    const end = {prefixMax:first.prefixMax,max:window.mv.maxScroll(),scroll:window.mv.scroll()};
    const second = start();
    window.mv.setScroll(5000);
    window.mv.scrollToEnd();
    window.mv.scrollInput(0,-10,"external");
    second.layout.finish();
    const cancelled = {prefixMax:second.prefixMax,scroll:window.mv.scroll()};
    window.mv.scrollToEnd();
    const settled = {max:window.mv.maxScroll(),scroll:window.mv.scroll()};
    return {end,cancelled,settled};
  });
  expect(result.end.max).toBeGreaterThan(result.end.prefixMax);
  expect(result.end.scroll).toBeCloseTo(result.end.max,2);
  expect(result.cancelled.scroll).toBeCloseTo(result.cancelled.prefixMax-10,2);
  expect(result.settled.scroll).toBeCloseTo(result.settled.max,2);
});

test("replacement prefixes toggle their own declared disclosure state", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(() => {
    window.mv.setMarkdown("Previous document.");
    window.mv.setScroll(0);
    const layout = window.mv.beginLayout("<details open>\n<summary>Open section</summary>\n\nDisclosure body.\n\n</details>\n\n" + "Reading text.\n\n".repeat(1000));
    layout.step(0);
    const pending = window.mv.stats().pending;
    window.mv.selectAll();
    const before = window.mv.selectedText();
    window.mv.clearSelection();
    const box = document.querySelector("#view").getBoundingClientRect();
    for (let y = 12; y < Math.min(box.height,180); y += 4) {
      for (let x = 20; x < box.width; x += 4) {
        window.mv.pointerMove(x,y);
        if (window.mv.cursor() !== "pointer") continue;
        window.mv.pointerDown(x,y);
        const action = window.mv.pointerUp(x,y);
        while (window.mv.stepPending(1000)) {}
        window.mv.selectAll();
        return {pending,before,action,after:window.mv.selectedText()};
      }
    }
    throw new Error("no disclosure summary in the published prefix");
  });
  expect(result.pending).toBe(true);
  expect(result.before).toContain("Disclosure body.");
  expect(result.action).toMatchObject({kind:"document"});
  expect(result.after).not.toContain("Disclosure body.");
});

test("external horizontal travel takes over internal vertical animation", async ({ page }) => {
  await ready(page);
  const result = await page.evaluate(async () => {
    window.mv.setMarkdown("```text\nSTART " + "abcdefghijklmnopqrstuvwxyz 0123456789 ".repeat(50) + " END\n```\n\n" + "Reading text.\n\n".repeat(200));
    window.mv.setScroll(0);
    window.mv.setScrollMode("internal");
    window.mv.pointerMove(document.querySelector("#view").getBoundingClientRect().width/2,30);
    window.mv.scrollInput(0,500,"step");
    window.mv.scrollInput(160,0,"external");
    const immediate = window.mv.scroll();
    // Let the reader's animation frames run beyond the internal easing duration.
    await new Promise(resolve => setTimeout(resolve,500));
    return {immediate,after:window.mv.scroll()};
  });
  expect(result).toEqual({immediate:0,after:0});
});

test("mode and pointer handoffs preserve displayed animation origins during layout", async ({ page }) => {
  await ready(page);
  const results = await page.evaluate(() => {
    const results = [];
    for (const handoff of ["mode","pointer"]) {
      for (const animated of [false,true]) {
        window.mv.setMarkdown("");
        window.mv.setScroll(0);
        window.mv.setScrollMode("internal");
        const layout = window.mv.beginLayout("A scrolling paragraph.\n\n".repeat(1000));
        while (!layout.done && window.mv.maxScroll() < 2500) layout.step(0);
        window.mv.setScroll(5000);
        const displayed = window.mv.scroll();
        if (animated) window.mv.scrollInput(0,500,"step");
        if (handoff === "mode") window.mv.setScrollMode("external");
        else {
          window.mv.pointerDown(30,30);
          window.mv.cancelPointer();
        }
        window.mv.scrollBy(-10);
        layout.finish();
        results.push({handoff,animated,displayed,final:window.mv.scroll()});
      }
    }
    return results;
  });
  for (const result of results) {
    expect(result.displayed).toBeGreaterThanOrEqual(2500);
    expect(result.displayed).toBeLessThan(5000);
    expect.soft(result.final, `${result.handoff}, animated=${result.animated}`).toBeCloseTo(result.animated ? result.displayed-10 : 4990,2);
  }
});
