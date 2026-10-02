import { FontLoader } from "@markview/fonts";
import { fontFallbacks, fonts } from "../../assets/fonts.js";

export interface FontDownload {
	url: string;
	name: string;
	received: number;
	total: number;
	complete: boolean;
	cached: boolean;
}

async function fetchFont(input: RequestInfo | URL, options?: RequestInit) {
	const fallback = fontFallbacks.get(String(input));
	let response: Response;
	try {
		response = await fetch(input, options);
	} catch (error) {
		if (!fallback) throw error;
		return fetch(fallback, options);
	}
	return !response.ok && fallback ? fetch(fallback, options) : response;
}

export async function loadDemoFonts(
	onProgress: (downloads: readonly FontDownload[]) => void,
) {
	const downloads = fonts.map((url) => ({
		url: url.href,
		name: url.pathname
			.split("/")
			.pop()!
			.replace(/\.(otf|ttf|woff2?)$/, ""),
		received: 0,
		total: 0,
		complete: false,
		cached: false,
	}));
	onProgress(downloads);
	// Storage may be disabled or exhausted; caching is optional.
	const cache = await globalThis.caches
		?.open("markview-demo-fonts-v1")
		.catch(() => undefined);
	const writes: Promise<void>[] = [];
	let serif: Response;
	const loader = new FontLoader({
		fetch: async (input, options) => {
			const download = downloads.find(
				(font) => font.url === String(input),
			)!;
			const cached = await cache
				?.match(download.url)
				.catch(() => undefined);
			download.cached = !!cached;
			const response = cached ?? (await fetchFont(input, options));
			if (!response.ok || !response.body) return response;
			if (download.url === fonts[0]!.href) serif = response.clone();
			if (cache && !cached)
				writes.push(
					cache.put(download.url, response.clone()).catch(() => {}),
				);
			// Compressed transfer sizes cannot measure decoded stream progress.
			const encoding = response.headers.get("content-encoding");
			if (!encoding || encoding === "identity")
				download.total = Number(response.headers.get("content-length"));
			onProgress(downloads);
			return new Response(
				response.body.pipeThrough(
					new TransformStream<Uint8Array, Uint8Array>({
						transform(chunk, controller) {
							download.received += chunk.byteLength;
							onProgress(downloads);
							controller.enqueue(chunk);
						},
						flush() {
							download.complete = true;
							onProgress(downloads);
						},
					}),
				),
				response,
			);
		},
	});
	const set = await loader.load({ sources: fonts }).catch(async (error) => {
		if (cache && downloads.every((font) => font.complete)) {
			// Finish pending writes before evicting the unvalidated batch.
			await Promise.all(writes);
			await Promise.all(
				fonts.map((url) => cache.delete(url.href).catch(() => false)),
			);
		}
		throw error;
	});
	document.fonts.add(
		await new FontFace("Reading serif", await serif!.arrayBuffer(), {
			display: "swap",
			weight: "100 900",
		}).load(),
	);
	await Promise.all(writes);
	return set;
}
