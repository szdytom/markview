import type { FontSet as WasmFontSet } from "../wasm/markview_web.js";
import type { FontSet } from "./font-set.js";

export const handles = new WeakMap<FontSet, WasmFontSet>();

/** Internal ownership transfer; never consumes the host's reusable handle. */
export function fontHandle(set: FontSet): WasmFontSet {
	const handle = handles.get(set);
	if (!handle) throw new Error("this FontSet has been destroyed");
	return handle.duplicate();
}
