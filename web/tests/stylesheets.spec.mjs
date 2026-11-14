import { expect, test } from "@playwright/test";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { decodePng } from "./png.mjs";

const assets = readdirSync(
	fileURLToPath(new URL("../dist/assets/", import.meta.url)),
);
const font = `/assets/${assets.find((name) => name.startsWith("NotoSerif-Regular-subset-"))}`;
const monoFont = `/assets/${assets.find((name) => name.startsWith("NotoSansMono-Regular-subset-"))}`;
const compact =
	"format_version = 3\nversion = 1\n[[rule]]\nwhen = ['body']\nbackground = '#123456'\ncolor = '#ffffff'\nline_height = 1.3";
const spacious =
	"format_version = 3\nversion = 1\n[meta]\nname = ''\n[[rule]]\nwhen = ['body']\nbackground = '#654321'\ncolor = '#ffffff'\nline_height = 2.1";

async function host(page, sources = [font]) {
	await page.route("**/stylesheets.html", (route) =>
		route.fulfill({
			contentType: "text/html",
			body: '<div id="a" style="width:500px;height:300px"></div><div id="b" style="width:500px;height:300px"></div>',
		}),
	);
	await page.goto("/stylesheets.html");
	await page.evaluate(async (sources) => {
		const api = await import("/integration-api.js");
		const fonts = await api.loadFontSet(
			{ sources },
			{ wasmUrl: "/markview_web_bg.wasm" },
		);
		const markdown =
			"# Themes\n\n" +
			Array.from(
				{ length: 30 },
				(_, i) =>
					`Paragraph ${i}. ${"A line of reading text for theme comparisons. ".repeat(5)}\n\n`,
			).join("");
		for (const id of ["a", "b"]) {
			window[id] = await api.Viewer.mount(
				document.querySelector(`#${id}`),
				{ fonts, markdown },
			);
		}
		fonts.destroy();
	}, sources);
	await settled(page);
}

async function settled(page) {
	await page.waitForFunction(() =>
		[window.a, window.b].every(
			(viewer) => !viewer.reader.markview.stats().pending,
		),
	);
}

async function background(page, id = "a") {
	const png = decodePng(await page.locator(`#${id} canvas`).screenshot());
	const offset = (2 * png.width + 2) * png.channels;
	return Array.from(png.data.subarray(offset, offset + 3));
}

test("lazy registration supports repeated switching, source preservation and independent defaults", async ({
	page,
}) => {
	let requests = 0;
	await page.route("**/themes/compact.mvss.toml", (route) => {
		requests++;
		return route.fulfill({ body: compact });
	});
	await host(page);
	const original = await background(page);
	const registered = await page.evaluate(async () => {
		const before = window.a.reader.markview.stats();
		window.a.registerStylesheet(
			"compact",
			await (await fetch("/themes/compact.mvss.toml")).text(),
		);
		return { before, after: window.a.reader.markview.stats() };
	});
	expect(registered.after.revision).toBe(registered.before.revision);
	expect(registered.after.pending).toBe(false);
	expect(await background(page)).toEqual(original);
	const offset = await page.evaluate(() => {
		const offset = window.a.getMarkdown().indexOf("Paragraph 15.");
		window.a.scrollToSource(offset);
		return offset;
	});
	await expect
		.poll(() => page.evaluate(() => window.a.readingPosition()?.offset))
		.toBe(offset);
	for (const ids of [["compact"], ["bundled:dark"], ["compact"]]) {
		await page.evaluate((ids) => window.a.setStylesheets(ids), ids);
		await settled(page);
		await expect
			.poll(() => page.evaluate(() => window.a.readingPosition()?.offset))
			.toBe(offset);
	}
	expect(requests).toBe(1);
	expect(await background(page)).toEqual([0x12, 0x34, 0x56]);
	expect(await background(page, "b")).toEqual(original);
	const height = await page.evaluate(() =>
		window.a.reader.markview.contentHeight(),
	);
	await page.evaluate(() =>
		window.a.setOptions({ theme: "dark", fontSize: 24 }),
	);
	await settled(page);
	expect(await background(page)).toEqual([0x12, 0x34, 0x56]);
	expect(
		await page.evaluate(() => window.a.reader.markview.contentHeight()),
	).toBeGreaterThan(height);
	await page.evaluate(() => {
		window.a.setStylesheets([]);
		window.b.setStylesheets(["bundled:dark"]);
	});
	await settled(page);
	expect(await background(page)).toEqual(await background(page, "b"));
	expect(await background(page)).not.toEqual(original);
});

test("registration and selection are atomic, namespaced and owned by each engine", async ({
	page,
}) => {
	await host(page);
	await page.evaluate(
		({ compact, spacious }) => {
			window.a.registerStylesheet("light", compact);
			window.a.registerStylesheet("override", spacious);
			window.a.setStylesheets(["override", "light", "bundled:light"]);
		},
		{ compact, spacious },
	);
	await settled(page);
	expect(await background(page)).toEqual([0x65, 0x43, 0x21]);
	const errors = await page.evaluate(
		({ compact }) => {
			const attempts = [
				() => window.a.registerStylesheet("bundled:light", compact),
				() => window.a.registerStylesheet("bundled:custom", compact),
				() => window.a.registerStylesheet(" ", compact),
				() => window.a.registerStylesheet("override", "bad TOML"),
				() =>
					window.a.registerStylesheet(
						"override",
						"format_version = 3\nversion = 1\ntargets = ['pdf']",
					),
				() => window.a.setStylesheets(["missing", "bundled:dark"]),
				() => window.a.setStylesheets(["bundled:print"]),
				() => window.a.setStylesheets(["bundled:builtin"]),
				() => window.b.setStylesheets(["light"]),
			];
			return attempts.map((attempt) => {
				try {
					attempt();
				} catch (error) {
					return String(error);
				}
			});
		},
		{ compact },
	);
	expect(errors).toHaveLength(9);
	for (const error of errors) expect(error).toBeTruthy();
	expect(await background(page)).toEqual([0x65, 0x43, 0x21]);
	await page.evaluate((compact) => {
		window.a.registerStylesheet("override", compact);
		window.a.setOptions({ fontSize: 20 });
	}, compact);
	expect(await background(page)).toEqual([0x65, 0x43, 0x21]);
	await page.evaluate(() => window.a.setStylesheets(["override"]));
	await settled(page);
	expect(await background(page)).toEqual([0x12, 0x34, 0x56]);
	const pending = await page.evaluate((spacious) => {
		const engine = window.b.reader.markview;
		const update = engine.beginLayout("# Latest source\n\nText.");
		engine.registerStylesheet("override", spacious);
		const afterRegistration = update.stale;
		try {
			engine.setStylesheets(["unknown"]);
		} catch {}
		const afterFailure = update.stale;
		engine.setStylesheets(["override"]);
		update.finish();
		return {
			afterRegistration,
			afterFailure,
			afterSelection: update.stale,
			heading: engine.outline().entries[0].text,
		};
	}, spacious);
	expect(pending).toEqual({
		afterRegistration: false,
		afterFailure: false,
		afterSelection: true,
		heading: "Latest source",
	});
	await settled(page);
	expect(await background(page, "b")).toEqual([0x65, 0x43, 0x21]);
	const destroyed = await page.evaluate(() => {
		const engine = window.a.reader.markview;
		window.a.destroy();
		window.b.destroy();
		return [
			() => window.a.registerStylesheet("test", ""),
			() => engine.setStylesheets([]),
		].map((attempt) => {
			try {
				attempt();
			} catch (error) {
				return String(error);
			}
		});
	});
	for (const error of destroyed) expect(error).toContain("destroyed");
});

test("fontdef-only selections reshape cached blocks and clearing restores default fonts", async ({
	page,
}) => {
	await host(page, [font, monoFont]);
	await page.evaluate(() => {
		window.a.setMarkdown(
			Array.from(
				{ length: 20 },
				(_, i) =>
					`Paragraph ${i}. ${"Wide letters WWW and narrow iii shape differently in each face. ".repeat(8)}\n\n`,
			).join(""),
		);
		window.a.registerStylesheet(
			"mono",
			"format_version = 3\nversion = 1\n[[fontdef]]\nid = 'serif'\nlookfor = ['Noto Sans Mono']",
		);
		window.a.registerStylesheet(
			"color",
			"format_version = 3\nversion = 1\n[[rule]]\nwhen = ['body']\ncolor = '#123456'",
		);
	});
	await settled(page);
	const baseline = await page.evaluate(() =>
		window.a.reader.markview.stats(),
	);
	expect(baseline.blocks).toBe(20);
	await page.evaluate(() =>
		window.a.setStylesheets(["mono", "bundled:light"]),
	);
	await settled(page);
	const mono = await page.evaluate(() => window.a.reader.markview.stats());
	expect(mono.reused).toBe(0);
	expect(
		Math.abs(mono.contentHeight - baseline.contentHeight),
	).toBeGreaterThan(50);
	await page.evaluate(() =>
		window.a.setStylesheets(["color", "mono", "bundled:light"]),
	);
	await settled(page);
	const recolored = await page.evaluate(() =>
		window.a.reader.markview.stats(),
	);
	expect(recolored.contentHeight).toBe(mono.contentHeight);
	expect(recolored.reused).toBe(recolored.blocks);
	for (const ids of [["bundled:light"], ["mono", "bundled:light"], []]) {
		await page.evaluate((ids) => window.a.setStylesheets(ids), ids);
		await settled(page);
		const stats = await page.evaluate(() =>
			window.a.reader.markview.stats(),
		);
		expect(stats.reused).toBe(0);
		expect(stats.contentHeight).toBe(
			ids.includes("mono") ? mono.contentHeight : baseline.contentHeight,
		);
	}
});
