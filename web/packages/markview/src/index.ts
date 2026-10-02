// Public surface of `@markview/viewer`: `init()` plus the component classes.

import { default as __wbg_init, configureFonts, type InitInput } from "../wasm/markview_web.js";
import { loadFontSources } from "./fonts.js";
import type { FontSource } from "./fonts.js";
import { LayoutUpdate } from "./layout-update.js";
import { Markview } from "./markview.js";
import { CanvasReader } from "./reader.js";
import type { CanvasReaderOptions } from "./reader.js";
import type { MarkviewOptions, MarkviewStats, Modifiers, ScrollMode, DocumentCursor, PointerAction, SourceRange, SourceGeometry, Heading, Outline } from "./types.js";

export { CanvasReader, LayoutUpdate, Markview };
export { FontSet } from "./font-set.js";
export { Viewer } from "./viewer.js";
export type { ViewerOptions, ReadingPosition } from "./viewer.js";
export type { ImagePixels, ImagePriority, ImageRequest, ImageResourceEvent, ResourceOptions } from "./resources.js";
export type { CanvasReaderOptions, MarkviewOptions, MarkviewStats, Modifiers, ScrollMode, DocumentCursor, PointerAction, SourceRange, SourceGeometry, Heading, Outline };
export type { FontSource };

/** How `init()` finds the binary and the host's text fonts. */
export interface InitOptions {
	/**
	 * Where `markview_web_bg.wasm` is. Defaults to that file beside this
	 * module, which is where the package ships it. Set this when a bundler
	 * moves the JavaScript somewhere the binary does not follow.
	 */
	wasmUrl?: string | URL;
	/**
	 * @deprecated Use `FontSet` and the per-reader `fonts` option.
	 * Legacy default text fonts shared by readers without an explicit set. URLs are fetched in parallel; byte
	 * sources are copied into wasm. Omitted means no text fonts are available.
	 * Only KaTeX fonts are embedded. Supply every face your document needs.
	 */
	fonts?: readonly FontSource[];
}

/** The one instantiation, so repeat `init()` calls share it. */
let initPromise: Promise<void> | null = null;
// Font failures can happen while wasm is still loading; retries share that
// instantiation so a late completion cannot replace a configured module.
let wasmPromise: Promise<void> | null = null;

/**
 * Loads the wasm binary and registers the host's text fonts. Call before
 * creating readers; repeat calls share the first successful call's options.
 * Failed initialization can be retried with new options.
 *
 * The binary is named beside this module rather than imported, because an
 * imported asset is only an asset to the bundler that resolves it: a published
 * entry point that already holds a rewritten filename string makes every
 * downstream bundler emit JavaScript alone and leave the binary behind. A
 * plain sibling reference survives bundling, and `wasmUrl` covers the case
 * where it does not.
 *
 * @throws when the binary is missing — build it with `pnpm --dir web build`.
 * @throws when a host font cannot be fetched or is not a valid font file.
 */
export function init(options?: InitOptions): Promise<void> {
	initPromise ??= (async () => {
		const [, faces] = await Promise.all([
			initWasm(options),
			loadFontSources(options?.fonts ?? []),
		]);
		configureFonts(faces);
	})().catch((error: unknown) => {
		initPromise = null;
		throw error;
	});
	return initPromise;
}

function initWasm(options?: InitOptions): Promise<void> {
	if (wasmPromise) {
		// Reuse a prior load, but recover its failure with this retry's options.
		return wasmPromise.catch(() => initWasm(options));
	}
	wasmPromise = loadWasm(options).catch((error: unknown) => {
		wasmPromise = null;
		throw error;
	});
	return wasmPromise;
}

async function loadWasm(options?: InitOptions): Promise<void> {
	try {
		const source = options?.wasmUrl ?? new URL("markview_web_bg.wasm", import.meta.url);
		// The glue wants `{ module_or_path }` when an argument is passed.
		await __wbg_init({ module_or_path: source as InitInput });
	} catch (error) {
		throw new Error(
			"the Markview wasm module could not be loaded; run pnpm --dir web build "
			+ "and make sure markview_web_bg.wasm is served beside the module, or pass "
			+ `init({ wasmUrl }): ${String(error)}`,
			{ cause: error },
		);
	}
}

// `initSync` stays internal: the async `init()` is the supported entry point.
export default init;
