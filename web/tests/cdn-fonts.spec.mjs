import { expect, test } from "@playwright/test";
import { cdnFonts, mockCdnFonts } from "./fixtures/cdn-fonts.mjs";

test.setTimeout(120_000);

test("SPA waits for pinned CDN fonts before mounting and reuses them across modes", async ({
	page,
}) => {
	await mockCdnFonts(page);
	const requests = [];
	page.on("request", (request) => {
		if (
			request.url().startsWith("https://cdn.jsdelivr.net/") &&
			request.resourceType() === "fetch"
		) {
			requests.push(request.url());
		}
	});
	let release;
	const gate = new Promise((resolve) => {
		release = resolve;
	});
	await page.route(cdnFonts, async (route) => {
		if (route.request().url().endsWith("/NotoSerif-Regular.ttf"))
			await gate;
		await route.fallback();
	});
	const request = page.waitForRequest(
		(request) =>
			request.url().endsWith("/NotoSerif-Regular.ttf") &&
			request.resourceType() === "fetch",
	);
	try {
		await page.goto("/index.html", { waitUntil: "domcontentloaded" });
		await request;
		await expect(page.locator("body")).not.toHaveAttribute(
			"data-ready",
			"true",
		);
		await expect(page.locator("canvas")).toHaveCount(0);
		await expect(page.locator("#loading-text")).toContainText(
			"Downloading fonts",
		);
	} finally {
		release();
	}
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
	expect(requests).toHaveLength(16);
	expect(new Set(requests).size).toBe(16);
	for (const url of requests) {
		expect(url).toMatch(
			/@(ffebf8c1ee449e544955a7e813c54f9b73848eac|Serif2\.003|Sans2\.004|v2\.051)\//,
		);
		expect(url).not.toContain("-subset");
	}
	await page.getByRole("link", { name: "Edit", exact: true }).click();
	await page.getByRole("link", { name: "Read", exact: true }).click();
	expect(requests).toHaveLength(16);
});

for (const knownSize of [true, false]) {
	test(`startup reports streamed font progress ${knownSize ? "with" : "without"} a total size`, async ({
		page,
	}) => {
		await mockCdnFonts(page);
		await page.addInitScript((knownSize) => {
			const originalFetch = window.fetch;
			window.fetch = async (input, options) => {
				const response = await originalFetch(input, options);
				if (!String(input).endsWith("/NotoSerif-Regular.ttf"))
					return response;
				const bytes = new Uint8Array(await response.arrayBuffer());
				const split = Math.ceil(bytes.length * 0.35);
				return new Response(
					new ReadableStream({
						start(controller) {
							controller.enqueue(bytes.slice(0, split));
							window.releaseFontDownload = () => {
								delete window.releaseFontDownload;
								controller.enqueue(bytes.slice(split));
								controller.close();
							};
						},
					}),
					{
						headers: knownSize
							? { "content-length": String(bytes.length) }
							: {},
					},
				);
			};
		}, knownSize);
		let releaseWasm;
		const wasmGate = new Promise((resolve) => {
			releaseWasm = resolve;
		});
		await page.route("**/markview_web_bg.wasm", async (route) => {
			await wasmGate;
			await route.continue();
		});
		try {
			await page.goto("/index.html", { waitUntil: "domcontentloaded" });
			await expect(page.locator("#loading-text")).toHaveText(
				"Downloading fonts · 15/16 complete",
			);
			await expect(page.locator("#engine-text")).toHaveText(
				"Downloading fonts · 15/16 complete",
			);
			await expect(page.locator("#loading-detail")).toHaveText(
				knownSize
					? /NotoSerif-Regular · 35% received · \d+ KB \/ \d+ KB/
					: /NotoSerif-Regular · \d+ KB received/,
			);
			await expect(page.locator("canvas")).toHaveCount(0);
			await page.evaluate(() => window.releaseFontDownload());
			await expect(page.locator("#loading-text")).toHaveText(
				"Preparing fonts and renderer…",
			);
			await expect(page.locator("#loading-detail")).toContainText(
				"All fonts downloaded",
			);
		} finally {
			releaseWasm();
			await page.evaluate(() => window.releaseFontDownload?.());
		}
		await expect(page.locator("body")).toHaveAttribute(
			"data-ready",
			"true",
			{
				timeout: 90_000,
			},
		);
		await expect(page.locator(".loading")).toHaveCount(0);
		await expect(page.locator("#engine-text")).toHaveText("Ready to read");
	});
}
