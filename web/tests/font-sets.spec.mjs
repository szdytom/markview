import { expect, test } from "@playwright/test";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const fixtures = fileURLToPath(
	new URL("../../crates/markview-web/tests/fonts/", import.meta.url),
);
const assetNames = readdirSync(
	fileURLToPath(new URL("../dist/assets/", import.meta.url)),
);
const face = (name) =>
	`/assets/${assetNames.find((file) => file.startsWith(name + "-subset-"))}`;
async function host(page) {
	await page.route("**/font-sets.html", (route) =>
		route.fulfill({
			contentType: "text/html",
			body: '<div id="a" style="width:500px;height:300px"></div><div id="b" style="width:500px;height:300px"></div>',
		}),
	);
	await page.route("**/fixture/*", (route) =>
		route.fulfill({
			body: readFileSync(
				`${fixtures}/${new URL(route.request().url()).pathname.split("/").pop()}`,
			),
		}),
	);
	await page.goto("/font-sets.html");
	await page.evaluate(async () => {
		window.api = await import("/integration-api.js");
		await window.api.init({ wasmUrl: "/markview_web_bg.wasm" });
	});
}

test("independent and shared font sets survive host release and budgeted font reflow", async ({
	page,
}) => {
	await host(page);
	await page.evaluate(
		async ({ serif, mono }) => {
			const { FontLoader, Viewer } = window.api;
			window.loader = new FontLoader();
			window.serif = await window.loader.load({ sources: [serif] });
			window.mono = await window.loader.load({ sources: [mono] });
			const markdown =
				"# Fonts\n\n" +
				Array.from(
					{ length: 50 },
					(_, i) =>
						`Paragraph ${i}. ${"Wide letters WWW and narrow iii shape differently in each face. ".repeat(12)}\n\n`,
				).join("");
			window.a = await Viewer.mount(document.querySelector("#a"), {
				fonts: window.serif,
				markdown,
			});
			window.b = await Viewer.mount(document.querySelector("#b"), {
				fonts: window.mono,
				markdown,
			});
		},
		{
			serif: face("NotoSerif-Regular"),
			mono: face("NotoSansMono-Regular"),
		},
	);
	await page.waitForFunction(
		() =>
			!window.a.reader.markview.stats().pending &&
			!window.b.reader.markview.stats().pending,
	);
	const before = await page.evaluate(() => {
		const offset = window.a.getMarkdown().indexOf("Paragraph 25");
		window.a.scrollToSource(offset);
		return {
			a: window.a.sourceToPreview(offset).rect.y,
			b: window.b.sourceToPreview(offset).rect.y,
			offset,
		};
	});
	expect(Math.abs(before.a - before.b)).toBeGreaterThan(100);
	await expect
		.poll(() => page.evaluate(() => window.a.readingPosition()?.offset))
		.toBe(before.offset);
	await page.evaluate(() => {
		window.a.setFonts(window.mono);
		window.mono.destroy();
		window.mono.destroy();
	});
	await page.waitForFunction(() => !window.a.reader.markview.stats().pending);
	const after = await page.evaluate(() => {
		let error;
		try {
			window.b.setFonts(window.mono);
		} catch (e) {
			error = String(e);
		}
		return {
			offset: window.a.readingPosition().offset,
			a: window.a.sourceToPreview(window.a.readingPosition().offset).rect
				.y,
			b: window.b.sourceToPreview(window.a.readingPosition().offset).rect
				.y,
			error,
			painted: window.a.reader.markview.frame().glyphs,
		};
	});
	expect(after.offset).toBe(before.offset);
	expect(after.a).toBe(after.b);
	expect(after.error).toContain("FontSet has been destroyed");
	expect(after.painted).toBeGreaterThan(0);
});

test("official WASM decodes CFF and TrueType WOFF formats and raw collections", async ({
	page,
}) => {
	await host(page);
	const result = await page.evaluate(async () => {
		const { FontSet, Markview } = window.api;
		const results = [];
		const canvas = document.createElement("canvas");
		document.body.append(canvas);
		for (const name of [
			"NotoSerif-Regular-subset.woff",
			"NotoSerif-Regular-subset.woff2",
			"NotoColorEmoji-subset.woff",
			"NotoColorEmoji-subset.woff2",
			"Noto-subset.ttc",
		]) {
			const data = new Uint8Array(
				await (await fetch(`/fixture/${name}`)).arrayBuffer(),
			);
			const padded = new Uint8Array(data.length + 10);
			padded.set(data, 5);
			const fonts = await FontSet.create([
				padded.subarray(5, 5 + data.length),
			]);
			const mv = await Markview.create(
				canvas,
				undefined,
				undefined,
				fonts,
			);
			mv.resize(500, 300, 1);
			mv.setMarkdown(
				name.includes("Emoji") ? "😀" : "Typography and $x^2$.",
			);
			results.push({ name, glyphs: mv.frame().glyphs });
			mv.destroy();
			fonts.destroy();
		}
		const errors = [];
		for (const value of ["404", "wOFF", "wOF2"]) {
			try {
				await FontSet.create([new TextEncoder().encode(value)]);
			} catch (e) {
				errors.push(String(e));
			}
		}
		canvas.remove();
		return { results, errors };
	});
	for (const item of result.results)
		expect(item.glyphs, item.name).toBeGreaterThan(0);
	expect(result.errors).toHaveLength(3);
	for (const error of result.errors)
		expect(error).toContain("invalid host font at index 0");
	expect(result.errors[1]).toContain("WOFF decoding failed");
});

test("explicit CDN descriptors and request policy cache, retry and host ownership", async ({
	page,
}) => {
	await host(page);
	const requests = [];
	let attempts = 0;
	await page.route("**/cdn/body.woff2", (route) => {
		requests.push(route.request().headers()["x-font-policy"]);
		return ++attempts === 1
			? route.fulfill({ status: 503, body: "retry" })
			: route.fulfill({
					body: readFileSync(
						`${fixtures}/NotoSerif-Regular-subset.woff2`,
					),
				});
	});
	const result = await page.evaluate(async () => {
		const { FontLoader, fontFiles } = window.api;
		const loader = new FontLoader({
			requestInit: { headers: { "X-Font-Policy": "explicit" } },
		});
		const description = fontFiles(new URL("/cdn/", location.href), [
			"body.woff2",
		]);
		let error;
		try {
			await loader.load(description);
		} catch (e) {
			error = String(e);
		}
		const [a, b] = await Promise.all([
			loader.load(description),
			loader.load(description),
		]);
		loader.clear();
		const alive = !a.destroyed;
		const c = await loader.load(description);
		c.destroy();
		const d = await loader.load(description);
		const recreated = c !== d && !d.destroyed;
		a.destroy();
		d.destroy();
		return { error, same: a === b, alive, recreated };
	});
	expect(result.error).toContain("HTTP 503");
	expect(result.same).toBe(true);
	expect(result.alive).toBe(true);
	expect(result.recreated).toBe(true);
	expect(requests).toEqual(["explicit", "explicit", "explicit"]);
});

test("custom fetch exceptions and invalid downloaded faces can retry", async ({
	page,
}) => {
	await host(page);
	const result = await page.evaluate(async (url) => {
		const { FontLoader } = window.api;
		let attempts = 0;
		const errors = [];
		const loader = new FontLoader({
			fetch: (...args) => {
				attempts++;
				if (attempts === 1) throw new Error("custom transport failed");
				if (attempts === 2)
					return Promise.resolve(new Response("404 font"));
				return fetch(...args);
			},
		});
		for (let i = 0; i < 2; i++) {
			try {
				await loader.load({ sources: [url] });
			} catch (e) {
				errors.push(String(e));
			}
		}
		const set = await loader.load({ sources: [url] });
		set.destroy();
		return { attempts, errors };
	}, face("NotoSerif-Regular"));
	expect(result.attempts).toBe(3);
	expect(result.errors[0]).toContain("custom transport failed");
	expect(result.errors[1]).toContain("invalid host font at index 0");
});
