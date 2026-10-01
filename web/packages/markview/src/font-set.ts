import { FontSet as WasmFontSet } from "../wasm/markview_web.js";
import { init, type InitOptions } from "./index.js";

import { handles } from "./font-handles.js";

/** Immutable host faces. Readers retain their own shared snapshot. */
export class FontSet {
	private constructor(handle: WasmFontSet) {
		handles.set(this, handle);
	}

	/** Decode and validate every face before publishing the set. */
	static async create(
		faces: readonly (Uint8Array | ArrayBuffer)[],
		initialization?: Pick<InitOptions, "wasmUrl">,
	): Promise<FontSet> {
		await init(initialization);
		return new FontSet(
			new WasmFontSet(
				faces.map((face) =>
					face instanceof Uint8Array ? face : new Uint8Array(face),
				),
			),
		);
	}

	get destroyed(): boolean {
		return !handles.has(this);
	}

	/** Existing readers retain the faces; subsequent attachments throw. */
	destroy(): void {
		handles.get(this)?.free();
		handles.delete(this);
	}
}
