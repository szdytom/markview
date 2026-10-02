import { FontLoader } from "@markview/fonts";
import { fonts } from "../../assets/fonts.js";

export interface FontDownload {
	url: string;
	name: string;
	received: number;
	total: number;
	complete: boolean;
}

export function loadDemoFonts(
	onProgress: (downloads: readonly FontDownload[]) => void,
) {
	const downloads = fonts.map((url) => ({
		url: url.href,
		name: url.pathname
			.split("/")
			.pop()!
			.replace(/\.(otf|ttf)$/, ""),
		received: 0,
		total: 0,
		complete: false,
	}));
	onProgress(downloads);
	const loader = new FontLoader({
		fetch: async (input, options) => {
			const download = downloads.find(
				(font) => font.url === String(input),
			)!;
			const response = await fetch(input, options);
			if (!response.ok || !response.body) return response;
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
	return loader.load({ sources: fonts });
}
