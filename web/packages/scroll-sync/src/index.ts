/** Half-open UTF-16 offsets in the exact source shared by both panes. */
export interface SourceRange {
	start: number;
	end: number;
}
/** Full vertical extent of a source range, including every wrapped line. */
export interface SourceExtent {
	top: number;
	bottom: number;
}
export interface SourceViewport {
	offset: number;
	top: number;
}
/** A source offset and vertical progress through its rendered unit. */
export interface ScrollAnchor {
	offset: number;
	fraction: number;
}
/** Coordinates use the same units as `SourceViewport.top`; `bottom > top`. */
export type SourceMeasurer = (source: SourceRange) => SourceExtent | null;

/** Captures progress across the complete semantic range, rather than one wrap. */
export function sourceToAnchor(
	viewport: SourceViewport,
	source: SourceRange | null,
	measure: SourceMeasurer,
): ScrollAnchor {
	const extent = measure(
		source ?? { start: viewport.offset, end: viewport.offset },
	);
	const fraction = extent
		? (viewport.top - extent.top) / (extent.bottom - extent.top)
		: 0;
	return {
		offset: viewport.offset,
		fraction: Math.max(0, Math.min(1, fraction)),
	};
}

/** Projects a reading anchor into source coordinates, preserving preview gaps. */
export function anchorToSource(
	anchor: ScrollAnchor,
	source: SourceRange | null,
	measure: SourceMeasurer,
): number | null {
	const extent = measure(
		source ?? { start: anchor.offset, end: anchor.offset },
	);
	return extent
		? extent.top + anchor.fraction * (extent.bottom - extent.top)
		: null;
}

export type SyncPane = "source" | "preview";
/** Serializable ticket for a measurement or a message across a host boundary. */
export interface SyncRequest {
	readonly origin: SyncPane;
	readonly documentVersion: number;
	readonly generation: number;
}

/** Input ownership and cancellation for synchronous or asynchronous adapters. */
export class ScrollSync {
	#owner: SyncPane = "source";
	#documentVersion: number;
	#generation = 0;

	constructor(documentVersion: number) {
		this.#documentVersion = documentVersion;
	}
	get owner(): SyncPane {
		return this.#owner;
	}
	/** Call for user gestures or explicit navigation, never for a follow event. */
	takeControl(pane: SyncPane): void {
		this.#owner = pane;
		this.cancel();
	}
	setDocumentVersion(version: number): void {
		if (version === this.#documentVersion) return;
		this.#documentVersion = version;
		this.cancel();
	}
	/** Invalidates pending work without changing the controlling pane. */
	cancel(): void {
		this.#generation++;
	}
	/** Follower events and obsolete document versions cannot start a request. */
	begin(
		origin: SyncPane,
		documentVersion = this.#documentVersion,
	): SyncRequest | null {
		if (origin !== this.#owner || documentVersion !== this.#documentVersion)
			return null;
		return { origin, documentVersion, generation: ++this.#generation };
	}
	/** Only the latest request may apply a measured or remotely delivered result. */
	isCurrent(request: SyncRequest): boolean {
		return (
			request.origin === this.#owner &&
			request.documentVersion === this.#documentVersion &&
			request.generation === this.#generation
		);
	}
}
