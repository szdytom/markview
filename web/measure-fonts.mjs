// Run after building both feature variants; timings are diagnostic, not a gate.
import { chromium } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
const fonts = {
	otf: readFileSync(
		`${root}/crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf`,
	),
	woff: readFileSync(
		`${root}/crates/markview-web/tests/fonts/NotoSerif-Regular-subset.woff`,
	),
	woff2: readFileSync(
		`${root}/crates/markview-web/tests/fonts/NotoSerif-Regular-subset.woff2`,
	),
};
const results = {};
const browser = await chromium.launch();
try {
	for (const variant of ["disabled", "enabled"]) {
		const samples = [];
		for (let i = 0; i < 7; i++) {
			const page = await browser.newPage();
			await page.route("**/*", (route) => {
				const path = new URL(route.request().url()).pathname;
				if (path === "/")
					return route.fulfill({
						contentType: "text/html",
						body: "<body>",
					});
				if (path === "/glue.js")
					return route.fulfill({
						contentType: "text/javascript",
						body: readFileSync(
							`${root}/web/packages/markview/wasm/markview_web.js`,
						),
					});
				if (path === "/viewer.wasm")
					return route.fulfill({
						contentType: "application/wasm",
						body: readFileSync(
							`${root}/artifacts/mvaac-woff/${variant}.wasm`,
						),
					});
				if (path.slice(1) in fonts)
					return route.fulfill({ body: fonts[path.slice(1)] });
				return route.abort();
			});
			await page.goto("http://font-measure.test/");
			samples.push(
				await page.evaluate(async (variant) => {
					const glue = await import("/glue.js");
					const bytes = await (
						await fetch("/viewer.wasm")
					).arrayBuffer();
					const t0 = performance.now();
					const module = await WebAssembly.compile(bytes);
					const t1 = performance.now();
					await glue.default({ module_or_path: module });
					const t2 = performance.now();
					const faces = {};
					for (const format of ["otf", "woff", "woff2"]) {
						const face = new Uint8Array(
							await (await fetch(`/${format}`)).arrayBuffer(),
						);
						const times = [];
						let error;
						for (let j = 0; j < 10; j++) {
							const start = performance.now();
							try {
								new glue.FontSet([face]).free();
							} catch (e) {
								error = String(e);
								break;
							}
							times.push(performance.now() - start);
						}
						if (
							variant === "disabled" &&
							format !== "otf" &&
							!error?.includes("`woff` feature")
						)
							throw new Error(
								"disabled decoder did not reject WOFF",
							);
						faces[format] = { times, error };
					}
					return {
						compileMs: t1 - t0,
						instantiateMs: t2 - t1,
						faces,
					};
				}, variant),
			);
			await page.close();
		}
		results[variant] = samples;
	}
	writeFileSync(
		`${root}/artifacts/mvaac-woff/timings.json`,
		JSON.stringify(results, null, 2),
	);
	const median = (values) =>
		values.toSorted((a, b) => a - b)[Math.floor(values.length / 2)];
	for (const [variant, samples] of Object.entries(results))
		console.log(
			variant,
			JSON.stringify({
				compileMs: median(samples.map((s) => s.compileMs)),
				instantiateMs: median(samples.map((s) => s.instantiateMs)),
				faces: Object.fromEntries(
					Object.keys(fonts).map((format) => [
						format,
						median(samples.flatMap((s) => s.faces[format].times)),
					]),
				),
			}),
		);
} finally {
	await browser.close();
}
