// Public types for `@markview/web`: the options the engine accepts and the
// statistics every read-back returns.

/** Layout and typography options; every key is optional with a default. */
export interface MarkviewOptions {
	/** Layout column width, logical px. (760) */
	width?: number;
	/** Base body size, logical px. (18) */
	fontSize?: number;
	/** Paper theme. ("light") */
	theme?: "light" | "dark";
	/** Justify body lines. (true) */
	justify?: boolean;
	/** Hyphenate across line breaks. (true) */
	hyphenate?: boolean;
	/** First-line indent of a paragraph, in multiples of the font size. (0) */
	paragraphIndent?: number;
	/** Greedy line breaking instead of optimum. (false) */
	greedy?: boolean;
	/** Treat leading front matter as metadata instead of content. (false) */
	hideFrontMatter?: boolean;
	/** Heading printed above hidden front matter. ("Metadata") */
	frontMatterLabel?: string;
}

/** The engine's counters, as returned by every stats read-back. */
export interface MarkviewStats {
	/** Changes only when Markdown source is replaced. */
	documentVersion: number;
	/** Bumped on every published snapshot. */
	revision: number;
	/** Blocks in the published snapshot. */
	blocks: number;
	/** Laid-out document height, logical px. */
	contentHeight: number;
	/** The reading column's width, logical px. It narrows to fit the canvas. */
	width: number;
	/** Blocks the last pass reused from the previous one. */
	reused: number;
	/** Last parse time, ms. */
	parseMs: number;
	/** Last layout time, ms. */
	layoutMs: number;
	/** Last presented frame time, ms. */
	frameMs: number;
	/** Frames presented so far. */
	frames: number;
	/** Glyphs in the atlas. */
	glyphs: number;
	/** Rendering backend, e.g. `"Gl"`. */
	backend: string;
	/** GPU adapter name. */
	adapter: string;
	/** UTF-16 code units in the current selection, like `String.length`. */
	selectionLength: number;
	/** Whether a started layout still has blocks to lay out. */
	pending: boolean;
}

/** Pointer modifier state for `Markview.pointerDown`. */
export interface Modifiers {
	shift: boolean;
	control: boolean;
	alt: boolean;
	meta: boolean;
}

/** Who maintains the motion between scroll events. */
export type ScrollMode = "external" | "internal";
export type DocumentCursor = "default" | "text" | "pointer";
/** An activation emitted on release, after a press that did not drag. */
export type PointerAction = { kind: "document"; reflowed: boolean } | { kind: "link" | "image"; target: string; modifiers: Modifiers };

/** Zero-based, half-open UTF-16 source range in the original string. */
export interface SourceRange { start: number; end: number; }
/** Document CSS pixels, independent of viewport scroll. */
export interface SourceGeometry {
	documentVersion: number;
	revision: number;
	source: SourceRange;
	rect: { x: number; y: number; width: number; height: number };
}
export interface Heading { text: string; level: number; anchor: string; source: SourceRange; }
export interface Outline { documentVersion: number; entries: Heading[]; }
