// Opt-in browser helpers; the component itself performs no resource I/O.

import type { ImagePixels, ImageRequest } from "./types.js";

/** Decodes a static frame using the browser and releases temporary objects. */
export async function decodeImage(
	source: Blob | ArrayBuffer | Uint8Array,
	signal?: AbortSignal,
): Promise<ImagePixels> {
	signal?.throwIfAborted();
	let blob =
		source instanceof Blob
			? source
			: new Blob([
					source instanceof Uint8Array
						? source.slice().buffer
						: source,
				]);
	if (!blob.type && /^\s*</.test(await blob.slice(0, 256).text())) {
		const root = new DOMParser().parseFromString(
			await blob.text(),
			"image/svg+xml",
		).documentElement;
		if (root.localName === "svg")
			blob = blob.slice(0, blob.size, "image/svg+xml");
	}
	signal?.throwIfAborted();
	const url = URL.createObjectURL(blob);
	const image = new Image();
	let abort: (() => void) | undefined;
	try {
		image.src = url;
		await new Promise<void>((resolve, reject) => {
			abort = () => reject(signal?.reason);
			signal?.addEventListener("abort", abort, { once: true });
			image.decode().then(resolve, reject);
		});
		signal?.throwIfAborted();
		const canvas = document.createElement("canvas");
		canvas.width = image.naturalWidth;
		canvas.height = image.naturalHeight;
		try {
			const context = canvas.getContext("2d", {
				willReadFrequently: true,
			});
			if (!context)
				throw new Error("Browser image decoding requires a 2D canvas");
			context.drawImage(image, 0, 0);
			const data = context.getImageData(
				0,
				0,
				canvas.width,
				canvas.height,
			).data;
			return {
				width: canvas.width,
				height: canvas.height,
				rgba: new Uint8Array(
					data.buffer,
					data.byteOffset,
					data.byteLength,
				),
			};
		} finally {
			canvas.width = canvas.height = 0;
		}
	} finally {
		if (abort) signal?.removeEventListener("abort", abort);
		image.src = "";
		URL.revokeObjectURL(url);
	}
}

/** Fetches, decodes and completes one request; failures become placeholders. */
export async function loadImageUrl(
	request: ImageRequest,
	options: { baseUrl?: string | URL; requestInit?: RequestInit } = {},
): Promise<void> {
	try {
		request.signal.throwIfAborted();
		const url = new URL(request.src, options.baseUrl ?? document.baseURI);
		if (
			!["http:", "https:", "blob:"].includes(url.protocol) &&
			!(url.protocol === "data:" && /^data:image\//i.test(url.href))
		) {
			throw new Error(`Unsupported image URL protocol: ${url.protocol}`);
		}
		const signal = options.requestInit?.signal
			? AbortSignal.any([request.signal, options.requestInit.signal])
			: request.signal;
		const response = await fetch(url, { ...options.requestInit, signal });
		if (!response.ok)
			throw new Error(`Image request failed: HTTP ${response.status}`);
		const pixels = await decodeImage(await response.blob(), signal);
		signal.throwIfAborted();
		request.resolve(pixels);
	} catch (error) {
		if (!request.signal.aborted) request.reject(String(error));
	}
}
