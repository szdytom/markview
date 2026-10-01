/** Host font files: OpenType, TrueType or collections, WOFF or WOFF2. */
export type FontSource = string | URL | ArrayBuffer | Uint8Array;

/** Fetches URL sources in parallel and keeps byte views' offsets intact. */
export async function loadFontSources(sources: readonly FontSource[]): Promise<Uint8Array[]> {
	return Promise.all(sources.map(async (source, index) => {
		if (source instanceof Uint8Array) return source;
		if (source instanceof ArrayBuffer) return new Uint8Array(source);
		try {
			const response = await fetch(source);
			if (!response.ok) throw new Error(`HTTP ${response.status}`);
			return new Uint8Array(await response.arrayBuffer());
		} catch (error) {
			throw new Error(`could not load host font at index ${index} (${source}): ${String(error)}`, { cause: error });
		}
	}));
}
