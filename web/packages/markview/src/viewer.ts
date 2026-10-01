import { init, type InitOptions } from "./index.js";
import { CanvasReader, type CanvasReaderOptions } from "./reader.js";
import type { Heading, MarkviewOptions, MarkviewStats, Outline, SourceGeometry } from "./types.js";

export interface ReadingPosition {
	documentVersion: number;
	revision: number;
	/** Source position at the top of the reading viewport, in UTF-16 units. */
	offset: number;
	/** Vertical displacement within that rendered line, as a fraction of height. */
	fraction: number;
	reason: "user" | "programmatic" | "reflow";
	heading: Heading | null;
}
export interface ViewerOptions extends CanvasReaderOptions {
	initialization?: InitOptions;
	onReadingPosition?: (position: ReadingPosition) => void;
	onSectionChange?: (heading: Heading | null) => void;
}

/** Container-mounted reader with versioned source navigation and reading events. */
export class Viewer {
	readonly element: HTMLDivElement;
	readonly canvas: HTMLCanvasElement;
	readonly reader: CanvasReader;
	#markdown: string;
	#disposed = false;
	#subscribers = new Set<(position: ReadingPosition) => void>();
	#position: ReadingPosition | null = null;
	#headingTarget: string | null = null;
	#target: { offset: number; fraction: number; version: number } | null = null;
	#reason: ReadingPosition["reason"] = "reflow";
	#lastRevision = -1;
	#lastScroll = NaN;
	#section: string | null = null;
	#outline: Outline | null = null;

	private constructor(element: HTMLDivElement, canvas: HTMLCanvasElement, reader: CanvasReader, options: ViewerOptions) {
		this.element = element;
		this.canvas = canvas;
		this.reader = reader;
		this.#markdown = options.markdown ?? "";
		if (options.onReadingPosition) this.#subscribers.add(options.onReadingPosition);
	}

	static async mount(container: HTMLElement, options: ViewerOptions = {}): Promise<Viewer> {
		await init(options.initialization);
		const element = document.createElement("div");
		element.className = "markview-viewer";
		element.style.cssText = "position:relative;width:100%;height:100%;min-width:0;min-height:0;overflow:hidden";
		const canvas = document.createElement("canvas");
		canvas.style.cssText = "display:block;width:100%;height:100%;touch-action:none";
		element.append(canvas);
		container.append(element);
		let viewer: Viewer | undefined;
		try {
			const reader = await CanvasReader.attach(canvas, {
				...options,
				onUserInput: () => { if (viewer) viewer.#takeUserInput(); options.onUserInput?.(); },
				onStats: (stats) => { if (viewer) viewer.#frame(stats, options); options.onStats?.(stats); },
			});
			viewer = new Viewer(element, canvas, reader, options);
			return viewer;
		} catch (error) { element.remove(); throw error; }
	}

	getMarkdown(): string { this.#live(); return this.#markdown; }
	/** Replaces source progressively, keeping the nearest source reading position. */
	setMarkdown(markdown: string, preserveOffset = this.#position?.offset ?? 0): void {
		this.#live();
		const fraction = this.#position?.fraction ?? 0;
		this.#markdown = markdown;
		this.reader.setMarkdown(markdown);
		this.#position = null;
		this.#headingTarget = null;
		this.scrollToSource(preserveOffset, fraction);
	}
	outline(): Outline {
		this.#live();
		if (this.#outline?.documentVersion !== this.reader.markview.stats().documentVersion) {
			this.#outline = this.reader.markview.outline();
		}
		return this.#outline;
	}
	sourceToPreview(offset: number): SourceGeometry | null { this.#live(); return this.reader.markview.sourceToPreview(offset); }
	previewToSource(y: number): SourceGeometry | null { this.#live(); return this.reader.markview.previewToSource(y); }
	readingPosition(): ReadingPosition | null { this.#live(); return this.#position; }
	currentSection(): Heading | null { return this.readingPosition()?.heading ?? null; }

	/** Waits through ordinary budgeted layout; a new user gesture cancels waiting. */
	scrollToSource(offset: number, fraction = 0): void {
		this.#live();
		this.#headingTarget = null;
		this.#target = { offset: Math.max(0, Math.min(this.#markdown.length, offset)), fraction,
			version: this.reader.markview.stats().documentVersion };
		this.#reason = "programmatic";
		this.#followTarget();
	}
	navigateHeading(anchor: string): boolean {
		this.#live();
		this.#target = null;
		this.#reason = "programmatic";
		const navigated = this.reader.markview.navigateHeading(anchor);
		if (navigated) this.#headingTarget = anchor;
		return navigated;
	}
	setOptions(options: MarkviewOptions): void {
		this.#live();
		this.reader.markview.setOptions(options);
	}
	onReadingPosition(listener: (position: ReadingPosition) => void): () => void {
		this.#live();
		this.#subscribers.add(listener);
		return () => this.#subscribers.delete(listener);
	}
	destroy(): void {
		if (this.#disposed) return;
		this.#disposed = true;
		this.#subscribers.clear();
		this.#target = null;
		this.reader.destroy();
		this.element.remove();
	}

	#takeUserInput(): void {
		this.#target = null;
		this.#headingTarget = null;
		this.#reason = "user";
		this.#lastRevision = this.reader.markview.stats().revision;
	}
	#followTarget(): void {
		const target = this.#target;
		if (!target) return;
		if (target.version !== this.reader.markview.stats().documentVersion) { this.#target = null; return; }
		const geometry = this.sourceToPreview(target.offset);
		if (!geometry) return;
		this.reader.markview.setScroll(geometry.rect.y + target.fraction * geometry.rect.height);
		this.#target = null;
	}
	#frame(stats: MarkviewStats, options: ViewerOptions): void {
		const engine = this.reader.markview;
		const changed = stats.revision !== this.#lastRevision;
		const userMoved = this.#reason === "user" && engine.scroll() !== this.#lastScroll;
		if (changed && !userMoved && this.#position?.documentVersion === stats.documentVersion && !this.#target && !this.#headingTarget) {
			const geometry = this.sourceToPreview(this.#position.offset);
			if (geometry) engine.setScroll(geometry.rect.y + this.#position.fraction * geometry.rect.height);
			this.#reason = "reflow";
		}
		this.#followTarget();
		if (!stats.pending) this.#headingTarget = null;
		const scroll = engine.scroll();
		if (!changed && scroll === this.#lastScroll) return;
		this.#lastRevision = stats.revision;
		this.#lastScroll = scroll;
		const geometry = this.previewToSource(scroll);
		if (!geometry) return;
		const entries = this.outline().entries;
		let heading: Heading | null = null;
		for (const entry of entries) { if (entry.source.start <= geometry.source.start) heading = entry; }
		this.#position = { documentVersion: geometry.documentVersion, revision: geometry.revision,
			offset: geometry.source.start, fraction: (scroll - geometry.rect.y) / Math.max(1, geometry.rect.height),
			reason: this.#reason, heading };
		for (const subscriber of this.#subscribers) subscriber(this.#position);
		const section = `${stats.documentVersion}:${heading?.anchor ?? ""}`;
		if (section !== this.#section) {
			this.#section = section;
			options.onSectionChange?.(heading);
		}
	}
	#live(): void { if (this.#disposed) throw new Error("this Viewer has been destroyed"); }
}
