import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

test.setTimeout(120_000);
const fonts = readdirSync(
	fileURLToPath(new URL("../dist/assets/", import.meta.url)),
)
	.filter((name) => /\.(otf|ttf)$/.test(name))
	.map((name) => `/assets/${name}`);

async function host(
	page,
	markdown,
	options = {},
	imageSize,
	measureScroll = false,
) {
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
		async ({ markdown, options, fonts, imageSize, measureScroll }) => {
			window.api = await import(
				measureScroll ? "/integration-api.js" : "/editor-api.js"
			);
			if (measureScroll) {
				window.scrollWork = {
					queries: 0,
					anchors: 0,
					maxBatchMs: 0,
					maxReadMs: 0,
				};
				for (const name of ["sourceToPreview", "scrollAnchors"]) {
					const original = window.api.Viewer.prototype[name];
					window.api.Viewer.prototype[name] = function (...args) {
						const begin = performance.now();
						const result = original.apply(this, args);
						if (name === "sourceToPreview")
							window.scrollWork.queries++;
						else {
							window.scrollWork.anchors += result.anchors.length;
							window.scrollWork.maxBatchMs = Math.max(
								window.scrollWork.maxBatchMs,
								performance.now() - begin,
							);
						}
						return result;
					};
				}
			}
			window.changes = [];
			window.editor = await window.api.Editor.mount(
				document.querySelector("#host"),
				{
					...options,
					markdown,
					onChange: (change) => window.changes.push(change),
					viewer: {
						...options.viewer,
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
			if (measureScroll) {
				const measure = window.editor.view.requestMeasure.bind(
					window.editor.view,
				);
				window.editor.view.requestMeasure = (request) =>
					measure(
						request?.key === window.editor
							? {
									...request,
									read(view) {
										const begin = performance.now();
										const result = request.read(view);
										window.scrollWork.maxReadMs = Math.max(
											window.scrollWork.maxReadMs,
											performance.now() - begin,
										);
										return result;
									},
								}
							: request,
					);
			}
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
		{ markdown, options, fonts, imageSize, measureScroll },
	);
	await page.waitForFunction(
		() => !window.editor.viewer.reader.markview.stats().pending,
	);
}

test("large document loading, edits and reflow avoid long scroll-map reads", async ({
	page,
}) => {
	await host(
		page,
		"# Large 中文😀\n\n" +
			"Short paragraph with words and 中文😀.\n\n".repeat(2000),
		{ viewer: { stepBudgetMs: 1 } },
		undefined,
		true,
	);
	for (const phase of ["loading", "editing", "reflow"]) {
		if (phase !== "loading") {
			await page.evaluate((phase) => {
				for (const key of Object.keys(window.scrollWork))
					window.scrollWork[key] = 0;
				if (phase === "editing")
					window.editor.view.dispatch({
						changes: { from: 0, insert: "Inserted before.\n\n" },
					});
				else window.editor.setOptions({ split: 0.65 });
			}, phase);
		}
		await page.waitForFunction(
			() => !window.editor.viewer.reader.markview.stats().pending,
		);
		await expect
			.poll(() => page.evaluate(() => window.scrollWork.anchors))
			.toBeGreaterThan(2000);
		const work = await page.evaluate(() => window.scrollWork);
		console.log(`[scroll ${phase}] ${JSON.stringify(work)}`);
		expect(work.queries, phase).toBeLessThan(100);
		expect(work.anchors, phase).toBeLessThan(2200);
		expect(work.maxBatchMs, phase).toBeLessThan(100);
		expect(work.maxReadMs, phase).toBeLessThan(100);
	}
	await page.evaluate(() => {
		const scroller = window.editor.view.scrollDOM;
		scroller.scrollTop = scroller.scrollHeight;
	});
	await expect
		.poll(() =>
			page.evaluate(() => {
				const engine = window.editor.viewer.reader.markview;
				return engine.maxScroll() - engine.scroll();
			}),
		)
		.toBeLessThan(2);
});

test("early-defined footnotes preserve main prose scroll alignment in both directions", async ({
	page,
}) => {
	const markdown =
		"# Footnotes[^long]\n\n[^long]: " +
		"Lengthy footnote content with 中文😀. ".repeat(400) +
		"\n\n" +
		Array.from(
			{ length: 100 },
			(_, i) => `Paragraph ${i}: Main prose with some words.\n\n`,
		).join("");
	await host(page, markdown, { viewer: { stepBudgetMs: 1 } });
	const note = await page.evaluate(() => {
		const { viewer } = window.editor;
		return {
			top: viewer.sourceToPreview(
				window.editor.getMarkdown().indexOf("Lengthy"),
			).rect.y,
			max: viewer.reader.markview.maxScroll(),
		};
	});
	expect(note.top).toBeLessThan(note.max);
	for (const paragraph of [25, 50, 75]) {
		const start = markdown.indexOf(`Paragraph ${paragraph}:`);
		await page.evaluate((start) => {
			const view = window.editor.view;
			view.scrollDOM.dispatchEvent(
				new WheelEvent("wheel", { deltaY: 0, bubbles: true }),
			);
			view.dispatch({
				effects: view.constructor.scrollIntoView(start, {
					y: "start",
					yMargin: view.documentPadding.top,
				}),
			});
		}, start);
		await expect
			.poll(() => page.evaluate(() => window.readEditor()))
			.toBe(start);
		await expect
			.poll(() =>
				page.evaluate((start) => {
					const { viewer } = window.editor;
					return Math.abs(
						viewer.reader.markview.scroll() -
							viewer.sourceToPreview(start).rect.y,
					);
				}, start),
			)
			.toBeLessThan(5);
	}
	const start = markdown.indexOf("Paragraph 50:");
	await page.evaluate((start) => {
		const { viewer } = window.editor;
		viewer.canvas.dispatchEvent(
			new WheelEvent("wheel", { deltaY: 0, bubbles: true }),
		);
		viewer.scrollToSource(start);
	}, start);
	await expect
		.poll(() => page.evaluate(() => window.readEditor()))
		.toBe(start);
});

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

test("both panes scroll continuously across prose, blank lines, code and images", async ({
	page,
}) => {
	const markdown =
		"# Scroll\n\n" +
		"A long paragraph with **formatting** and 中文😀. ".repeat(90) +
		"\n\n\n\n```rust\n" +
		Array.from({ length: 45 }, (_, i) => `let value_${i} = ${i};`).join(
			"\n",
		) +
		"\n```\n\n\n\n![image](tall.png)\n\n\n\n" +
		"Ending paragraph.\n\n".repeat(70);
	await host(page, markdown, {}, [80, 1200]);
	await page.waitForFunction(
		() =>
			window.editor.viewer.sourceToPreview(
				window.editor.getMarkdown().indexOf("![image]"),
			)?.rect.height >= 1200,
	);
	for (const origin of ["source", "preview"]) {
		const samples = await page.evaluate(async (origin) => {
			const { view, viewer } = window.editor;
			const engine = viewer.reader.markview;
			const scroller = view.scrollDOM;
			const input = origin === "source" ? scroller : viewer.canvas;
			input.dispatchEvent(
				new WheelEvent("wheel", { deltaY: 0, bubbles: true }),
			);
			const maximum =
				origin === "source"
					? scroller.scrollHeight - scroller.clientHeight
					: engine.maxScroll();
			const samples = [];
			for (const direction of [1, -1]) {
				for (let step = 0; step <= 100; step++) {
					const target =
						maximum *
						(direction === 1 ? step / 100 : 1 - step / 100);
					if (origin === "source") scroller.scrollTop = target;
					else viewer.scrollTo(target);
					await new Promise((resolve) => setTimeout(resolve, 35));
					samples.push({
						direction,
						source: scroller.scrollTop,
						preview: engine.scroll(),
					});
				}
			}
			return samples;
		}, origin);
		for (let i = 1; i < samples.length; i++) {
			if (samples[i].direction !== samples[i - 1].direction) continue;
			for (const pane of ["source", "preview"])
				expect(
					(samples[i][pane] - samples[i - 1][pane]) *
						samples[i].direction,
					`${origin} drives ${pane}, step ${i}`,
				).toBeGreaterThanOrEqual(-2);
		}
		expect(samples[100].source).toBeGreaterThan(2000);
		expect(samples[100].preview).toBeGreaterThan(3000);
		expect(samples.at(-1).source).toBeLessThan(2);
		expect(samples.at(-1).preview).toBeLessThan(2);
	}
});

test("scroll endpoints, rapid gesture takeover and resizing preserve stable panes", async ({
	page,
}) => {
	await host(page, documentText());
	await page.locator(".cm-content").click();
	await page.keyboard.press("Control+End");
	await page.evaluate(() => {
		const scroller = window.editor.view.scrollDOM;
		scroller.scrollTop = scroller.scrollHeight;
	});
	await expect
		.poll(() =>
			page.evaluate(() => {
				const engine = window.editor.viewer.reader.markview;
				return engine.maxScroll() - engine.scroll();
			}),
		)
		.toBeLessThan(2);
	await page.locator("canvas").focus();
	await page.keyboard.press("Home");
	await expect
		.poll(() => page.evaluate(() => window.editor.view.scrollDOM.scrollTop))
		.toBeLessThan(2);
	await page.keyboard.press("End");
	await expect
		.poll(() =>
			page.evaluate(() => {
				const scroller = window.editor.view.scrollDOM;
				return (
					scroller.scrollHeight -
					scroller.clientHeight -
					scroller.scrollTop
				);
			}),
		)
		.toBeLessThan(2);
	await page.keyboard.press("Home");
	await expect
		.poll(() => page.evaluate(() => window.editor.view.scrollDOM.scrollTop))
		.toBeLessThan(2);
	await page.mouse.wheel(0, 850);
	await page.waitForTimeout(50);
	await page.locator(".cm-scroller").hover();
	await page.mouse.wheel(0, 250);
	await page.waitForTimeout(50);
	await page.locator("canvas").hover();
	await page.mouse.wheel(0, -80);
	await page.waitForTimeout(800);
	const read = () =>
		page.evaluate(() => ({
			source: window.editor.view.scrollDOM.scrollTop,
			preview: window.editor.viewer.reader.markview.scroll(),
			selection: window.editor.view.state.selection.main.head,
		}));
	const before = await read();
	await page.waitForTimeout(300);
	expect(await read()).toEqual(before);
	await page.evaluate(() => window.editor.setOptions({ split: 0.65 }));
	await page.waitForTimeout(800);
	const resized = await read();
	await page.waitForTimeout(300);
	expect(await read()).toEqual(resized);
	expect(resized.selection).toBe(before.selection);
});

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

test("divider grip stays centered across layouts and host styles", async ({
	page,
}) => {
	await host(page, "# Divider", { toc: false });
	for (const demo of [false, true]) {
		if (demo) {
			await page.addStyleTag({ url: "/main.css" });
			await page
				.locator("#host")
				.evaluate((host) => host.classList.add("workspace"));
		}
		for (const boxSizing of ["content-box", "border-box"]) {
			for (const [orientation, width] of [
				["horizontal", 1180],
				["vertical", 1180],
				["auto", 500],
			]) {
				await page.evaluate(
					({ orientation, width, boxSizing }) => {
						document.querySelector("#host").style.width =
							`${width}px`;
						document.querySelector(".mv-divider").style.boxSizing =
							boxSizing;
						window.editor.setOptions({ orientation });
					},
					{ orientation, width, boxSizing },
				);
				await expect
					.poll(() =>
						page.locator(".mv-divider").evaluate((divider) => {
							const rect = divider.getBoundingClientRect();
							const style = getComputedStyle(divider);
							const grip = getComputedStyle(divider, "::after");
							const transform = new DOMMatrix(grip.transform);
							const size = (axis, start, end) =>
								parseFloat(grip[axis]) +
								(grip.boxSizing === "border-box"
									? 0
									: parseFloat(grip[start]) +
										parseFloat(grip[end]));
							return Math.max(
								Math.abs(
									parseFloat(style.borderLeftWidth) +
										parseFloat(grip.left) +
										transform.e +
										size(
											"width",
											"borderLeftWidth",
											"borderRightWidth",
										) /
											2 -
										rect.width / 2,
								),
								Math.abs(
									parseFloat(style.borderTopWidth) +
										parseFloat(grip.top) +
										transform.f +
										size(
											"height",
											"borderTopWidth",
											"borderBottomWidth",
										) /
											2 -
										rect.height / 2,
								),
							);
						}),
					)
					.toBeLessThan(0.1);
			}
		}
	}
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
