import { FontSet, type FontSource, type InitOptions } from "@markview/viewer";
export type { FontSource };

/** Direct files only. No CSS parsing or implicit character-range downloads. */
export interface FontDescription {
	readonly sources: readonly FontSource[];
}
export interface FontLoaderOptions {
	baseUrl?: string | URL;
	requestInit?: RequestInit;
	fetch?: typeof globalThis.fetch;
}

/** Explicit self-hosted or CDN file URLs; does not fetch until loaded. */
export function fontFiles(
	baseUrl: string | URL,
	files: readonly string[],
): FontDescription {
	return { sources: files.map((file) => new URL(file, baseUrl)) };
}

/** Caches explicit downloads and immutable sets within one request policy. */
export class FontLoader {
	readonly #options: FontLoaderOptions;
	#bytes = new Map<string, Promise<Uint8Array>>();
	#sets = new Map<string, Promise<FontSet>>();
	#identities = new WeakMap<object, number>();
	#nextIdentity = 0;

	constructor(options: FontLoaderOptions = {}) {
		this.#options = options;
	}

	async load(
		description: FontDescription,
		initialization?: Pick<InitOptions, "wasmUrl">,
	): Promise<FontSet> {
		const key = JSON.stringify(
			description.sources.map((source) => {
				if (typeof source === "string" || source instanceof URL)
					return this.#url(source);
				let identity = this.#identities.get(source);
				if (identity === undefined)
					this.#identities.set(
						source,
						(identity = this.#nextIdentity++),
					);
				return identity;
			}),
		);
		const previous = this.#sets.get(key);
		if (previous) {
			const set = await previous;
			if (!set.destroyed) return set;
			this.#sets.delete(key);
		}
		const pending = Promise.all(
			description.sources.map((source) => this.#read(source)),
		).then((faces) => FontSet.create(faces, initialization));
		this.#sets.set(key, pending);
		try {
			return await pending;
		} catch (error) {
			if (this.#sets.get(key) === pending) this.#sets.delete(key);
			for (const source of description.sources) {
				if (typeof source === "string" || source instanceof URL)
					this.#bytes.delete(this.#url(source));
			}
			throw error;
		}
	}

	/** Evicts cache entries; returned sets stay owned by the host. */
	clear(): void {
		this.#bytes.clear();
		this.#sets.clear();
	}

	#url(source: string | URL): string {
		const base = this.#options.baseUrl ?? globalThis.document?.baseURI;
		return new URL(source, base).href;
	}
	#read(source: FontSource): Promise<Uint8Array> {
		if (source instanceof Uint8Array) return Promise.resolve(source);
		if (source instanceof ArrayBuffer)
			return Promise.resolve(new Uint8Array(source));
		const url = this.#url(source);
		let pending = this.#bytes.get(url);
		if (!pending) {
			pending = Promise.resolve().then(async () => {
				try {
					const response = await (
						this.#options.fetch ?? globalThis.fetch
					)(url, this.#options.requestInit);
					if (!response.ok)
						throw new Error(`HTTP ${response.status}`);
					return new Uint8Array(await response.arrayBuffer());
				} catch (error) {
					if (this.#bytes.get(url) === pending)
						this.#bytes.delete(url);
					throw new Error(
						`could not load host font (${url}): ${String(error)}`,
						{ cause: error },
					);
				}
			});
			this.#bytes.set(url, pending);
		}
		return pending;
	}
}

const defaultLoader = new FontLoader();
/** Uses a shared cache and the default browser fetch policy. */
export function loadFontSet(
	description: FontDescription,
	initialization?: Pick<InitOptions, "wasmUrl">,
): Promise<FontSet> {
	return defaultLoader.load(description, initialization);
}
