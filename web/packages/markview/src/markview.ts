// The `Markview` class: a thin typed facade over the wasm-bindgen handle.
// Every engine call returns plain values or a JSON `Stats` string; this class
// parses the JSON once and hands out `MarkviewStats`.

import { Markview as WasmMarkview, create as wasmCreate } from "../wasm/markview_web.js";
import { ResourceEvents, type ResourceOptions } from "./resources.js";
import { LayoutUpdate } from "./layout-update.js";
import type { MarkviewOptions, MarkviewStats, Modifiers, ScrollMode, DocumentCursor, PointerAction, SourceGeometry, Outline } from "./types.js";
import { parseStats, serializeOptions } from "./internal.js";

/**
 * One rendering of a Markdown document, driven by the page.
 *
 * Create with `Markview.create(canvas, options)` after `init()` has resolved.
 * `destroy()` releases the wasm handle; every later call throws.
 */
export class Markview {
	/** The wasm handle; `null` once `destroy()` ran. Not reachable at runtime. */
	#handle: WasmMarkview | null;

	/**
	 * Bumped by every `beginLayout`. The handle keeps one pending pass, so an
	 * update holding an older number no longer owns it.
	 */
	#generation = 0;
	readonly #resources: ResourceEvents;

	private constructor(handle: WasmMarkview, resources?: ResourceOptions) {
		this.#resources = new ResourceEvents(resources);
		this.#handle = handle;
	}

	/** Builds a handle that draws into `canvas`, importing `options`. */
	static async create(canvas: HTMLCanvasElement, options?: MarkviewOptions, resources?: ResourceOptions): Promise<Markview> {
		// A handle created before any frame will size itself on the first
		// `resize()`; nothing else needs to happen here.
		const handle = await wasmCreate(canvas, serializeOptions(options));
		return new Markview(handle, resources);
	}

	/** Parses `markdown`, lays the document out completely and publishes it. */
	setMarkdown(markdown: string): MarkviewStats {
		// The full layout cancels whatever pass was pending, so every handle
		// already handed out stops owning anything.
		this.#supersede();
		const handle = this.#live();
		const stats = parseStats(handle.setMarkdown(markdown));
		this.#resources.replace(handle);
		return stats;
	}

	/** Starts a resumable layout of `markdown` and returns its handle. */
	beginLayout(markdown: string): LayoutUpdate {
		// The stats `beginUpdate` returns carry the revision published before
		// the pass started; a step whose revision still matches has published
		// nothing of its own yet.
		const baseline = parseStats(this.#live().beginUpdate(markdown)).revision;
		this.#resources.replace(this.#live());
		const generation = this.#supersede();
		// The update asks for the handle on every call, so a `destroy()` that
		// lands before it finishes raises this class's own error, and it asks
		// whether it still owns the pending pass, so neither a later
		// `beginLayout` nor a full replacement can be steered by an update for
		// the document it replaced.
		return new LayoutUpdate(
			() => this.#live(),
			() => this.#generation === generation,
			baseline,
			() => this.#flushResources(),
		);
	}

	/** Renders and presents one frame from the latest published snapshot. */
	frame(): MarkviewStats {
		this.#flushResources();
		const handle = this.#live();
		const stats = parseStats(handle.frame());
		this.#resources.priorities(handle);
		return stats;
	}

	/**
	 * Sizes the drawing surface from logical CSS pixels plus the device ratio.
	 * Returns whether the reading column narrowed, which starts a reflow and
	 * replaces any resumable layout in flight.
	 */
	resize(cssWidth: number, cssHeight: number, dpr?: number): boolean {
		const reflowed = this.#live().resize(normalize(cssWidth), normalize(cssHeight), normalize(dpr ?? 1));
		// A reflow takes the pending pass, so it supersedes any layout update
		// that was driving it.
		if (reflowed) this.#supersede();
		return reflowed;
	}

	/** Complete outline of the newest parsed document, including hidden headings. */
	outline(): Outline { return JSON.parse(this.#live().outline()) as Outline; }
	/** Returns `null` until this source position has current-version geometry. */
	sourceToPreview(offset: number): SourceGeometry | null {
		return JSON.parse(this.#live().sourceToPreview(Math.max(0, Math.trunc(normalize(offset))))) as SourceGeometry | null;
	}
	/** Maps document CSS pixels to the closest visible source line. */
	previewToSource(y: number): SourceGeometry | null {
		return JSON.parse(this.#live().previewToSource(normalize(y))) as SourceGeometry | null;
	}
	/** Opens enclosing disclosures and waits for unpublished heading geometry. */
	navigateHeading(anchor: string): boolean { return this.#live().navigateHeading(anchor); }

	/** Sets the document scroll in logical pixels. */
	setScroll(y: number): void {
		this.#live().setScroll(normalize(y));
	}

	/** Scrolls the document by `dy` logical px. */
	scrollBy(dy: number): void {
		this.#live().scrollBy(normalize(dy));
	}

	/** Scrolls to the final document end, waiting for pending layout. */
	scrollToEnd(): void {
		this.#live().scrollToEnd();
	}

	/** The current scroll position, logical px. */
	scroll(): number {
		return this.#live().scroll();
	}

	/** The largest scroll position, logical px; `0` when the document fits. */
	maxScroll(): number {
		return this.#live().maxScroll();
	}

	/** The laid-out document height, logical px. */
	contentHeight(): number {
		return this.#live().contentHeight();
	}

	/** Starts a press: a word selection, or a block one when already pressed. */
	pointerDown(x: number, y: number, modifiers?: Partial<Modifiers>): void {
		const mods = modifiers ?? {};
		const live = this.#live();
		live.pointerDown(normalize(x), normalize(y), mods.shift === true, mods.control === true, mods.alt === true, mods.meta === true);
	}

	/** Extends the press in flight. */
	pointerMove(x: number, y: number): void {
		this.#live().pointerMove(normalize(x), normalize(y));
	}

	/** Ends the press in flight. */
	pointerUp(x: number, y: number): PointerAction | null {
		const action = JSON.parse(this.#live().pointerUp(normalize(x), normalize(y))) as PointerAction | null;
		if (action?.kind === "document" && action.reflowed) this.#supersede();
		return action;
	}
	/** Stops a captured gesture without activating its target. */
	cancelPointer(): void { this.#live().cancelPointer(); }
	/** Clears hover when the pointer leaves without a captured gesture. */
	pointerLeave(): void { this.#live().pointerLeave(); }
	/** The cursor at the current pointer, including scroll and reflow updates. */
	cursor(): DocumentCursor { return this.#live().cursor() as DocumentCursor; }
	/** Enables image activation when the host can answer it. */
	setImagesClickable(clickable: boolean): void { this.#live().setImagesClickable(clickable); }
	/** Cancels current animation and changes ownership of wheel motion. */
	setScrollMode(mode: ScrollMode): void { this.#live().setScrollMode(mode); }
	/** Feeds normalized CSS-pixel travel to the shared scrolling logic. */
	scrollInput(dx: number, dy: number, kind: "external" | "step"): void {
		this.#live().scrollInput(normalize(dx), normalize(dy), kind);
	}

	/** Selects the whole document. */
	selectAll(): void {
		this.#live().selectAll();
	}

	/** Clears the selection. */
	clearSelection(): void {
		this.#live().clearSelection();
	}

	/** The reading text of the selection; `""` when the selection is empty. */
	selectedText(): string {
		return this.#live().selectedText();
	}

	/** Writes `selectedText()` to the clipboard. Resolves false when empty. */
	async copy(): Promise<boolean> {
		const text = this.selectedText();
		if (!text) return false;
		try {
			await navigator.clipboard.writeText(text);
			return true;
		} catch {
			// The async clipboard can refuse without a user gesture, where the
			// legacy command still copies what the document has selected.
			return legacyCopy(text);
		}
	}

	/** A snapshot of the engine's counters. */
	stats(): MarkviewStats {
		return parseStats(this.#live().stats());
	}

	/** The GPU adapter and backend this handle draws through. */
	adapter(): string {
		return this.#live().adapter();
	}

	/**
	 * Advances a pass the handle started by itself and reports whether one is
	 * still running. A resumable layout owns its own pass through
	 * [`LayoutUpdate`]; this drives work nothing else holds, which today is the
	 * reflow a narrower canvas triggers.
	 */
	stepPending(budgetMs?: number): boolean {
		this.#flushResources();
		const handle = this.#live();
		if (!handle.updatePending()) return false;
		const budget = budgetMs ?? 8;
		handle.stepUpdate(Number.isFinite(budget) ? budget : 8);
		return handle.updatePending();
	}

	/** Replaces the layout options and lays the document out again. */
	setOptions(options: MarkviewOptions): void {
		// Relaying out cancels the pending pass, so it supersedes it too.
		this.#supersede();
		this.#live().setConfig(serializeOptions(options));
	}

	/** Releases the wasm handle. Later calls throw. */
	destroy(): void {
		const handle = this.#live();
		this.#handle = null;
		this.#supersede();
		this.#resources.clear();
		handle.free();
	}

	#flushResources(): void {
		if (this.#resources.flush(this.#live())) this.#supersede();
	}

	/** The live handle, or the `destroy()` failure every later call must raise. */
	#live(): WasmMarkview {
		if (!this.#handle) {
			throw new Error("this Markview has been destroyed");
		}
		return this.#handle;
	}

	/**
	 * Takes ownership of the handle's pending pass, making every update handed
	 * out before this inert. Returns the new generation.
	 */
	#supersede(): number {
		this.#generation += 1;
		return this.#generation;
	}
}

// The engine clamps NaN and infinities itself; the facade only needs to keep
// them from crossing the boundary as an exception.
function normalize(value: number): number {
	return Number.isFinite(value) ? value : 0;
}

// `execCommand` still copies where the async clipboard needs a permission.
function legacyCopy(text: string): boolean {
	// `select()` focuses the helper, so the focus it takes must be handed back,
	// or the canvas would never see a keyboard event again.
	const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
	const helper = document.createElement("textarea");
	helper.value = text;
	helper.setAttribute("readonly", "");
	helper.style.cssText = "position:fixed;top:-1000px;opacity:0";
	document.body.append(helper);
	helper.select();
	const copied = document.execCommand("copy");
	helper.remove();
	previous?.focus({ preventScroll: true });
	return copied;
}
