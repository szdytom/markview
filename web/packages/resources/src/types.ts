/** Browser transport only needs these fields of a viewer/host request. */
export interface ImagePixels {
	width: number;
	height: number;
	rgba: Uint8Array;
}
export interface ImageRequest {
	readonly src: string;
	readonly signal: AbortSignal;
	resolve(pixels: ImagePixels): void;
	reject(message: string): void;
}
export interface ImageResourceEvent {
	kind: "request" | "priority";
	request: ImageRequest;
}
export interface ResourceOptions {
	onResources?: (events: readonly ImageResourceEvent[]) => void;
	onError?: (error: unknown) => void;
}
