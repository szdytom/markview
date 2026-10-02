import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { decodePng } from "./png.mjs";
const fonts = readdirSync(
	fileURLToPath(new URL("../dist/assets/", import.meta.url)),
)
	.filter((f) => /\.(otf|ttf)$/.test(f))
	.map((f) => `/assets/${f}`);
const svg = (color) =>
	`<svg xmlns="http://www.w3.org/2000/svg" width="80" height="45"><rect width="80" height="45" fill="${color}"/></svg>`;
async function host(page) {
	await page.route("**/svg-host.html", (route) =>
		route.fulfill({
			contentType: "text/html",
			body: '<input id="focus"><div id="host" style="width:1100px;height:650px"></div>',
		}),
	);
	await page.route("**/images/sample.svg", (route) =>
		route.fulfill({ contentType: "image/svg+xml", body: svg("#16a34a") }),
	);
	await page.goto("/svg-host.html");
	await page.evaluate(async (fonts) => {
		window.api = await import("/integration-api.js");
		window.fonts = await window.api.loadFontSet(
			{ sources: fonts },
			{ wasmUrl: "/markview_web_bg.wasm" },
		);
		window.errors = [];
	}, fonts);
}

test("SVG file, data URL, host bytes and inline elements paint through public resources", async ({
	page,
}, testInfo) => {
	await host(page);
	const inline =
		'<svg width="80" height="45">\n\n<rect width="80" height="45" fill="#ea580c"/>\n</svg>';
	const source = `# SVG\n\n![file](sample.svg)\n\n![data](data:image/svg+xml,${encodeURIComponent(svg("#e11d48"))})\n\n![bytes](memory.svg)\n\n${inline}\n\nAfter diagram.`;
	await page.evaluate(
		async ({ source, bytes }) => {
			const { Viewer, decodeImage, loadImageUrl } = window.api;
			window.requests = [];
			window.completed = 0;
			window.viewer = await Viewer.mount(
				document.querySelector("#host"),
				{
					fonts: window.fonts,
					markdown: source,
					resources: {
						onResources(events) {
							for (const e of events)
								if (e.kind === "request") {
									window.requests.push(e.request.src);
									if (e.request.src === "memory.svg")
										decodeImage(
											new TextEncoder().encode(bytes),
											e.request.signal,
										).then((p) => {
											window.completed++;
											e.request.resolve(p);
										});
									else
										loadImageUrl(
											{
												...e.request,
												resolve(p) {
													window.completed++;
													e.request.resolve(p);
												},
											},
											{
												baseUrl: new URL(
													"/images/",
													location.href,
												),
											},
										);
								}
						},
						onError: (e) => window.errors.push(String(e)),
					},
				},
			);
		},
		{ source, bytes: svg("#2563eb") },
	);
	await page.waitForFunction(
		() =>
			window.completed === 4 &&
			!window.viewer.reader.markview.stats().pending,
	);
	const mapped = await page.evaluate(() => {
		const offset = window.viewer.getMarkdown().indexOf("<svg width");
		const m = window.viewer.sourceToPreview(offset + 10);
		return {
			source: window.viewer
				.getMarkdown()
				.slice(m.source.start, m.source.end),
			errors: window.errors,
		};
	});
	expect(mapped.source).toBe(inline);
	expect(mapped.errors).toEqual([]);
	const screenshot = await page
		.locator("canvas")
		.screenshot({ path: testInfo.outputPath("svg.png") });
	const image = decodePng(screenshot);
	for (const color of [
		[22, 163, 74],
		[225, 29, 72],
		[37, 99, 235],
		[234, 88, 12],
	]) {
		let count = 0;
		for (let i = 0; i < image.data.length; i += image.channels)
			if (color.every((c, j) => Math.abs(c - image.data[i + j]) < 4))
				count++;
		expect(count, String(color)).toBeGreaterThan(500);
	}
});

test("SVG local references work while external dependencies and malformed XML fail explicitly", async ({
	page,
}) => {
	await host(page);
	const result = await page.evaluate(async () => {
		const { decodeImage, loadImageUrl } = window.api;
		const wrap = (body) =>
			`<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">${body}</svg>`;
		const local = await decodeImage(
			new Blob(
				[
					wrap(
						'<defs><rect id="r" width="20" height="20" fill="red"/></defs><use href="#r"/>',
					),
				],
				{ type: "image/svg+xml" },
			),
		);
		const errors = [];
		for (const body of [
			'<image href="relative.png"/>',
			'<use href="icons.svg#r"/>',
			'<rect style="fill:url(../paint.svg#p)"/>',
			'<style>@import "theme.css";</style>',
			'<image href="https://example.test/a.png"/>',
		]) {
			try {
				await decodeImage(
					new Blob([wrap(body)], { type: "image/svg+xml" }),
				);
			} catch (e) {
				errors.push(String(e));
			}
		}
		try {
			await decodeImage(
				new Blob(["<svg><rect></svg>"], { type: "image/svg+xml" }),
			);
		} catch (e) {
			errors.push(String(e));
		}
		let failed;
		await loadImageUrl({
			src: `data:image/svg+xml,${encodeURIComponent(wrap('<image href="relative.png"/>'))}`,
			signal: new AbortController().signal,
			resolve() {
				throw new Error("unexpected success");
			},
			reject(error) {
				failed = String(error);
			},
		});
		return { pixel: [...local.rgba.slice(0, 4)], errors, failed };
	});
	expect(result.pixel).toEqual([255, 0, 0, 255]);
	expect(
		result.errors
			.slice(0, 5)
			.every((e) => e.includes("SVG external resources are unsupported")),
	).toBe(true);
	expect(result.errors[5]).toContain("Invalid SVG XML");
	expect(result.failed).toContain("relative.png");
});

test("unequal image, table, nested and folded content follow source across arrival and edits", async ({
	page,
}) => {
	await host(page);
	const source =
		"# Mixed\n\n" +
		"Opening words.\n\n".repeat(25) +
		"![late](late.svg)\n\n" +
		"| Item | Description |\n| --- | --- |\n" +
		Array.from(
			{ length: 35 },
			(_, i) =>
				`| cell ${i} | ${"wrapped table content ".repeat(10)} |\n`,
		).join("") +
		"\n" +
		"> ### Nested\n>\n> - A nested list item\n>   - A deeper item\n\n" +
		"<details>\n<summary>Folded</summary>\n\n## Hidden mixed\n\n" +
		"hidden text\n\n".repeat(25) +
		"</details>\n\n" +
		"## After mixed\n\n" +
		"Ending words.\n\n".repeat(80);
	await page.evaluate(async (source) => {
		const { Editor } = window.api;
		window.request = null;
		window.editor = await Editor.mount(document.querySelector("#host"), {
			markdown: source,
			viewer: {
				fonts: window.fonts,
				resources: {
					onResources(events) {
						for (const e of events)
							if (e.kind === "request")
								window.request = e.request;
					},
				},
			},
		});
		window.readEditor = () => {
			const view = window.editor.view,
				r = view.scrollDOM.getBoundingClientRect();
			return view.posAtCoords(
				{
					x: view.contentDOM.getBoundingClientRect().left + 2,
					y: r.top + view.documentPadding.top + 1,
				},
				false,
			);
		};
	}, source);
	await page.waitForFunction(
		() =>
			window.request &&
			!window.editor.viewer.reader.markview.stats().pending,
	);
	await page.locator(".cm-content").click();
	const target = source.indexOf("cell 25");
	await page.evaluate((target) => {
		const view = window.editor.view;
		view.dispatch({
			effects: view.constructor.scrollIntoView(target, { y: "start" }),
		});
	}, target);
	await expect
		.poll(() =>
			page.evaluate(
				(target) => Math.abs(window.readEditor() - target),
				target,
			),
		)
		.toBeLessThan(10);
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.readEditor() -
						window.editor.viewer.readingPosition().offset,
				),
			),
		)
		.toBeLessThan(300);
	const before = await page.evaluate(
		() => window.editor.viewer.readingPosition().offset,
	);
	await page.evaluate(() => {
		const width = 80,
			height = 1200,
			rgba = new Uint8Array(width * height * 4);
		rgba.fill(255);
		window.request.resolve({ width, height, rgba });
	});
	await page.waitForFunction(
		() => !window.editor.viewer.reader.markview.stats().pending,
	);
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.editor.viewer.readingPosition().offset -
						window.readEditor(),
				),
			),
		)
		.toBeLessThan(300);
	expect(
		Math.abs(
			(await page.evaluate(
				() => window.editor.viewer.readingPosition().offset,
			)) - before,
		),
	).toBeLessThan(300);
	const heading = page.getByRole("button", {
		name: "After mixed",
		exact: true,
	});
	await heading.click();
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.editor.viewer.readingPosition().offset -
						window.readEditor(),
				),
			),
		)
		.toBeLessThan(100);
	const after = await page.evaluate(
		() => window.editor.viewer.readingPosition().offset,
	);
	expect(after).toBeGreaterThan(source.indexOf("## Hidden mixed"));
	await page.evaluate(() => {
		const view = window.editor.view;
		view.dispatch({
			changes: { from: 0, insert: "Added before reading.\n\n" },
		});
	});
	await page.waitForFunction(
		() => !window.editor.viewer.reader.markview.stats().pending,
	);
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.editor.viewer.readingPosition().offset -
						window.readEditor(),
				),
			),
		)
		.toBeLessThan(180);
});

test("inline SVG preserves overlapping Markdown text and empty-alt source geometry", async ({
	page,
}) => {
	await host(page);
	const image = '<svg width="100" height="140"><text>*hi</text></svg>';
	const source = `before ${image} after* tail\n\n> > <details open><summary>first</summary>\n> > # first\n> > </details>\n> > <details open><summary>second</summary>\n> > # 中文\n> > </details>`;
	await page.evaluate(async (source) => {
		window.viewer = await window.api.Viewer.mount(
			document.querySelector("#host"),
			{
				fonts: window.fonts,
				markdown: source,
				resources: {
					onResources(events) {
						for (const event of events)
							if (event.kind === "request") {
								event.request.resolve({
									width: 100,
									height: 140,
									rgba: new Uint8Array(100 * 140 * 4).fill(
										255,
									),
								});
							}
					},
				},
			},
		);
	}, source);
	await page.waitForFunction(
		() => !window.viewer.reader.markview.stats().pending,
	);
	const result = await page.evaluate(() => {
		const source = window.viewer.getMarkdown();
		const mv = window.viewer.reader.markview;
		const mapped = mv.sourceToPreview(source.indexOf("<text>"));
		const reverse = mv.previewToSource(mapped.rect.y + 1);
		mv.selectAll();
		return {
			image: source.slice(mapped.source.start, mapped.source.end),
			rect: mapped.rect,
			reverse: reverse.source,
			source: mapped.source,
			text: mv.selectedText(),
			headings: mv
				.outline()
				.entries.map((h) => source.slice(h.source.start, h.source.end)),
		};
	});
	expect(result.image).toBe(image);
	expect(result.rect.width).toBe(100);
	expect(result.rect.height).toBe(140);
	expect(result.reverse).toEqual(result.source);
	expect(result.text).toContain("after tail");
	expect(result.text).not.toContain("hi");
	expect(result.headings).toEqual(["# first", "# 中文"]);
});

test("SVG overlap keeps normalized code and clips math outside the image", async ({
	page,
}) => {
	await host(page);
	for (const source of [
		"before <svg><text>`a\nb</text></svg>  after ` tail",
		"before <svg><text>$x</text></svg> after$ tail",
	]) {
		await page.evaluate(async (source) => {
			window.viewer?.destroy();
			window.viewer = await window.api.Viewer.mount(
				document.querySelector("#host"),
				{
					fonts: window.fonts,
					markdown: source,
					resources: {
						onResources(events) {
							for (const event of events)
								if (event.kind === "request") {
									event.request.resolve({
										width: 100,
										height: 140,
										rgba: new Uint8Array(
											100 * 140 * 4,
										).fill(255),
									});
								}
						},
					},
				},
			);
		}, source);
		await page.waitForFunction(
			() => !window.viewer.reader.markview.stats().pending,
		);
		const result = await page.evaluate(() => {
			const mv = window.viewer.reader.markview;
			const source = window.viewer.getMarkdown();
			mv.selectAll();
			const offset = source.lastIndexOf("after");
			const mapped = mv.sourceToPreview(offset);
			return {
				text: mv.selectedText(),
				offset,
				mapped,
				svgEnd: source.indexOf("</svg>") + 6,
			};
		});
		expect(result.text.replace(/\s+/g, " ")).toBe("before after tail");
		expect(result.mapped.source.start).toBeGreaterThanOrEqual(
			result.svgEnd,
		);
		expect(result.mapped.source.start).toBeLessThanOrEqual(result.offset);
		expect(result.mapped.source.end).toBeGreaterThan(result.offset);
	}
});
