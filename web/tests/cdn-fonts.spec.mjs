import { expect, test } from "@playwright/test";
import { cdnFonts, mockCdnFonts } from "./fixtures/cdn-fonts.mjs";

test.setTimeout(120_000);

test("SPA waits for pinned CDN fonts before mounting and reuses them across modes", async ({
	page,
}) => {
	await mockCdnFonts(page);
	const requests = [];
	page.on("request", (request) => {
		if (cdnFonts.test(request.url())) {
			requests.push(request.url());
		}
	});
	let release;
	const gate = new Promise((resolve) => {
		release = resolve;
	});
	await page.route(cdnFonts, async (route) => {
		if (
			route
				.request()
				.url()
				.endsWith("/noto-serif-latin-wght-normal.woff2")
		)
			await gate;
		await route.fallback();
	});
	const request = page.waitForRequest(
		(request) =>
			request.url().endsWith("/noto-serif-latin-wght-normal.woff2") &&
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
			"Loading fonts",
		);
	} finally {
		release();
	}
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
	expect(requests).toHaveLength(15);
	expect(
		await page.evaluate(() =>
			Array.from(document.fonts, ({ family, status }) => ({
				family,
				status,
			})),
		),
	).toContainEqual({ family: "Reading serif", status: "loaded" });
	expect(new Set(requests).size).toBe(15);
	expect(requests.filter((url) => url.endsWith(".woff2"))).toHaveLength(12);
	for (const url of requests) {
		expect(url).toMatch(/(?:@|\/)(4\.5\.12|5\.3\.0|Sans2\.004|v2\.051)\//);
		expect(url).not.toContain("-subset");
	}
	await page.getByRole("link", { name: "Edit", exact: true }).click();
	await page.getByRole("link", { name: "Read", exact: true }).click();
	expect(requests).toHaveLength(15);
	const cachedUrls = await page.evaluate(async () => {
		const cache = await caches.open("markview-demo-fonts-v1");
		return (await cache.keys()).map((request) => request.url);
	});
	expect(cachedUrls.sort()).toEqual([...requests].sort());
	// A reload must consume persisted bytes even when the CDN is unavailable.
	await page.route(cdnFonts, (route) => route.abort());
	await page.reload();
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
	expect(requests).toHaveLength(15);
	await page.getByRole("link", { name: "Edit", exact: true }).click();
	await expect(
		page.getByRole("textbox", { name: "Markdown source" }),
	).toBeVisible();
});

test("a failed download is retried while completed fonts stay cached", async ({
	page,
}) => {
	await mockCdnFonts(page);
	const failed = "**/noto-serif-latin-wght-normal.woff2";
	const fail = (route) => route.fulfill({ status: 503, body: "Unavailable" });
	await page.route(failed, fail);
	await page.goto("/index.html");
	await expect(page.getByRole("alert")).toContainText("could not start");
	await expect
		.poll(() =>
			page.evaluate(async () => {
				const cache = await caches.open("markview-demo-fonts-v1");
				return (await cache.keys()).length;
			}),
		)
		.toBe(14);
	await page.unroute(failed, fail);
	const requests = [];
	page.on("request", (request) => {
		if (cdnFonts.test(request.url()) && request.resourceType() === "fetch")
			requests.push(request.url());
	});
	await page.getByRole("button", { name: "Try again" }).click();
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
	expect(requests).toHaveLength(1);
	expect(requests[0]).toContain("noto-serif-latin-wght-normal.woff2");
});

for (const failure of ["http", "network"]) {
	test(`font mirror ${failure} failures use the CDN and cache the recovered bytes`, async ({
		page,
	}) => {
		await mockCdnFonts(page);
		const fallbackRequests = [];
		page.on("request", (request) => {
			if (request.url().includes("cdn.jsdelivr.net/npm/"))
				fallbackRequests.push(request.url());
		});
		await page.route("https://registry.npmmirror.com/**", (route) =>
			failure === "http"
				? route.fulfill({ status: 503, body: "Unavailable" })
				: route.abort(),
		);
		await page.goto("/index.html");
		await expect(page.locator("body")).toHaveAttribute(
			"data-ready",
			"true",
		);
		expect(fallbackRequests).toHaveLength(12);
		const cached = await page.evaluate(async () => {
			const cache = await caches.open("markview-demo-fonts-v1");
			return (await cache.keys()).map((request) => request.url);
		});
		expect(cached).toHaveLength(15);
		expect(
			cached.filter((url) =>
				url.startsWith("https://registry.npmmirror.com/"),
			),
		).toHaveLength(12);
		await page.route(cdnFonts, (route) => route.abort());
		await page.reload();
		await expect(page.locator("body")).toHaveAttribute(
			"data-ready",
			"true",
		);
		expect(fallbackRequests).toHaveLength(12);
	});
}

test("demo summaries and bold italics retain the serif face and ordinary spaces", async ({
	page,
}) => {
	await mockCdnFonts(page);
	await page.goto("/index.html");
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
	const measurements = await page.evaluate(async () => {
		const { FontSet, Markview, init } = await import("/integration-api.js");
		await init({ wasmUrl: "/markview_web_bg.wasm" });
		const cache = await caches.open("markview-demo-fonts-v1");
		const requests = await cache.keys();
		const faces = await Promise.all(
			requests.map(async (request) => ({
				url: request.url,
				bytes: await (await cache.match(request)).arrayBuffer(),
			})),
		);
		const summary = "There is more to a page than its first impression";
		const italic = "Bold italic";
		const markdown = `<details>\n<summary>${summary}</summary>\n\nHidden.\n\n</details>\n\n***${italic}***`;
		const canvas = document.createElement("canvas");
		document.body.append(canvas);
		const measure = async (faces) => {
			const fonts = await FontSet.create(faces.map((face) => face.bytes));
			const mv = await Markview.create(
				canvas,
				{ width: 760, fontSize: 18 },
				undefined,
				fonts,
			);
			mv.resize(900, 400, 1);
			mv.setMarkdown(markdown);
			mv.frame();
			const widths = (text) =>
				Array.from(
					text,
					(_, i) =>
						mv.sourceToPreview(markdown.indexOf(text) + i).rect
							.width,
				);
			const result = { summary: widths(summary), italic: widths(italic) };
			mv.destroy();
			fonts.destroy();
			return result;
		};
		const demo = await measure(faces);
		const serif = await measure(
			faces.filter((face) =>
				face.url.includes("/noto-serif-latin-wght-"),
			),
		);
		canvas.remove();
		return { demo, serif, space: demo.summary[summary.indexOf(" ")] };
	});
	expect(measurements.space).toBeCloseTo(4.68, 2);
	expect(measurements.demo.summary[0]).toBeCloseTo(11.484, 2);
	expect(measurements.demo).toEqual(measurements.serif);
});

for (const cached of [false, true]) {
	test(`malformed HTTP 200 font bytes ${cached ? "already cached" : "from the CDN"} can recover on retry`, async ({
		page,
	}) => {
		await mockCdnFonts(page);
		const requests = [];
		page.on("request", (request) => {
			if (cdnFonts.test(request.url())) requests.push(request.url());
		});
		const malformedUrl = "**/noto-serif-latin-wght-normal.woff2";
		const malformed = (route) =>
			route.fulfill({
				status: 200,
				contentType: "font/woff2",
				body: "not a font",
			});
		if (cached) {
			await page.goto("/index.html");
			await expect(page.locator("body")).toHaveAttribute(
				"data-ready",
				"true",
				{ timeout: 90_000 },
			);
			await page.evaluate(async () => {
				const cache = await caches.open("markview-demo-fonts-v1");
				const request = (await cache.keys()).find((request) =>
					request.url.endsWith("/noto-serif-latin-wght-normal.woff2"),
				);
				await cache.put(
					request,
					new Response("not a font", {
						headers: { "content-type": "font/woff2" },
					}),
				);
			});
			await page.reload();
		} else {
			await page.route(malformedUrl, malformed);
			await page.goto("/index.html");
		}
		await expect(page.getByRole("alert")).toContainText("could not start");
		expect(requests).toHaveLength(15);
		expect(
			await page.evaluate(async () => {
				const cache = await caches.open("markview-demo-fonts-v1");
				return (await cache.keys()).length;
			}),
		).toBe(0);
		await page.unroute(malformedUrl, malformed);
		requests.length = 0;
		await page.getByRole("button", { name: "Try again" }).click();
		await expect(page.locator("body")).toHaveAttribute(
			"data-ready",
			"true",
			{ timeout: 90_000 },
		);
		expect(requests).toHaveLength(15);
		await expect(page.getByRole("alert")).toBeHidden();
		expect(
			await page.evaluate(async () => {
				const cache = await caches.open("markview-demo-fonts-v1");
				return (await cache.keys()).length;
			}),
		).toBe(15);
	});
}

for (const operation of ["unavailable", "open", "match", "put"]) {
	test(`font startup works when cache ${operation} fails`, async ({
		page,
	}) => {
		await mockCdnFonts(page);
		await page.addInitScript((operation) => {
			if (operation === "unavailable") {
				Object.defineProperty(window, "caches", { value: undefined });
				return;
			}
			const prototype =
				operation === "open" ? CacheStorage.prototype : Cache.prototype;
			prototype[operation] = async () => {
				throw new DOMException(
					"Storage unavailable",
					"QuotaExceededError",
				);
			};
		}, operation);
		await page.goto("/index.html");
		await expect(page.locator("body")).toHaveAttribute(
			"data-ready",
			"true",
			{ timeout: 90_000 },
		);
		await expect(page.getByRole("alert")).toBeHidden();
	});
}

for (const knownSize of [true, false]) {
	test(`startup reports streamed font progress ${knownSize ? "with" : "without"} a total size`, async ({
		page,
	}) => {
		await mockCdnFonts(page);
		await page.addInitScript((knownSize) => {
			const originalFetch = window.fetch;
			window.fetch = async (input, options) => {
				const response = await originalFetch(input, options);
				if (
					!String(input).endsWith(
						"/noto-serif-latin-wght-normal.woff2",
					)
				)
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
				"Loading fonts · 14/15 complete",
			);
			await expect(page.locator("#engine-text")).toHaveText(
				"Loading fonts · 14/15 complete",
			);
			await expect(page.locator("#loading-detail")).toHaveText(
				knownSize
					? /noto-serif-latin-wght-normal · 35% received · \d+ KB \/ \d+ KB/
					: /noto-serif-latin-wght-normal · \d+ KB received/,
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
