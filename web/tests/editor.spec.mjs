import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { decodePng, inkPixels } from "./png.mjs";

test.setTimeout(120_000);
const fonts = readdirSync(
	fileURLToPath(new URL("../dist/assets/", import.meta.url)),
)
	.filter((name) => /\.(otf|ttf)$/.test(name))
	.map((name) => `/assets/${name}`);

async function host(page, markdown, options = {}, imageSize) {
	await page.route("**/editor-host.html", (route) =>
		route.fulfill({
			contentType: "text/html",
			body: '<input id="outside"><div id="host" style="width:1180px;height:650px"></div>',
		}),
	);
	await page.route("**/editor-api.js", (route) =>
		route.fulfill({
			contentType: "text/javascript",
			path: fileURLToPath(
				new URL("../dist/editor-api.js", import.meta.url),
			),
		}),
	);
	await page.goto("/editor-host.html");
	await page.evaluate(
		async ({ markdown, options, fonts, imageSize }) => {
			window.api = await import("/editor-api.js");
			window.changes = [];
			window.editor = await window.api.Editor.mount(
				document.querySelector("#host"),
				{
					...options,
					markdown,
					onChange: (change) => window.changes.push(change),
					viewer: {
						resources: imageSize && {
							onResources(events) {
								for (const event of events)
									if (event.kind === "request") {
										const [width, height] = imageSize;
										event.request.resolve({
											width,
											height,
											rgba: new Uint8Array(
												width * height * 4,
											).fill(255),
										});
									}
							},
						},
						initialization: {
							wasmUrl: "/markview_web_bg.wasm",
							fonts,
						},
					},
				},
			);
			window.readEditor = () => {
				const view = window.editor.view;
				return view.posAtCoords(
					{
						x: view.contentDOM.getBoundingClientRect().left + 1,
						y:
							view.scrollDOM.getBoundingClientRect().top +
							view.documentPadding.top +
							1,
					},
					false,
				);
			};
		},
		{ markdown, options, fonts, imageSize },
	);
	await page.waitForFunction(
		() => !window.editor.viewer.reader.markview.stats().pending,
	);
}

function documentText() {
	return (
		"# Beginning\n\n" +
		"word ".repeat(1800) +
		"\n\n## Code\n\n```rust\n" +
		Array.from(
			{ length: 140 },
			(_, i) => `let line_${i} = \"中文😀 é\";`,
		).join("\n") +
		"\n```\n\n> ### Nested\n>\n> Quote content.\n\n" +
		"<details>\n<summary>Folded section</summary>\n\n## Hidden\n\nInside.\n\n</details>\n\n" +
		"## End\n\n" +
		"Ending paragraph.\n\n".repeat(120)
	);
}

for (const [name, image] of [
	[
		"wrapped Markdown",
		`![${"long image description ".repeat(30)}](tall.png)`,
	],
	["single-line Markdown", "![image](tall.png)"],
	[
		"multiline SVG",
		'<svg width="80" height="1200">\n' +
			'<rect width="80" height="1200" fill="red"/>\n'.repeat(35) +
			"</svg>",
	],
]) {
	test(`${name} image follows editor scrolling through wraps and adjacent blank lines`, async ({
		page,
	}) => {
		const before = "# Images\n\n" + "Before image.\n\n".repeat(35);
		const source = before + image + "\n\n" + "After image.\n\n".repeat(80);
		await host(page, source, {}, [80, 1200]);
		await page.waitForFunction(
			(start) =>
				window.editor.viewer.sourceToPreview(start)?.rect.height >=
				1200,
			before.length,
		);
		await page.evaluate((start) => {
			const view = window.editor.view;
			view.dispatch({
				effects: view.constructor.scrollIntoView(start, {
					y: "start",
					yMargin: view.documentPadding.top,
				}),
			});
		}, before.length);
		await expect
			.poll(() => page.evaluate(() => window.readEditor()))
			.toBe(before.length);
		const samples = await page.evaluate(
			async ({ start, end }) => {
				const view = window.editor.view;
				const height =
					view.lineBlockAt(end - 1).bottom -
					view.lineBlockAt(start).top;
				const steps = Math.ceil(
					(height + view.defaultLineHeight * 2) / 3,
				);
				const samples = [];
				for (let step = 0; step < steps; step++) {
					view.scrollDOM.scrollTop += 3;
					await new Promise((resolve) => setTimeout(resolve, 40));
					samples.push(window.editor.viewer.reader.markview.scroll());
				}
				return samples;
			},
			{ start: before.length, end: before.length + image.length },
		);
		for (let i = 1; i < samples.length; i++)
			expect(
				samples[i] - samples[i - 1],
				`step ${i}`,
			).toBeGreaterThanOrEqual(-2);
		expect(samples.at(-1) - samples[0]).toBeGreaterThan(900);
	});
}

test("preview image progress maps across wrapped source and survives editor takeover", async ({
	page,
}) => {
	const before = "# Images\n\n" + "Before image.\n\n".repeat(35);
	const image = `![${"long image description ".repeat(30)}](tall.png)`;
	await host(
		page,
		before + image + "\n\n" + "After image.\n\n".repeat(80),
		{},
		[80, 1200],
	);
	await page.waitForFunction(
		(start) =>
			window.editor.viewer.sourceToPreview(start)?.rect.height >= 1200,
		before.length,
	);
	await page.evaluate(
		(start) => window.editor.viewer.scrollToSource(start, 0.4),
		before.length,
	);
	await expect
		.poll(() =>
			page.evaluate(
				() => window.editor.viewer.readingPosition()?.fraction,
			),
		)
		.toBeCloseTo(0.4, 2);
	await page.locator("canvas").hover();
	await page.mouse.wheel(0, 1);
	await expect
		.poll(() => page.evaluate(() => window.readEditor()))
		.toBeGreaterThan(before.length + image.length * 0.3);
	const position = await page.evaluate(() => ({
		offset: window.readEditor(),
		scroll: window.editor.viewer.reader.markview.scroll(),
	}));
	expect(position.offset).toBeLessThan(before.length + image.length * 0.6);
	await page.locator(".cm-scroller").hover();
	await page.mouse.wheel(0, 3);
	await expect
		.poll(() =>
			page.evaluate(() => window.editor.viewer.readingPosition()?.offset),
		)
		.toBe(before.length);
	const followed = await page.evaluate(() =>
		window.editor.viewer.reader.markview.scroll(),
	);
	expect(Math.abs(followed - position.scroll)).toBeLessThan(100);
});

test("CodeMirror and preview follow source inside long paragraphs and code without focus changes", async ({
	page,
}) => {
	await host(page, documentText());
	await page.locator(".cm-content").click();
	await page.evaluate(() => {
		const view = window.editor.view;
		const rect = view.coordsAtPos(0);
		view.scrollDOM.scrollTop = 1100;
	});
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.readEditor() -
						window.editor.viewer.readingPosition().offset,
				),
			),
		)
		.toBeLessThan(180);
	const paragraph = await page.evaluate(() => ({
		editor: window.readEditor(),
		viewer: window.editor.viewer.readingPosition().offset,
		focus: window.editor.view.hasFocus,
	}));
	expect(paragraph.editor).toBeGreaterThan(2000);
	expect(paragraph.viewer).toBeGreaterThan(2000);
	expect(paragraph.focus).toBe(true);
	const codeOffset = await page.evaluate(() =>
		window.editor.getMarkdown().indexOf("let line_80"),
	);
	await page.evaluate((offset) => {
		window.editor.viewer.scrollToSource(offset);
	}, codeOffset);
	// A user gesture on the preview takes over from the editor's follow motion.
	await page.locator("canvas").hover();
	await page.mouse.wheel(0, 180);
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.readEditor() -
						window.editor.viewer.readingPosition().offset,
				),
			),
		)
		.toBeLessThan(100);
	const code = await page.evaluate(() => ({
		editor: window.readEditor(),
		viewer: window.editor.viewer.readingPosition().offset,
		focus: window.editor.view.hasFocus,
		selection: window.editor.view.state.selection.main.head,
	}));
	expect(code.viewer).toBeGreaterThan(codeOffset);
	expect(code.editor).toBeGreaterThan(codeOffset);
	expect(code.focus).toBe(true);
	// Wait for wheel easing before checking that both panes stay still.
	await expect
		.poll(async () => {
			const before = await page.evaluate(() =>
				window.editor.viewer.reader.markview.scroll(),
			);
			await page.waitForTimeout(100);
			const after = await page.evaluate(() =>
				window.editor.viewer.reader.markview.scroll(),
			);
			return Math.abs(after - before);
		})
		.toBeLessThan(1);
	const stable = await page.evaluate(() => ({
		editor: window.editor.view.scrollDOM.scrollTop,
		viewer: window.editor.viewer.reader.markview.scroll(),
	}));
	await page.waitForTimeout(350);
	const later = await page.evaluate(() => ({
		editor: window.editor.view.scrollDOM.scrollTop,
		viewer: window.editor.viewer.reader.markview.scroll(),
	}));
	expect(Math.abs(stable.editor - later.editor)).toBeLessThan(2);
	expect(Math.abs(stable.viewer - later.viewer)).toBeLessThan(2);
});

test("editing, history, indentation and Markdown continuation update the same versioned viewer", async ({
	page,
}) => {
	await host(page, "# 中文😀\r\n\r\n- item");
	expect(await page.evaluate(() => window.editor.getMarkdown())).toBe(
		"# 中文😀\n\n- item",
	);
	await page.locator(".cm-content").click();
	await page.keyboard.press("Control+End");
	await page.keyboard.press("Enter");
	await page.keyboard.type("next");
	await expect
		.poll(() => page.evaluate(() => window.editor.getMarkdown()))
		.toBe("# 中文😀\n\n- item\n- next");
	await page.keyboard.press("Tab");
	expect(await page.evaluate(() => window.editor.getMarkdown())).toContain(
		"  - next",
	);
	await page.keyboard.press("Control+z");
	await expect
		.poll(() => page.evaluate(() => window.editor.getMarkdown()))
		.not.toContain("  - next");
	await page.keyboard.press("Control+y");
	await expect
		.poll(() => page.evaluate(() => window.editor.getMarkdown()))
		.toContain("  - next");
	await page.evaluate(() =>
		window.editor.setMarkdown("# New heading\n\nUpdated"),
	);
	await expect(page.locator(".mv-toc button")).toHaveText("New heading");
	await page.waitForFunction(
		() => !window.editor.viewer.reader.markview.stats().pending,
	);
	const result = await page.evaluate(() => ({
		source: window.editor.getMarkdown(),
		preview: window.editor.viewer.getMarkdown(),
		last: window.changes.at(-1),
		toc: window.editor.viewer.outline(),
	}));
	expect(result.preview).toBe(result.source);
	expect(result.last.markdown).toBe(result.source);
	expect(result.last.documentVersion).toBe(result.toc.documentVersion);
});

test("TOC opens folded headings, divider and configuration keep component lifecycle isolated", async ({
	page,
}) => {
	await host(page, documentText());
	await page.locator("#outside").focus();
	await page.locator(".mv-toc button", { hasText: "Hidden" }).click();
	await expect
		.poll(() =>
			page.evaluate(() => window.editor.viewer.currentSection()?.anchor),
		)
		.toBe("hidden");
	await expect
		.poll(() =>
			page.evaluate(() =>
				Math.abs(
					window.readEditor() -
						window.editor.viewer.readingPosition().offset,
				),
			),
		)
		.toBeLessThan(100);
	const divider = await page.locator(".mv-divider").boundingBox();
	const before = await page.locator(".mv-write").boundingBox();
	await page.mouse.move(
		divider.x + divider.width / 2,
		divider.y + divider.height / 2,
	);
	await page.mouse.down();
	await page.mouse.move(divider.x + 140, divider.y + divider.height / 2, {
		steps: 5,
	});
	await page.mouse.up();
	const after = await page.locator(".mv-write").boundingBox();
	expect(after.width).toBeGreaterThan(before.width + 100);
	await page.evaluate(() =>
		window.editor.setOptions({
			theme: "dark",
			orientation: "vertical",
			toc: false,
		}),
	);
	await expect(page.locator(".markview-editor")).toHaveAttribute(
		"data-theme",
		"dark",
	);
	await expect(page.locator(".mv-toc")).toBeHidden();
	expect(
		(await page.locator(".mv-write").boundingBox()).width,
	).toBeGreaterThan(1000);
	await page.evaluate(async () => {
		const old = window.editor;
		old.destroy();
		old.destroy();
		window.editor = await window.api.Editor.mount(
			document.querySelector("#host"),
			{ markdown: "# Again" },
		);
	});
	await expect(page.locator(".cm-editor")).toHaveCount(1);
	await expect(page.locator("canvas")).toHaveCount(1);
	await expect(page.locator(".mv-toc button")).toHaveText("Again");
});

test("the built editor example paints source and preview and supports narrow layout", async ({
	page,
}) => {
	const errors = [];
	page.on("pageerror", (error) => errors.push(String(error)));
	await page.goto("/editor.html");
	await page.waitForFunction(
		() =>
			window.editor &&
			!window.editor.viewer.reader.markview.stats().pending,
	);
	const buffer = await page
		.locator("#desk")
		.screenshot({ path: test.info().outputPath("editor.png") });
	const png = decodePng(buffer);
	expect(inkPixels(png, { r: 249, g: 250, b: 252 })).toBeGreaterThan(10000);
	await page.setViewportSize({ width: 480, height: 800 });
	await expect
		.poll(() =>
			page
				.locator(".mv-split")
				.evaluate((el) => getComputedStyle(el).flexDirection),
		)
		.toBe("column");
	expect(errors).toEqual([]);
	await expect(page.locator("#error")).toBeHidden();
});

test("Chinese composition, extensions and multiple instances retain content and focus", async ({
	page,
}) => {
	await host(page, "# IME\n\n");
	await page.evaluate(async () => {
		const container = document.createElement("div");
		container.id = "second";
		container.style.cssText = "width:700px;height:400px";
		document.body.append(container);
		window.second = await window.api.Editor.mount(container, {
			markdown: "# Second\n\nUntouched",
		});
		window.editor.view.focus();
		window.editor.view.dispatch({
			selection: { anchor: window.editor.view.state.doc.length },
		});
	});
	const session = await page.context().newCDPSession(page);
	await session.send("Input.imeSetComposition", {
		text: "中",
		selectionStart: 1,
		selectionEnd: 1,
	});
	await session.send("Input.imeSetComposition", {
		text: "中文",
		selectionStart: 2,
		selectionEnd: 2,
	});
	expect(await page.evaluate(() => window.editor.view.hasFocus)).toBe(true);
	await session.send("Input.insertText", { text: "中文😀" });
	await expect
		.poll(() => page.evaluate(() => window.editor.getMarkdown()))
		.toBe("# IME\n\n中文😀");
	await expect
		.poll(() => page.evaluate(() => window.editor.viewer.getMarkdown()))
		.toBe("# IME\n\n中文😀");
	expect(await page.evaluate(() => window.second.getMarkdown())).toBe(
		"# Second\n\nUntouched",
	);
	await page.evaluate(() => {
		const View = window.editor.view.constructor;
		window.editor.setOptions({ extensions: View.editable.of(false) });
	});
	await expect(page.locator("#host .cm-content")).toHaveAttribute(
		"contenteditable",
		"false",
	);
	await page.evaluate(() => window.second.destroy());
	await expect(page.locator("#second canvas")).toHaveCount(0);
	await expect(page.locator("#host canvas")).toHaveCount(1);
});
