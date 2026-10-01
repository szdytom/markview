import { loadImageUrl } from "./image-loader.js";
import type { ImageResourceEvent, ResourceOptions } from "./types.js";
export { decodeImage, loadImageUrl } from "./image-loader.js";
export interface BrowserResourceOptions {
	baseUrl?: string | URL;
	requestInit?: RequestInit;
	onError?: (error: unknown) => void;
}
/** Opt-in transport; every request retains the viewer's cancellation signal. */
export function browserResources(
	options: BrowserResourceOptions = {},
): ResourceOptions {
	return {
		onResources: (events: readonly ImageResourceEvent[]) => {
			for (const event of events)
				if (event.kind === "request")
					void loadImageUrl(event.request, options);
		},
		...(options.onError ? { onError: options.onError } : {}),
	};
}

export type {
	ImagePixels,
	ImageRequest,
	ImageResourceEvent,
	ResourceOptions,
} from "./types.js";
