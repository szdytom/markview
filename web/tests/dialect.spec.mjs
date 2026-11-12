import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
const font = `/assets/${readdirSync(new URL("../dist/assets/", import.meta.url)).find((name) => name.startsWith("NotoSerif-Regular-subset-"))}`;
async function host(page, markdown, options = {}) {
	await page.route("**/dialect-test.html", (route) =>
		route.fulfill({
			contentType: "text/html",
			body: '<main style="height:600px;width:650px"></main>',
		}),
	);
	await page.goto("/dialect-test.html");
	await page.evaluate(
		async ({ font, markdown, options }) => {
			const api = await import("/integration-api.js");
			const fonts = await api.loadFontSet(
				{ sources: [font] },
				{ wasmUrl: "/markview_web_bg.wasm" },
			);
			window.viewer = await api.Viewer.mount(
				document.querySelector("main"),
				{ fonts, markdown, markview: options },
			);
			fonts.destroy();
		},
		{ font, markdown, options },
	);
	await page.waitForFunction(() => !viewer.reader.markview.stats().pending);
}
async function text(page) {
	return page.evaluate(() => {
		const engine = viewer.reader.markview;
		engine.selectAll();
		return engine.selectedText();
	});
}

test("live dialect switches reuse core comments/highlights and preserve independent hard breaks", async ({
	page,
}) => {
	await host(
		page,
		"# Dialect\n\nVisible %%SECRET%% and ==marked==.\n\n```text\n%%CODE%%\n```\n\n> [!note] Note\n> Body.\n",
	);
	expect(await text(page)).toContain("SECRET");
	expect(await text(page)).toContain("==marked==");
	const version = await page.evaluate(
		() => viewer.reader.markview.stats().documentVersion,
	);
	await page.evaluate(() =>
		viewer.setOptions({
			markdownFlavor: "obsidian",
			highlightMarks: false,
		}),
	);
	const selected = await text(page);
	expect(selected).not.toContain("SECRET");
	expect(selected).not.toContain("==marked==");
	expect(selected).toContain("%%CODE%%");
	expect(
		await page.evaluate(
			() => viewer.reader.markview.stats().documentVersion,
		),
	).toBeGreaterThan(version);
	await page.evaluate(() =>
		viewer.setOptions({ markdownFlavor: "gfm", highlightMarks: true }),
	);
	expect(await text(page)).toContain("SECRET");
	expect(await text(page)).not.toContain("==marked==");
	await page.evaluate(() => viewer.setMarkdown("alpha\nbeta\ngamma\n"));
	await page.waitForFunction(() => !viewer.reader.markview.stats().pending);
	for (const flavor of ["gfm", "obsidian"]) {
		const heights = await page.evaluate((flavor) => {
			viewer.setOptions({ markdownFlavor: flavor, hardBreaks: false });
			const soft = viewer.reader.markview.contentHeight();
			viewer.setOptions({ markdownFlavor: flavor, hardBreaks: true });
			return [soft, viewer.reader.markview.contentHeight()];
		}, flavor);
		expect(heights[1]).toBeGreaterThan(heights[0]);
	}
});

test("dialect changes reparse pending source and invalidate its old layout without reparsing layout-only changes", async ({
	page,
}) => {
	await host(page, "# Original\n\nOriginal text.");
	const result = await page.evaluate(() => {
		const engine = viewer.reader.markview;
		const source =
			"# Newest\n\nNewest %%SECRET%% ==marked== text.\n\n" +
			"Body.\n\n".repeat(200);
		const pending = engine.beginLayout(source);
		const before = engine.stats().documentVersion;
		engine.setOptions({ markdownFlavor: "obsidian" });
		engine.selectAll();
		const selected = engine.selectedText();
		const parsed = engine.stats().documentVersion;
		engine.setOptions({ markdownFlavor: "obsidian", fontSize: 24 });
		return {
			stale: pending.stale,
			before,
			parsed,
			after: engine.stats().documentVersion,
			selected,
			geometry: engine.sourceToPreview(source.indexOf("Newest", 3)),
		};
	});
	expect(result.stale).toBe(true);
	expect(result.parsed).toBeGreaterThan(result.before);
	expect(result.after).toBe(result.parsed);
	expect(result.selected).toContain("Newest");
	expect(result.selected).not.toContain("Original");
	expect(result.selected).not.toContain("SECRET");
	expect(result.geometry?.documentVersion).toBe(result.parsed);
});

test("grid tables default on and switch independently of the preset", async ({
	page,
}) => {
	await host(page, "+-------+\n| alpha |\n+-------+\n");
	expect(await text(page)).toContain("alpha");
	expect(await text(page)).not.toContain("+-------+");
	for (const markdownFlavor of ["gfm", "obsidian"]) {
		for (const gridTables of [false, true]) {
			await page.evaluate((options) => viewer.setOptions(options), {
				markdownFlavor,
				gridTables,
			});
			const selected = await text(page);
			expect(selected).toContain("alpha");
			expect(selected.includes("+-------+")).toBe(!gridTables);
		}
	}
});
