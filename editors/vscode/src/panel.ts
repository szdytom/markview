/**
 * The preview panel: the engine's pixels in a webview beside the source.
 *
 * The webview owns the scroll surface. It keeps a spacer as tall as the
 * document and asks for the bands it is showing, so a long document costs only
 * the screenful in view. Each band's text layer travels back with its tile and
 * gives that part of the screen both of the mappings the panel needs: a click
 * resolves to a byte through a cluster's rectangle, and a byte resolves to a
 * position through a row.
 *
 * The two surfaces follow each other. A scroll in the preview reveals the byte
 * its top edge is showing in the editor, and a scroll in the editor moves the
 * preview to the block holding its first visible byte. Neither moves the caret,
 * and each direction stands down for a moment after the other has moved, so
 * the two cannot drive each other.
 */
import * as vscode from "vscode";
import { templateFor } from "./template.js";
import * as path from "path";
import { randomBytes } from "node:crypto";
import type { Block, Cluster, Row, Session, State } from "./sidecar.js";

/** How long typing settles before the engine is asked for the new text. */
const COALESCE_MS = 40;

let panel: vscode.WebviewPanel | undefined;
/** The group the panel was opened in, for revealing it again. */
let panelColumn: vscode.ViewColumn | undefined;
let opening = Promise.resolve();
let openRequest = 0;
let requestedDocument: vscode.TextDocument | undefined;
let followingSession: Session | undefined;
/**
 * What the panel has observed, for a test to assert against.
 *
 * A webview's inside is not reachable from the extension host, so the panel
 * records what the webview reports and what it asks for. That is the same
 * information the panel acts on, not a separate account of it.
 */
const report = {
	ready: false,
    documentUri: "",
    background: "",
    restored: false,
    tileError: "",
	webviewBytes: 0,
	selectionRequest: undefined as { start: number; end: number } | undefined,
	selectionState: null as unknown,
	/** The scroll offsets the webview asked for, in order. */
	requested: [] as number[],
	/** The height the webview gave its scrollable extent. */
	scrollHeight: 0,
	/** The height the engine gave the document. */
	documentHeight: 0,
	/** The height the `open` answer itself carried. */
	openedHeight: 0,
	/** How many blocks the engine published with it. */
	blocks: 0,
	/** How many bands the webview holds right now, which stays bounded. */
	live: 0,
	/** The webview's viewport height, in layout pixels. */
	viewport: 0,
	/** Where the webview's view sits, in document pixels. */
	scroll: 0,
	/** The source offset at the top of the view, and how far into its block. */
	source: -1,
	into: 0,
	/** What the panel asked the engine for, to observe coalescing. */
	opens: 0,
	/** Scrolls carried from one surface to the other, by direction. */
	syncs: { preview: 0, editor: 0 },
	/** Scrolls ignored because the other surface had just moved. */
	stoodDown: 0,
	/** The source offset the panel last revealed in the editor. */
	revealed: -1,
	/** The source offset the panel last carried the editor's view to. */
	carried: -1,
	/** The links the reader followed, in order. */
	routed: [] as Array<{ kind: string; target: string }>,
	/** What the last click in the preview resolved to. */
	clicked: null as unknown,
	/** Every link the layout drew, with the rectangle a click hits. */
	links: [] as Array<Record<string, unknown>>,
	/** Where the engine says each fragment in this document is. */
	anchors: {} as Record<string, number>,
	/** What the reader has selected in the preview. */
	selection: undefined as { text: string; source_start: number } | undefined,
	/** Whether the webview's own copy command wrote the selection out. */
	copied: false,
	/** Why it did not, when it did not. */
	copyError: "",
	/** Whether the webview's document had the focus the browser wants. */
	focused: false,
	/** How many text-layer spans the webview holds, for a test to read. */
	spans: 0,
	/** The settings the panel resolved for the document it is showing. */
	settings: {} as Record<string, unknown>,
	/** The appearance the panel last sent the engine. */
	appearance: { theme: "", reflow: false },
	/** The path the panel last exported to. */
	exported: "",
	/** The scroll offset of the last tile, and a fingerprint of its pixels. */
	tile: -1,
	painted: 0,
	/** Version acknowledged after decoded visible tiles pass a paint frame. */
	paintedVersion: -1,
	displayedTiles: 0,
	pixelVersions: [] as number[],
	refreshing: false,
	tileBounds: [] as Array<{ width: number; height: number; expectedHeight: number; pixels: number; expectedPixels: number; scale: number; fit: number; viewportWidth: number; documentWidth: number; inset: number[] }>,
	paintedAt: 0,
	/** The characters drawn so far, and how they are ordered. */
	text: "",
	ordered: true,
	unique: true,
	/** The document's own text, as the find widget sees it. */
	findable: { length: 0, head: "", tail: "" },
	/** The rendered text the find widget searches. */
	rendered: "",

	editors: [] as string[],
	/** Scrolls the preview reported, and how many of them mapped to a block. */
	scrollMessages: 0,
	unmapped: 0,
	/** Visible-range events from the editor for the document on screen. */
	editorEvents: 0,
};

/** Counts JSON metadata and binary bytes; VS Code's transport framing is excluded. */
function post(message: unknown) {
    let binaryBytes = 0;
    const json = JSON.stringify(message, (_key, value) => {
        if (value instanceof Uint8Array) { binaryBytes += value.byteLength; return null; }
        return value;
    });
    report.webviewBytes += Buffer.byteLength(json) + binaryBytes;
    return panel?.webview.postMessage(message);
}

/**
 * The bands a webview holds at once: the ones in view and their neighbours, so
 * a scroll has the next screen ready without keeping the ones left behind.
 */
const KEEP = 1;

/** What the panel has observed so far. */
export function panelReport() {
	return {
		...report,
		requested: [...report.requested],
		syncs: { ...report.syncs },
	};
}
/** The engine's id for each document this panel is showing. */
const ids = new Map<string, string>();
let sequence = 0;
let findEcho: { start: number; end: number } | undefined;
let coalescing: NodeJS.Timeout | undefined;
let layoutRequest = 0;
/** Stops listening to the geometry of the document shown before this one. */
let unfollow: (() => void) | undefined;
/** How long an edit keeps the two surfaces from carrying each other's moves. */
const UPDATE_MS = 400;
let updatingUntil = 0;
let holdTimer: NodeJS.Timeout | undefined;
/**
 * A move made while an edit held the surfaces still.
 *
 * The hold is about not carrying the editor's view moving under the text, not
 * about ignoring the reader. A scroll made during it is kept and carried once
 * the edit has settled.
 */
let deferred: { preview?: boolean; editor?: boolean } = {};
/**
 * Where the reader is: the source offset of the block at the top of the view
 * and how far into it the view is.
 *
 * An edit moves the text under the reader, so the anchor is moved with it by
 * exactly the same amount, which is what keeps the same words on screen.
 */
let anchor: { offset: number; into: number } | undefined;
/**
 * What a report from each state the webview may still be holding has to be
 * shifted by.
 *
 * The reader can scroll before the new state arrives, and that report names an
 * offset in the text as it was before the edit. The host knows what the edit
 * moved, so the report is shifted into the current text rather than discarded.
 */
let generation = 0;
const shifts = new Map<number, number>([[0, 0]]);
/** The editor listeners for the document the panel is showing. */
let watching: vscode.Disposable[] = [];

function render(uri: string, scroll: number): string {
	const nonce = randomBytes(16).toString("hex");
	return `<!DOCTYPE html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src blob:; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
<style>
	html, body { margin: 0; padding: 0; height: 100%; overflow: hidden; }
	#viewport { position: absolute; inset: 16px; overflow-y: auto; overflow-x: hidden; scrollbar-gutter: stable; }
	#spacer { position: relative; width: 100%; overflow: hidden; }
	#page { position: absolute; top: 0; left: 0; transform-origin: 0 0; }
	#tiles { position: absolute; top: 0; left: 0; width: 100%; }
	#tiles img { position: absolute; left: 0; width: 100%; max-height: none; display: block; }
	#text { position: absolute; top: 0; left: 0; right: 0;
		pointer-events: none; }
	#text span { position: absolute; color: transparent; white-space: pre;
		transform-origin: 0 0;
		/* The reader selects and copies the engine's own characters; the
		   pixels underneath are what they see. */
		pointer-events: auto; user-select: text; -webkit-user-select: text; }
	#notice { position: absolute; left: 0; right: 0; bottom: 0;
		font: 12px system-ui; padding: 4px 8px; color: var(--muted); }
	/* The document's own text, out of the way but rendered: the find widget
	   searches what is rendered, and the pixels below cannot be searched. */
	#source { position: absolute; left: -100000px; top: 0; width: 1px;
		white-space: pre; color: transparent; }
</style></head><body>
<div id="source"></div>
<div id="viewport"><div id="spacer"><div id="page"><div id="tiles"></div><div id="text"></div></div></div></div>
<div id="notice"></div>
<script nonce="${nonce}">
const vscode = acquireVsCodeApi();
const documentUri = ${JSON.stringify(uri).replace(/</g, '\\u003c')};
let restoredTop = ${scroll};
const persist = () => vscode.setState({ uri: documentUri, scroll: scrollY() });
const viewport = document.getElementById('viewport');
const spacer = document.getElementById('spacer');
const page = document.getElementById('page');
let fit = 1;
const scrollY = () => viewport.scrollTop / fit;
const viewHeight = () => Math.ceil(viewport.clientHeight / fit);
const rasterScale = () => window.devicePixelRatio * fit;
function fitPage(top = scrollY()) {
    if (!state || viewport.clientWidth <= 0) return;
    fit = state ? Math.min(1, viewport.clientWidth / state.width) : 1;
    page.style.width = (state ? state.width : viewport.clientWidth) + 'px';
    page.style.transform = 'scale(' + fit + ')';
    spacer.style.height = height() * fit + 'px';
    viewport.scrollTop = top * fit;
}
const tiles = document.getElementById('tiles');
const text = document.getElementById('text');
const notice = document.getElementById('notice');
const source = document.getElementById('source');
let state = null;
/** Which state this webview is showing, so the host can place a report. */
let shown = 0;
let epoch = 0;
let paintedEpoch = -1;
let displayEpoch = -1;
const KEEP = ${KEEP};
/** What is on screen: the band index, its tile, its text and its rows. */
let resident = new Map();
/** The rows drawn so far, and the spans that carry the text. */
let rows = [];
let known = new Map();
/** The document text the drawn spans belong to, and the findable rendered
    text and its source ranges once the reader asks for them. */
let drawnVersion = -1;
let ranges = [];
let pendingFind = undefined;
let pending = new Set();
/** The range the editor selected, until the spans carrying it are drawn. */
let pendingSelection = null;
function height() { return state ? state.height : 0; }

/** The band indices the viewport shows: a scroll rarely lands on a boundary. */
function firstBand(top, view) { return Math.max(0, Math.floor(top / view)); }
function lastBand(top, view) {
	return Math.max(0, Math.floor(Math.max(0, top + view - 1) / view));
}

function report() {
	const top = rowAt(scrollY());
	vscode.postMessage({
		generation: shown,
        selectionState: { pending: pendingSelection, selected: selected(), first: text.firstChild?.dataset.start, last: text.lastChild?.dataset.end },
		live: resident.size,
		displayedTiles: tiles.childElementCount,
		pixelVersions: [...tiles.children].map(image => Number(image.dataset.version)),
		refreshing: displayEpoch !== epoch,
		viewport: viewHeight(),
		scrollHeight: viewport.scrollHeight / fit,
		at: scrollY(),
		// What the reader is looking at, as a source offset and how far into
		// its row the top of the view is.
		source: top ? top.source_start : -1,
		into: top ? scrollY() - top.y : 0,
		// The characters drawn so far, which is what a copy would take, and
		// whether the spans carrying them are in reading order and one per
		// source offset. Bands arrive in the order the reader scrolls, which
		// is not the order of the text.
		text: text.textContent,
		ordered: (function () {
			let last = -1;
			for (const span of text.children) {
				const start = Number(span.dataset.start);
				if (start < last) return false;
				last = start;
			}
			return true;
		})(),
		unique: known.size === text.children.length,
		// The document's own text, which the find widget searches, reported
		// by its ends so that a report stays small.
		findable: {
			length: source.textContent.length,
			head: source.textContent.slice(0, 32),
			tail: source.textContent.slice(-32),
		},
		// The rendered text the find widget searches, once it has been asked
		// for, and where in it the phrase the test looked for sits.
	});
}

/** The block covering a vertical position in the document. */
function blockAt(y) {
	if (!state) return undefined;
	return state.blocks.find(function (block) {
		return block.y <= y && y < block.y + block.height;
	});
}

/** The block a source offset falls in aim of. */
function blockAtSource(offset) {
	if (!state) return undefined;
	// The blank line between two paragraphs belongs to neither of them, so an
	// offset in one is answered by the block that comes next: that is the text
	// below where the editor's view is.
	return (
		state.blocks.find(function (block) {
			return block.source_start <= offset && offset < block.source_end;
		}) ||
		state.blocks.find(function (block) {
			return block.source_start >= offset;
		})
	);
}

/**
 * Puts the view back where the reader was, in a new state.
 *
 * A keystroke above the view moves everything below it down, so the offset
 * the reader was at no longer holds the same text. The host tracks the block
 * the reader is in through the edit and sends its new offset, and the view is
 * put back the same distance into that block.
 */
function forget(clear = false) {
	epoch += 1;
	// Keep the last complete pixels until the replacement viewport decodes.
	if (clear) tiles.replaceChildren();
	text.inert = true;
	resident.clear();
	pending.clear();
}

/** Empties the drawn text, which belongs to the text the state replaced. */
function forgetText() {
	text.replaceChildren();
	known.clear();
	rows = [];
}

/** Drops the bands more than KEEP screens from the ones in view. */
function prune(first, last) {
	for (const [index, band] of Array.from(resident)) {
		if (index < first - KEEP || index > last + KEEP) {
			band.image.remove();
			resident.delete(index);
		}
	}
}

/** Every row drawn so far, in the order it is drawn. */
function rowsOnScreen() {
	return drawnRows();
}

/**
 * The row a vertical position falls in, which is the first cluster of the
 * line it lands on: a line of text is many rows sharing one y.
 */
function rowAt(y) {
	let top = null;
	for (const row of rowsOnScreen()) {
		if (row.y <= y && y < row.y + row.height) {
			// Rows of one line share a y, and neighbouring lines can overlap by
			// a fraction of a pixel, so the line at the top of the view is the
			// last one that starts at or above it; within a line it is the
			// first cluster, which is the one with the lowest source offset.
			if (
				!top ||
				row.y > top.y ||
				(row.y === top.y && row.source_start < top.source_start)
			) {
				top = row;
			}
		}
	}
	// Spacing follows the next row from the currently painted band.
	const band = resident.get(firstBand(y, viewHeight()));
	return top || band?.rows.find(row => row.y > y) || null;
}

/** The row a source offset belongs to, if one of them was drawn from it. */
function rowFor(offset) {
	return rowsOnScreen().find(function (row) {
		return row.source_start <= offset && offset < row.source_end;
	});
}

/**
 * Draws a band's text over its pixels.
 *
 * A band's text is added once and never taken away: the reader selects, copies
 * and searches what has been drawn, and a selection whose nodes a scroll
 * removed would not survive being extended. Adding rather than rebuilding is
 * what keeps those nodes; the order is kept by inserting each cluster where
 * its source offset belongs.
 */
function layer(band) {
	for (const cluster of band.clusters) {
		const drawn = known.get(cluster.id);
		if (drawn) {
			// A layout that moved without the text changing moves the span
			// rather than replacing it, so a selection in it survives.
			drawn.style.left = cluster.x + 'px';
			drawn.style.top = cluster.y + 'px';
			drawn.style.fontSize = (cluster.height * 0.75) + 'px';
			continue;
		}
		const span = document.createElement('span');
		span.textContent = cluster.text;
		span.style.left = cluster.x + 'px';
		span.style.top = cluster.y + 'px';
		span.style.fontSize = (cluster.height * 0.75) + 'px';
		span.dataset.start = cluster.source_start;
		span.dataset.end = cluster.source_end;
		known.set(cluster.id, span);
		text.insertBefore(span, firstSpanAfter(cluster.source_start));
	}
}

/** The last span whose source offset is at or before one. */
function spanFor(offset, first = false) {
	let low = 0;
	let high = text.children.length;
	while (low < high) {
		const middle = (low + high) >> 1;
		if (Number(text.children[middle].dataset.start) <= offset) low = middle + 1;
		else high = middle;
	}
	let index = Math.max(0, low - 1);
	if (first) {
		while (index > 0 && text.children[index - 1].dataset.start === text.children[index].dataset.start) index -= 1;
	}
	return text.children[index];
}

/** The first span whose source offset is beyond one, for insertion. */
function firstSpanAfter(offset) {
	let low = 0;
	let high = text.children.length;
	while (low < high) {
		const middle = (low + high) >> 1;
		const span = text.children[middle];
		if (Number(span.dataset.start) <= offset) low = middle + 1;
		else high = middle;
	}
	return text.children[low] || null;
}

/** The drawn rows in reading order, which is the order of their positions. */
function drawnRows() {
	if (rows.length === 0) return [];
	return rows.slice().sort(function (a, b) { return a.y - b.y; });
}

/**
 * Asks for every band the viewport shows, and only those.
 *
 * A scroll offset is not a multiple of the viewport height, so the visible
 * rectangle generally spans two bands; asking for only the one it starts in
 * would leave the last part of the screen unpainted. A band already on screen
 * is not asked for again.
 */
function ask() {
	if (!state) return;
	const view = viewHeight();
	if (!view) return;
	const top = scrollY();
	const first = firstBand(top, view);
	const last = lastBand(top, view);
	for (let index = first; index <= last; index += 1) {
		if (resident.has(index) || pending.has(index)) continue;
		pending.add(index);
		vscode.postMessage({
			tile: index * view, band: index, generation: shown, epoch,
			width: state.width, height: view, scale: rasterScale(),
		});
	}
	prune(first, last);
	present();
	acknowledgePaint();
	report();
}

async function place(data) {
	if (data.generation !== shown || data.epoch !== epoch) return;
	// A band remains pending while its PNG decodes.
	if (data.failed) { pending.delete(data.band); return; }
	// A band answers the document it was asked for, and a panel that has
	// moved on to another one must not draw it.
	if (!state || data.id !== state.id) return;
	if (data.height !== viewHeight() || data.scale !== rasterScale()) {
        vscode.postMessage({ tileError: 'Tile geometry changed: ' + JSON.stringify({height:data.height, view:viewHeight(), scale:data.scale, expected:rasterScale()}) });
        return;
    }
	const image = document.createElement('img');
	const url = URL.createObjectURL(new Blob([data.png], { type: 'image/png' }));
	image.src = url;
	try { await image.decode(); } catch (error) {
        vscode.postMessage({ tileError: "PNG decode: " + String(error) });
		if (data.generation === shown && data.epoch === epoch) pending.delete(data.band);
		return;
	} finally { URL.revokeObjectURL(url); }
	if (data.generation !== shown || data.epoch !== epoch) return;
	pending.delete(data.band);
	image.style.top = data.scroll + 'px';
	image.style.height = data.height + 'px';
	image.dataset.band = data.band;
	image.dataset.version = drawnVersion;
	const previous = resident.get(data.band);
	resident.set(data.band, {
		image,
        background: data.background,
		clusters: data.clusters || [],
		rows: data.rows || [],
	});
	if (displayEpoch === epoch) {
		// Decode first, then replace within the same browser task.
		if (previous) previous.image.replaceWith(image);
		else tiles.appendChild(image);
		rows = rows.concat(data.rows || []);
		layer(resident.get(data.band));
	}
	ask();
	if (displayEpoch === epoch && pendingSelection) {
		selectRange(pendingSelection.start, pendingSelection.end);
	}
}

/** Publish a complete visible generation, never a partly decoded frame. */
function present() {
	if (displayEpoch === epoch) return;
	const view = viewHeight();
	for (let i = firstBand(scrollY(), view); i <= lastBand(scrollY(), view); i += 1) {
		if (!resident.has(i)) return;
	}
    const background = resident.get(firstBand(scrollY(), view)).background;
    if (background) document.body.style.backgroundColor = 'rgb(' + background.join(',') + ')';
	tiles.replaceChildren(...[...resident.values()].map(band => band.image));
	rows = [];
	for (const band of resident.values()) {
		rows = rows.concat(band.rows);
		layer(band);
	}
	displayEpoch = epoch;
	text.inert = false;
	if (pendingSelection) selectRange(pendingSelection.start, pendingSelection.end);
}

function acknowledgePaint() {
    if (displayEpoch !== epoch || paintedEpoch === epoch) return;
    const painted = epoch;
    requestAnimationFrame(() => requestAnimationFrame(() => {
        if (painted !== epoch || paintedEpoch === epoch) return;
        const view = viewHeight();
        for (let i = firstBand(scrollY(), view); i <= lastBand(scrollY(), view); i += 1) {
            if (!resident.has(i)) return;
        }
        paintedEpoch = epoch;
        vscode.postMessage({
            paintedVersion: drawnVersion, generation: shown,
            background: getComputedStyle(document.body).backgroundColor,
            tileBounds: [...resident.values()].map(({ image }) => ({
                width: image.getBoundingClientRect().width,
                height: image.getBoundingClientRect().height,
                expectedHeight: parseFloat(image.style.height) * fit,
                pixels: image.naturalHeight,
                expectedPixels: Math.ceil(parseFloat(image.style.height) * rasterScale()),
                scale: window.devicePixelRatio, fit, viewportWidth: viewport.clientWidth, documentWidth: state.width,
                inset: (() => {
                    const box = viewport.getBoundingClientRect();
                    return [box.top, window.innerWidth - box.right, window.innerHeight - box.bottom, box.left];
                })(),
            })),
        });
    }));
}

viewport.addEventListener('scroll', () => {
    persist();
	ask();
	const top = rowAt(scrollY());
	// The source offset and the distance into its row travel with the scroll,
	// so the reader's anchor is never half of one report and half of another.
	vscode.postMessage({
		scroll: scrollY(),
		source: top ? top.source_start : -1,
		into: top ? scrollY() - top.y : 0,
	});
});
/**
 * What the reader clicked on.
 *
 * A link wins over the text under it, because that is what the engine drew
 * there; otherwise the byte is the cluster whose rectangle holds the point.
 * A click that ends a selection is the reader selecting, not pointing.
 */
viewport.addEventListener('click', function (event) {
	if (displayEpoch !== epoch) return;
	if (String(window.getSelection()) !== '') return;
	const link = linkAt(event.clientX, event.clientY);
	if (link) {
		vscode.postMessage({
			link: link.kind,
			linkTarget: link.target,
			linkUrl: link.url,
		});
		return;
	}
	const cluster = clusterAt(event.clientX, event.clientY);
	if (cluster) {
		vscode.postMessage({
			reveal: { start: cluster.source_start, end: cluster.source_end },
		});
	}
});

/** The document coordinates of a point on the page. */
function point(x, y) {
	const box = viewport.getBoundingClientRect();
	return { x: (x - box.left) / fit, y: (y - box.top) / fit + scrollY() };
}

/** The link whose rectangle holds a point, if the engine drew one there. */
function linkAt(clientX, clientY) {
	if (!state) return undefined;
	const at = point(clientX, clientY);
	for (const block of state.blocks) {
		const links = block.links || [];
		for (const link of links) {
			const y = block.y + link.y;
			if (
				link.x <= at.x && at.x < link.x + link.width &&
				y <= at.y && at.y < y + link.height
			) {
				return link;
			}
		}
	}
	return undefined;
}

/** The cluster whose rectangle holds a point, over the bands on screen. */
function clusterAt(clientX, clientY) {
	const at = point(clientX, clientY);
	for (const band of resident.values()) {
		for (const cluster of band.clusters) {
		if (
				cluster.x <= at.x && at.x < cluster.x + cluster.width &&
				cluster.y <= at.y && at.y < cluster.y + cluster.height
			) {
				return cluster;
			}
		}
	}
	return undefined;
}

/** Selects a source range in the text over the bands on screen. */
function selectRange(start, end) {
	text.style.visibility = "";
	const first = spanFor(start, true);
	// The last span is the one holding the character before the end, so a
	// range ending where the next glyph begins does not take that glyph too.
	const last = spanFor(Math.max(start, end - 1)) || first;
	const holds = (span, offset) =>
		span &&
		Number(span.dataset.start) <= offset &&
		offset < Number(span.dataset.end);
	// Only spans that carry the requested bytes can be selected. A range in a
	// part of the document that has not been drawn yet is kept until its band
	// arrives rather than selecting whatever span happens to be nearest, which
	// would show the reader text they did not select.
	if (!holds(first, start) || !holds(last, Math.max(start, end - 1))) {
		pendingSelection = { start, end };
		return;
	}
	pendingSelection = null;
	const range = document.createRange();
	range.setStart(first.firstChild || first, 0);
	range.setEnd(last.firstChild || last, (last.textContent || '').length);
	const selection = window.getSelection();
	selection.removeAllRanges();
	selection.addRange(range);
	vscode.postMessage({ selection: selected() || null, spans: text.children.length });
}

/** The source range the nth character of the rendered text was drawn from. */
function rangeAt(index) {
	let seen = 0;
	for (let position = 0; position < ranges.length; position += 1) {
		const next = seen + (ranges[position][2] || 0);
		if (index < next) return ranges[position];
		seen = next;
	}
	return undefined;
}

/** What the reader has selected, and the bytes it was set from. */
function selected() {
	const selection = window.getSelection();
	if (!selection || selection.rangeCount === 0) return undefined;
	const value = String(selection);
	if (value === '') return undefined;
	const range = selection.getRangeAt(0);
	const span = range.startContainer.parentElement;
	return {
		text: value,
		source_start: span && span.dataset ? Number(span.dataset.start) : -1,
	};
}

/** Uses the same Chromium find API as VS Code's webview find widget. */
function findIn(needle, previous = false) {
	text.style.visibility = "hidden";
	if (!source.firstChild) {
		pendingFind = { needle, previous };
		return false;
	}
	return window.find(needle, false, previous, true, false, false, false);
}

// The workbench find field takes focus outside this frame. Hide the duplicate
// selection overlay before Chromium searches, including after a drag selection.
window.addEventListener('blur', () => { text.style.visibility = 'hidden'; });
window.addEventListener('focus', () => { text.style.visibility = ''; });

document.addEventListener('selectionchange', function () {
	// A selection inside the findable text is the find widget landing on a
	// match: the passage it names is what the reader asked for, so the
	// preview goes there and the editor shows it.
	const active = window.getSelection();
	const anchor = active && active.anchorNode;
	if (anchor && source.contains(anchor) && !active.isCollapsed) {
		// Only one copy of the text participates in browser find.
		text.style.visibility = 'hidden';
		const from = active.anchorOffset;
		const to = active.focusOffset;
		const first = Math.min(from, to);
		// A selection's end is exclusive: the last character it includes is
		// the one before it, and a match ending at the end of the text has no
		// cluster after it to ask for.
		const last = Math.max(from, to) - 1;
		const start = rangeAt(first);
		const end = last >= first ? rangeAt(last) : undefined;
		if (start && end) {
			vscode.postMessage({
				reveal: { start: start[0], end: end[1] },
				find: true,
			});
		}
		return;
	}
	text.style.visibility = '';
	vscode.postMessage({ selection: selected() || null, spans: text.children.length });
});

// The bands are viewport-tall, so a resize makes every one of them stale.
let resizingTop;
let resizeEnd;
window.addEventListener('resize', () => {
    // Keep one native anchor through animated resizing, avoiding cumulative
    // rounding of the browser's physical scroll position at each step.
    resizingTop ??= scrollY();
    fitPage(resizingTop);
    forget(); ask();
    clearTimeout(resizeEnd);
    resizeEnd = setTimeout(() => { resizingTop = undefined; }, 150);
});
viewport.addEventListener('wheel', () => { resizingTop = undefined; }, { passive: true });
// Moving between monitors can change density without changing CSS dimensions.
function watchDensity() {
    matchMedia('(resolution: ' + window.devicePixelRatio + 'dppx)').addEventListener('change', () => {
        forget(); ask(); watchDensity();
    }, { once: true });
}
watchDensity();
window.addEventListener('message', async (event) => {
	const message = event.data;
	if (message.state) {
		// A resend is the host answering a webview that had not run when the
		// state was first sent: one that already holds a state needs nothing.
		if (message.resend && state) return;
		const changedDocument = state?.id !== message.state.id;
		state = message.state;
		shown = message.generation;
		fitPage(restoredTop ?? scrollY());
        restoredTop = undefined;
        persist();
		// The drawn text belongs to the text the state replaced, so it goes
		// with it — but only when the text changed: a layout that moved
		// without an edit leaves what is drawn where it is, and the spans that
		// arrive next move to their new positions.
		if (changedDocument || (typeof message.version === 'number' && message.version !== drawnVersion)) {
			forgetText();
			drawnVersion = message.version;
			source.textContent = '';
			ranges = [];
		}
		forget(changedDocument);
		ask();
	}
	if (message.notice) notice.textContent = message.notice;
	if (message.tile) place(message.tile);
	if (message.scroll !== undefined) viewport.scrollTop = message.scroll * fit;
	if (message.select) selectRange(message.select.start, message.select.end);
	if (message.redraw) {
		forget();
		ask();
	}
	if (message.findText && message.generation === shown) {
		source.textContent = message.findText.text;
		ranges = message.findText.ranges;
		vscode.postMessage({ rendered: source.textContent });
		report();
		if (pendingFind !== undefined) {
			const waiting = pendingFind;
			pendingFind = undefined;
			findIn(waiting.needle, waiting.previous);
		}
	}
	if (message.find) findIn(message.find, message.previous);
	if (message.clearSelection) {
		// A click that ends a selection is the reader selecting rather than
		// pointing, so a host that wants to point clears it first.
		window.getSelection().removeAllRanges();
	}
	if (message.clickAt) {
		// A test cannot deliver a mouse to a webview, so the panel can ask for
		// one here, in document coordinates. It is dispatched at the element
		// under the point and goes through the same listener a click does.
		const box = viewport.getBoundingClientRect();
		const clientX = box.left + message.clickAt.x * fit;
		const clientY = box.top + (message.clickAt.y - scrollY()) * fit;
		const target = document.elementFromPoint(clientX, clientY);
		if (target) {
			target.dispatchEvent(
				new MouseEvent('click', { clientX, clientY, bubbles: true }),
			);
		}
		// What the point resolved to, so a host can tell a click that landed
		// from one that fell beside what it aimed at.
		const landed = linkAt(clientX, clientY);
		const cluster = landed ? undefined : clusterAt(clientX, clientY);
		vscode.postMessage({
			clicked: landed
				? { kind: landed.kind, url: landed.url, target: landed.target }
				: cluster
					? { cluster: cluster.source_start }
					: null,
			element: Boolean(target),
		});
	}
	if (message.dragAt) {
		// A drag is the browser's own gesture, so its ends are resolved the
		// way the browser resolves them: by hit-testing the point against the
		// drawn spans. Doing that here is the closest a test can come to
		// delivering the gesture, and the selection it builds is the same one.
		const box = viewport.getBoundingClientRect();
		const at = function (point) {
			return document.caretRangeFromPoint(
				box.left + point.x * fit,
				box.top + (point.y - scrollY()) * fit,
			);
		};
		const from = at(message.dragAt.from);
		const to = at(message.dragAt.to);
		if (from && to) {
			const range = document.createRange();
			range.setStart(from.startContainer, from.startOffset);
			range.setEnd(to.startContainer, to.startOffset);
			const selection = window.getSelection();
			selection.removeAllRanges();
			selection.addRange(range);
		}
	}
	if (message.copySelection) {
        const selection = window.getSelection();
        // Copy the selected engine text even while focus is in a host menu.
        const copyText = selection?.rangeCount ? selection.getRangeAt(0).toString() : '';
        vscode.postMessage({ copyText });
    }
});
// The panel is ready once its own document has run: the host learns the
// scrollable extent from here, and again whenever the state resizes it.
vscode.postMessage({ ready: true });
ask();
</script></body></html>`;
}

let current:
	| {
			document: vscode.TextDocument;
			session: Session;
			id: string;
			/** The block map, in the editor's own offset units. */
			blocks: Block[];
			/** The rows of each band the panel has been given. */
			bands: Map<number, Row[]>;
			/** The key the offset table was built for, and the table. */
			tableKey?: string;
			table?: Int32Array;
	  }
	| undefined;

/**
 * The engine counts source offsets in UTF-8 bytes and the editor counts them
 * in UTF-16 code units. They agree on ASCII and diverge on everything else, so
 * every offset that crosses between the two is translated here and the rest of
 * the panel works in the editor's units.
 */
function utf16Of(table: Int32Array | undefined, byte: number): number {
	if (!table) {
		return byte;
	}
	let low = 0;
	let high = table.length / 2 - 1;
	while (low < high) {
		const middle = Math.ceil((low + high) / 2);
		if (table[middle * 2] <= byte) {
			low = middle;
		} else {
			high = middle - 1;
		}
	}
	return table[low * 2 + 1] + (byte - table[low * 2]);
}

/** Pairs of byte offset and code-unit offset, one pair per character. */
function unitTable(text: string): Int32Array | undefined {
	if (!/[^\x00-\x7f]/.test(text)) {
		return undefined;
	}
	const pairs: number[] = [];
	let bytes = 0;
	let units = 0;
	for (const character of text) {
		pairs.push(bytes, units);
		bytes += Buffer.byteLength(character, "utf8");
		units += character.length;
	}
	pairs.push(bytes, units);
	return Int32Array.from(pairs);
}

/** The table for a document, rebuilt whenever its text changes. */
function tableFor(document: vscode.TextDocument): Int32Array | undefined {
	const key = `${document.uri.toString()}@${document.version}`;
	if (current?.tableKey !== key) {
		if (current) {
			current.tableKey = key;
			current.table = unitTable(document.getText());
		}
	}
	return current?.table;
}

/** One state, with its offsets taken into the editor's units. */
function adopt(state: State): State {
	const table = current ? tableFor(current.document) : undefined;
	return {
		...state,
		// The server uses the requested column width but omits it from state.
		width: Number(report.settings.width ?? 760),
		blocks: state.blocks.map((block) => ({
			...block,
			source_start: utf16Of(table, block.source_start),
			source_end: utf16Of(table, block.source_end),
		})),
	};
}

/** One band's text, with its offsets taken into the editor's units. */
function adoptClusters(clusters: Cluster[]): Cluster[] {
	const table = current ? tableFor(current.document) : undefined;
	return clusters.map((cluster) => ({
		...cluster,
		source_start: utf16Of(table, cluster.source_start),
		source_end: utf16Of(table, cluster.source_end),
	}));
}

/** One band's rows, with their offsets taken into the editor's units. */
function adoptRows(rows: Row[]): Row[] {
	const table = current ? tableFor(current.document) : undefined;
	return rows.map((row) => ({
		...row,
		source_start: utf16Of(table, row.source_start),
		source_end: utf16Of(table, row.source_end),
	}));
}

/**
 * Opens a document in the engine and answers with its id and its geometry.
 *
 * One request opens it, so a host that has just been given a document's
 * geometry has not silently asked for it twice.
 */
async function openInEngine(
	session: Session,
	document: vscode.TextDocument,
): Promise<{ id: string; state: State; version: number; settings: ReturnType<typeof settingsOf> }> {
	layoutRequest += 1;
	const key = document.uri.toString();
	const existing = ids.get(key);
	const id = existing ?? `doc-${sequence++}`;
	const rules = await templateFor({}, document);
	const settings = settingsOf(document);
	const version = document.version;
	const state = await session.open(id, document.getText(), {
		path: document.uri.fsPath,
		...rules,
		settings,
	});
	ids.set(key, id);
	report.opens += 1;
	return { id, state, version, settings };
}

function settingsOf(document: vscode.TextDocument) {
	// The document, rather than its URI: the editor resolves a `[language]`
	// override from the scope's language, which only a document carries.
	const configuration = vscode.workspace.getConfiguration(
		"markview",
		document,
	);
	return {
		font_size: configuration.get<number>("fontSize"),
		width: configuration.get<number>("columnWidth"),
		justify: configuration.get<boolean>("justify"),
		hyphenate: configuration.get<boolean>("hyphenate"),
		paragraph_indent: configuration.get<number>("paragraphIndent"),
		codeblock_wrap: configuration.get<boolean>("codeblockWrap"),
	};
}

/** Sends the editor's text to the engine, coalescing a burst of keystrokes. */
function schedule(session: Session, document: vscode.TextDocument): void {
	if (!showing(document)) {
		return;
	}
	// The edit is what holds the two surfaces still, and it starts here: the
	// editor reports its view moving under the text before the engine has
	// even been asked for the new layout.
	hold();
	if (coalescing) {
		clearTimeout(coalescing);
	}
	coalescing = setTimeout(() => {
		void (async () => {
			// The panel may have moved to another document while the burst
			// settled, and this document's text is no longer what it shows.
			if (!showing(document)) {
				return;
			}
			const { state, version, settings } = await openInEngine(session, document);
			if (!showing(document) || !current || version !== document.version) {
				return;
			}
			report.settings = settings;
			const adopted = adopt(state);
			current.blocks = adopted.blocks;
			current.bands.clear();
			rememberAnchors(adopted);
						report.documentHeight = state.height;
			// An edit is not a scroll: what the editor reports while a
			// document is being relaid out is the reader's place moving
			// under it, and carrying that would undo the place the preview
			// has just been put back into.
			hold();
			await publish(adopted);
			await restoreAnchor();
			scheduleRenderedText();
		})();
	}, COALESCE_MS);
}

/** Whether the panel is showing this document. */
function showing(document: vscode.TextDocument): boolean {
	return current?.document.uri.toString() === document.uri.toString();
}

/**
 * Hands the webview a document's geometry.
 *
 * The pixels and the text over them travel together, band by band, so the
 * webview is never asked to draw a screenful whose text it does not have. A
 * new state therefore clears what is on screen and the bands are asked for
 * again; the scroll offset is the webview's own and does not move, which is
 * what keeps a keystroke from moving the reader's place.
 */
async function publish(state: unknown): Promise<void> {
	// The state being published already holds every edit so far, so a report
	// against it needs no shifting and the ones before it need the shifts
	// that have been accumulated since they were published.
	shownState = state as State;
	generation += 1;
	shifts.set(generation, 0);
	for (const held of [...shifts.keys()]) {
		if (held < generation - 2) {
			shifts.delete(held);
		}
	}
	await post({
		state,
		version: current?.document.version,
		generation,
	});
}

/** Holds the two surfaces still while an edit is laid out. */
function hold(): void {
	updatingUntil = Date.now() + UPDATE_MS;
	if (holdTimer) {
		clearTimeout(holdTimer);
	}
	holdTimer = setTimeout(release, UPDATE_MS);
}

/**
 * Carries whatever the reader did while the surfaces were held.
 *
 * A scroll during the hold is the reader's, not the layout's, so it is kept
 * and carried once the edit has settled. The editor's own report is not: the
 * text moved under it, and if it is still showing the same place as the
 * preview there is nothing to carry.
 */
function release(): void {
	holdTimer = undefined;
	const shown = current;
	const pending = deferred;
	deferred = {};
	if (!shown) {
		return;
	}
	if (pending.preview) {
		// The preview has already been put back where the reader left it in
		// the new text, so what is carried is what it is showing now rather
		// than the offset it was at before the edit moved everything below it.
		void carryPreview(report.scroll);
		return;
	}
	if (pending.editor) {
		const editor = editorFor(shown);
		const top = editor?.visibleRanges[0]?.start;
		if (editor && top) {
			void carryEditor(editor.document.offsetAt(top));
		}
	}
}

/** Moves the reader's anchor with the text an edit inserted or removed. */
function shiftAnchor(
	changes: readonly vscode.TextDocumentContentChangeEvent[],
): number {
	if (!anchor) {
		return 0;
	}
	let moved = 0;
	for (const change of changes) {
		const end = change.rangeOffset + change.rangeLength;
		if (end <= anchor.offset) {
			// The edit is entirely before the reader, so the text they were on
			// has moved by exactly what the edit added or removed.
			const delta = change.text.length - change.rangeLength;
			anchor.offset += delta;
			moved += delta;
		} else if (change.rangeOffset < anchor.offset) {
			// The edit reaches across the reader: the text they were on is
			// gone, and the nearest survivor is the end of what replaced it.
			const delta = change.rangeOffset + change.text.length - anchor.offset;
			anchor.offset = change.rangeOffset + change.text.length;
			moved += delta;
		}
	}
	return moved;
}

/**
 * Remembers where this layout put each fragment, and where each link is.
 *
 * The rectangles are the panel's own — block position plus the link's own —
 * which is what a click is hit-tested against, so a host can point at a link
 * without laying the document out a second time to guess where it went.
 */
function rememberAnchors(state: State): void {
	const anchors: Record<string, number> = {};
	const links: Array<Record<string, unknown>> = [];
	for (const block of state.blocks) {
		for (const link of block.links ?? []) {
			if (link.kind === "anchor" && typeof link.to === "number") {
				anchors[link.url] = link.to;
			} else if (
				link.kind === "anchor" &&
				typeof report.anchors[link.url] === "number"
			) {
				// A layout that has not settled may not have resolved the
				// fragment yet. The place it was last found is better than
				// dropping it, which would leave a click on the link with
				// nowhere to go until the next message happens to carry it.
				anchors[link.url] = report.anchors[link.url];
			}
			links.push({
				kind: link.kind,
				url: link.url,
				target: link.target,
				to: link.to ?? null,
				x: link.x,
				y: block.y + link.y,
				width: link.width,
				height: link.height,
			});
		}
	}
	report.anchors = anchors;
	report.links = links;
}

/**
 * The last state sent to the webview.
 *
 * A panel is made a moment before its own document runs, so the state sent
 * while it was loading can have gone nowhere; keeping it lets the host answer
 * the webview's own "ready" with it rather than leaving a blank surface.
 */
let shownState: State | undefined;

/** The editor showing the document the panel is previewing, if it is open. */
function editorFor(shown: {
	document: vscode.TextDocument;
}): vscode.TextEditor | undefined {
	return vscode.window.visibleTextEditors.find(
		(candidate) =>
			candidate.document.uri.toString() === shown.document.uri.toString(),
	);
}

/**
 * How long a move the panel made is left to be answered.
 *
 * The editor reveals a line and then reports where its view actually landed,
 * which is wherever the editor decided to put it: a few lines away at one text
 * size and half a screen away at another. That report is the answer to the
 * move, not a move of the reader, and carrying it back is what undid the
 * restored reading position. It cannot be recognised by distance — the
 * distance depends on the text size, the wrapping and the height of the
 * editor — so it is recognised by being what the panel is waiting for.
 */
const ECHO_MS = 600;
/**
 * The byte the panel asked the editor to show, if a reveal is unanswered.
 *
 * A reveal is answered by the editor reporting a view with that byte in it.
 * That is what makes the answer identifiable: it is the report that contains
 * what the panel asked for, not the first report to arrive or the one that
 * arrives soonest. A report that does not contain it is the reader's move, and
 * is carried as usual.
 */
let expectReveal = -1;
/** Where that answer landed, and how long the same landing is not a move. */
let revealedAt: number | undefined;
let revealGraceUntil = 0;
/** A scroll the panel made in the preview, whose report is its own answer. */
let previewEcho: { y: number; until: number } | undefined;

/** Coalesce scroll updates while invalidating older asynchronous mappings. */
let scrollSequence = 0;
let scrollTimer: ReturnType<typeof setTimeout> | undefined;
let pendingScroll: (() => Promise<void>) | undefined;
let previewRow: number | undefined;
function scheduleScroll(move: (sequence: number) => Promise<void>): void {
	const sequence = ++scrollSequence;
	pendingScroll = () => move(sequence);
	if (scrollTimer) return;
	scrollTimer = setTimeout(() => {
		scrollTimer = undefined;
		const pending = pendingScroll;
		pendingScroll = undefined;
		void pending?.();
	}, 50);
}

/** The block a position in the document falls in. */
function blockAt(
	blocks: readonly Block[],
	holds: (block: Block) => boolean,
): Block | undefined {
	return blocks.find(holds);
}

/**
 * The block a source offset belongs to.
 *
 * A blank line between two paragraphs belongs to neither of them, so an
 * offset in one is answered with the block that comes next rather than with
 * nothing: that is the text the editor's top line is above.
 */
function blockFor(
	blocks: readonly Block[],
	offset: number,
): Block | undefined {
	return (
		blockAt(
			blocks,
			(candidate) =>
				candidate.source_start <= offset && offset < candidate.source_end,
		) ?? blockAt(blocks, (candidate) => candidate.source_start >= offset)
	);
}

/** The block a vertical position in the document falls in. */
function blockShown(blocks: readonly Block[], y: number): Block | undefined {
	return blockAt(
		blocks,
		(candidate) => candidate.y <= y && y < candidate.y + candidate.height,
	);
}

/**
 * Puts the preview back where the reader was in a new layout.
 *
 * The block the anchor names has moved with the text, and the row inside it is
 * found by the engine rather than guessed, so an edit deep inside a block of
 * many screens restores to the row the reader was reading.
 */
async function restoreAnchor(): Promise<void> {
	const held = anchor;
	if (!held) {
		return;
	}
	// A search that does not find the row still leaves the reader in the block
	// they were reading rather than wherever the state change left the view.
	const block = blockFor(current?.blocks ?? [], held.offset);
	const y = (await yOf(held.offset)) ?? block?.y;
	if (y === undefined) {
		return;
	}
	report.source = held.offset;
	report.into = held.into;
	report.scroll = y + held.into;
	previewEcho = { y: report.scroll, until: Date.now() + ECHO_MS };
	void post({ scroll: report.scroll });
}

/** How far around a guess the engine is asked for the rows of a band. */
const SEARCH_PX = 900;

/**
 * The position a source offset is drawn at.
 *
 * The rows of the bands on hand answer for the offsets inside them, and an
 * offset outside them is placed by asking the engine for the band around where
 * the block map says it should be. That is what lets a target deep inside a
 * block of many screens be found at all: the block start is not the answer,
 * and the rows that say where the offset is drawn could not otherwise be
 * reached.
 */
async function yOf(offset: number): Promise<number | undefined> {
	const shown = current;
	if (!shown) {
		return undefined;
	}
	for (const rows of shown.bands.values()) {
		const row = rows.find(
			(candidate) =>
				candidate.source_start <= offset && offset < candidate.source_end,
		);
		if (row) {
			return row.y;
		}
	}
	const block = blockFor(shown.blocks, offset);
	if (!block) {
		return undefined;
	}
	// Blank lines and leading Markdown syntax follow the next native row.
	if (offset <= block.source_start) {
		const layer = await shown.session.text(shown.id, block.y, block.y + SEARCH_PX);
		const rows = adoptRows(layer.rows);
		shown.bands.set(block.y, rows);
		return rows.find(row => row.source_start >= offset)?.y;
	}
	// The block is searched using the text density of the band the last probe
	// returned: source units per pixel over that band say how far to step for
	// the bytes still missing, which converges even when one line of a block
	// holds most of its bytes. Guessing by a block's overall proportion does
	// not, and returning the guess without finding the row would lose the
	// reader's place rather than move it.
	// The search keeps a verified interval of positions: the rows it has seen
	// put the target above `low` or below `high`, and every probe either finds
	// the row or narrows the interval. It steps by the text density of the band
	// it last saw — which crosses one line holding most of a block's bytes in a
	// single step — and falls back to the middle of the interval whenever that
	// step would leave it, so the search cannot stall or overshoot forever.
	const span = block.source_end - block.source_start;
	const through = span > 0 ? (offset - block.source_start) / span : 0;
	let low = block.y;
	let high = block.y + block.height;
	let guess = block.y + Math.max(0, Math.min(1, through)) * block.height;
	for (let attempt = 0; attempt < 16 && high - low > 1; attempt += 1) {
		if (!(guess > low && guess < high)) {
			guess = (low + high) / 2;
		}
		const top = Math.max(0, guess - SEARCH_PX);
		const layer = await shown.session.text(shown.id, top, guess + SEARCH_PX);
		const rows = adoptRows(layer.rows);
		shown.bands.set(top, rows);
		const row = rows.find(
			(candidate) =>
				candidate.source_start <= offset && offset < candidate.source_end,
		);
		if (row) {
			return row.y;
		}
		const first = rows[0];
		const last = rows[rows.length - 1];
		if (!first || !last) {
			break;
		}
		// An offset between the rows drawn around it — on a blank line, or
		// between two clusters — belongs with the last row at or above it.
		if (first.source_start <= offset && offset < last.source_end) {
			const below = rows.filter(
				(candidate) => candidate.source_start <= offset,
			);
			return below.length > 0 ? below[below.length - 1].y : first.y;
		}
		const bandBytes = last.source_end - first.source_start;
		const bandPixels = last.y + last.height - first.y;
		const density = bandPixels > 0 ? bandBytes / bandPixels : 0;
		if (offset < first.source_start) {
			high = Math.min(high, first.y);
			guess = density > 0 ? first.y - (first.source_start - offset) / density : low;
		} else {
			low = Math.max(low, last.y + last.height);
			guess = density > 0 ? last.y + (offset - last.source_end) / density : high;
		}
		guess = Math.max(low, Math.min(high, guess));
	}
	return undefined;
}

/**
 * The row drawn at a position.
 *
 * The bands on hand answer for the positions inside them, and a position
 * outside them is asked of the engine: one fetch of the band around it is
 * enough, because the position is inside that band by construction.
 */
async function rowAtY(y: number): Promise<Row | undefined> {
	const shown = current;
	if (!shown) {
		return undefined;
	}
	for (const rows of shown.bands.values()) {
		const row = rows.find(
			(candidate) => candidate.y <= y && y < candidate.y + candidate.height,
		);
		if (row) {
			return row;
		}
	}
	const top = Math.max(0, y - SEARCH_PX);
	const layer = await shown.session.text(shown.id, top, y + SEARCH_PX);
	const rows = adoptRows(layer.rows);
	shown.bands.set(top, rows);
	// Paragraph spacing has no text row; follow the next native row.
	return rows.find(candidate => y < candidate.y + candidate.height) ?? rows.at(-1);
}

/** Aligns the editor with the preview's native row without moving the caret. */
async function carryPreview(y: number, sequence = ++scrollSequence): Promise<void> {
	const shown = current;
	if (!shown || !syncs(shown)) {
		return;
	}
	// The byte at the top of the preview is the engine's to name, not the
	// webview's: the rows over the band may not have arrived yet, and the
	// block's own start is nowhere near a position deep inside it.
	if (
		previewEcho &&
		Date.now() < previewEcho.until &&
		Math.abs(y - previewEcho.y) <= 2
	) {
		// The panel's own scroll, coming back: not a move of the reader's.
		previewEcho = undefined;
		report.stoodDown += 1;
		return;
	}
	const row = y <= 0 ? { source_start: 0, y: 0 } : await rowAtY(y);
	if (sequence !== scrollSequence || shown !== current) return;
	if (!row) {
		report.unmapped += 1;
		return;
	}
	// The mapped row also anchors gaps before their pixels have arrived.
	report.source = row.source_start;
	report.into = y - row.y;
	anchor = { offset: row.source_start, into: report.into };
	if (previewRow === row.source_start) return;
	previewRow = row.source_start;
	report.syncs.preview += 1;
	report.carried = row.source_start;
	reveal(shown, row.source_start, row.source_start);
}

/**
 * Moves the preview to what the editor is showing.
 *
 * The preview is told which source offset to show rather than where to put
 * its view: it holds the rows that say where that offset is drawn, and the
 * host would only be guessing from a block map.
 */
/** Whether the reader asked the two surfaces to follow each other. */
function syncs(shown: { document: vscode.TextDocument }): boolean {
	return (
		vscode.workspace
			.getConfiguration("markview", shown.document)
			.get<boolean>("scrollSync") ?? true
	);
}

async function carryEditor(offset: number): Promise<void> {
	const sequence = ++scrollSequence;
	const shown = current;
	const y = offset === 0 ? 0 : await yOf(offset);
	if (sequence === scrollSequence && shown === current) await carryEditorAt(offset, y);
}

/** Moves the preview to a position the editor is showing, already resolved. */
async function carryEditorAt(
	offset: number,
	there: number | undefined,
): Promise<void> {
	const shown = current;
	if (!shown || !syncs(shown)) {
		return;
	}
	// The source origin is a viewport boundary, even when its Markdown
	// syntax or leading blank lines have no rendered row.
	if (there === undefined || (offset === 0
		? report.scroll === 0
		: Math.abs(there - report.scroll) <= 1)) {
		report.stoodDown += 1;
		return;
	}
	// A move the reader made is the last thing the editor showed, so the
	// landing the panel remembered is no longer what an editor report of that
	// position means. Returning to it is a move like any other.
	revealedAt = undefined;
	previewRow = undefined;
	report.source = offset;
	report.into = 0;
	anchor = offset === 0 ? undefined : { offset, into: 0 };
	report.syncs.editor += 1;
	report.scroll = there;
	previewEcho = { y: there, until: Date.now() + ECHO_MS };
	void post({ scroll: there });
}

export function openPreview(
    context: vscode.ExtensionContext,
    session: Session,
    document: vscode.TextDocument,
    restored?: { panel: vscode.WebviewPanel; scroll: number },
    follow = false,
): Promise<void> {
    const request = ++openRequest;
    requestedDocument = document;
    followingSession = session;
    // Serialize engine opens and skip superseded tab changes.
    const task = opening.then(() => {
        if (request === openRequest) return showPreview(context, session, document, request, restored, follow);
    });
    opening = task.catch(() => {});
    return task;
}

async function showPreview(
    context: vscode.ExtensionContext,
    session: Session,
    document: vscode.TextDocument,
    request: number,
    restored: { panel: vscode.WebviewPanel; scroll: number } | undefined,
    follow: boolean,
): Promise<void> {
	const column = vscode.ViewColumn.Beside;
	if (!panel) {
		panel = restored?.panel ?? vscode.window.createWebviewPanel(
			"markview.preview",
			`Preview ${document.uri.path.split("/").pop() ?? ""}`,
			{ viewColumn: column, preserveFocus: true },
			{
				enableScripts: true,
				retainContextWhenHidden: true,
				// The find widget searches the text layer, which is the
				// document's own characters.
				enableFindWidget: true,
			},
		);
		panel.webview.options = { enableScripts: true };
        let lastEditor = vscode.window.activeTextEditor?.document.uri.toString();
        let followTimer: ReturnType<typeof setTimeout> | undefined;
        const active = vscode.window.onDidChangeActiveTextEditor(() => {
            // Creating/revealing a panel emits transient editor changes too.
            if (followTimer) clearTimeout(followTimer);
            followTimer = setTimeout(() => {
                const editor = vscode.window.activeTextEditor;
                if (panel?.active || !editor) return;
                const uri = editor.document.uri.toString();
                if (uri === lastEditor) return;
                lastEditor = uri;
                if (editor.document.languageId !== "markdown" || uri === requestedDocument?.uri.toString()) return;
                const session = followingSession;
                if (!session) return;
                void openPreview(context, session, editor.document, undefined, true).catch(error => {
                    void vscode.window.showErrorMessage(`Markview: ${String(error)}`);
                });
            }, COALESCE_MS);
        });
		panel.onDidDispose(() => {
            active.dispose();
            if (followTimer) clearTimeout(followTimer);
            ++openRequest;
            requestedDocument = undefined;
            followingSession = undefined;
			panel = undefined;
			panelColumn = undefined;
			shownState = undefined;
			current = undefined;
			unfollow?.();
			unfollow = undefined;
			unwatch();
		});
		// One handler for the panel's life: the webview is reloaded for each
		// document, and a handler registered per document would answer every
		// tile request as many times as documents had been opened.
		panel.webview.onDidReceiveMessage(receive);
		panelColumn = panel.viewColumn;
	context.subscriptions.push(panel);
	}
    // Old notifications must not refill a webview that is loading another file.
    current = undefined;
    shownState = undefined;
    ++layoutRequest;
    unwatch();
    unfollow?.();
    unfollow = undefined;
    if (coalescing) clearTimeout(coalescing);
    coalescing = undefined;
	// A new document has not been drawn yet: what was observed of the last one
	// would answer a test's wait before the new webview has run at all.
	report.ready = false;
    report.tileError = "";
    report.documentUri = document.uri.toString();
	report.paintedVersion = -1;
	report.paintedAt = 0;
	report.live = 0;
	report.viewport = 0;
	report.scrollHeight = 0;
	report.documentHeight = 0;
	report.openedHeight = 0;
	report.blocks = 0;
	report.source = -1;
	report.into = 0;
	report.text = "";
	report.rendered = "";
	report.ordered = true;
	report.unique = true;
	report.findable = { length: 0, head: "", tail: "" };
	report.restored = !!restored;
    panel.title = `Preview ${document.uri.path.split("/").pop() ?? ""}`;
	panel.webview.html = render(document.uri.toString(), restored?.scroll ?? 0);
	// A panel that is behind another tab in its group is shown again, so
	// asking for the preview brings it back rather than reloading it unseen.
	if (!restored && !follow) panel.reveal(panel.viewColumn ?? column, true);

	let opened;
	do {
        opened = await openInEngine(session, document);
        if (request !== openRequest) return;
    }
	while (opened.version !== document.version);
	const { id, state, version, settings } = opened;
    report.settings = settings;
	current = { document, session, id, blocks: [], bands: new Map() };
	anchor = undefined;
	generation += 1;
	shifts.clear();
	shifts.set(generation, 0);
	// A raster that settles moves the blocks under it, so the engine
	// republishes the geometry. The scroll extent is a function of that
	// geometry and has to follow it, which is what the subscription is for.
	unfollow = session.onLayout((moved) => {
		if (!current || moved.id !== current.id) {
			return;
		}
		const adopted = adopt(moved);
		current.blocks = adopted.blocks;
		current.bands.clear();
		rememberAnchors(adopted);
		report.documentHeight = moved.height;
		hold();
		void publish(adopted).then(restoreAnchor).then(scheduleRenderedText);
	});
	const adopted = adopt(state);
	if (current) {
		current.blocks = adopted.blocks;
		current.bands.clear();
	}
	rememberAnchors(adopted);
	report.documentHeight = state.height;
	report.openedHeight = state.height;
	report.blocks = state.blocks.length;
	const incomplete = state.complete ? "" : "Layout in progress…";
	const diagnostics = state.math_errors
		? `${state.math_errors} formula(s) shown as source`
		: "";
	const watcher = vscode.workspace.onDidChangeTextDocument((event) => {
		if (event.document.uri.toString() === document.uri.toString()) {
			const moved = shiftAnchor(event.contentChanges);
			for (const [held, total] of shifts) {
				shifts.set(held, total + moved);
			}
			schedule(session, event.document);
		}
	});
	shownState = adopted;
    // A restored webview starts only after its serializer returns. Its `ready`
    // handshake resends this state, so never await delivery during restoration.
	void post({
		state: adopted,
		version,
		generation,
		notice: [incomplete, diagnostics].filter(Boolean).join(" · "),
	});
	scheduleRenderedText();

	const saved = vscode.workspace.onDidSaveTextDocument(async (saved) => {
		if (saved.uri.toString() === document.uri.toString()) {
			await session.saved(id);
		}
	});
	// What the reader selects in the editor is selected in the preview: the
	// text layer carries the bytes, so the same range is highlighted.
	const chose = vscode.window.onDidChangeTextEditorSelection((event) => {
		if (!showing(event.textEditor.document)) {
			return;
		}
		const range = event.selections[0];
		if (!range || range.isEmpty) {
			return;
		}
        report.selectionRequest = {
            start: event.textEditor.document.offsetAt(range.start),
            end: event.textEditor.document.offsetAt(range.end),
        };
        if (findEcho?.start === report.selectionRequest.start && findEcho.end === report.selectionRequest.end) {
            findEcho = undefined;
            return;
        }
        findEcho = undefined;
        void post({ select: report.selectionRequest });
	});
	// The editor's own scrolling moves the preview: the block holding the
	// first byte it shows is the block the preview puts at the top.
	const scrolled = vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
		if (!current || !showing(event.textEditor.document)) {
			return;
		}
		const range = event.visibleRanges[0];
		if (!range) {
			return;
		}
		report.editorEvents += 1;
		const shown = current;
		scheduleScroll(async (sequence) => {
			const offset = event.textEditor.document.offsetAt(range.start);
			const where = offset === 0 ? 0 : await yOf(offset);
			if (sequence !== scrollSequence || shown !== current) return;
			if (expectReveal >= 0 && Date.now() < revealGraceUntil) {
				const asked = expectReveal;
				expectReveal = -1;
				const shows = event.visibleRanges.some(
					(candidate) =>
						event.textEditor.document.offsetAt(candidate.start) <= asked &&
						asked <= event.textEditor.document.offsetAt(candidate.end),
				);
				if (shows) {
					// The answer to the reveal: the editor is reporting a view with
					// the byte the panel asked for in it, not a move of the
					// reader's. A report that does not show it is their move, and
					// falls through to be carried.
					revealedAt = where;
					revealGraceUntil = Date.now() + ECHO_MS;
					report.stoodDown += 1;
					return;
				}
			}
			if (
				revealedAt !== undefined &&
				Date.now() < revealGraceUntil &&
				where !== undefined &&
				Math.abs(where - revealedAt) <= 1
			) {
				// The same landing reported again, still not a move of the
				// reader's. A different position is one, and is carried.
				report.stoodDown += 1;
				return;
			}
			if (Date.now() < updatingUntil) {
				// The document is being relaid out, so this is the view moving
				// under the text rather than the reader scrolling. It is kept in
				// case it was a scroll, and released once the edit has settled.
				deferred.editor = true;
				report.stoodDown += 1;
				return;
			}
			void carryEditorAt(offset, where);
		});
	});
	// A setting that shapes the layout is resolved for the document at hand,
	// so moving one re-lays out the preview showing it rather than waiting for
	// the next edit or the next open.
	const configured = vscode.workspace.onDidChangeConfiguration((event) => {
		if (event.affectsConfiguration("markview", document) ||
			event.affectsConfiguration("markviewExport.template", document)) {
			void relayout(session, document, id).catch((error) => {
				void vscode.window.showErrorMessage(`Markview: ${String(error)}`);
			});
		}
	});
	// Only the document on screen is watched. A previewed document left
	// behind keeps its engine state, but its edits belong to a preview the
	// reader is no longer looking at.
	unwatch();
	watching = [watcher, saved, scrolled, chose, configured];
	context.subscriptions.push(watcher, saved, scrolled, chose, configured);
}

/**
 * Lays the document out again under settings that have changed.
 *
 * The engine resolves settings per document, so the same text under different
 * settings is a new layout rather than an edit: the state is published like
 * one, which is what moves the drawn text and asks for the pixels again.
 */
async function relayout(
	session: Session,
	document: vscode.TextDocument,
	id: string,
): Promise<void> {
	if (!showing(document)) {
		return;
	}
	const request = ++layoutRequest;
	const rules = await templateFor({}, document);
	if (request !== layoutRequest || !showing(document)) return;
	const settings = settingsOf(document);
	const version = document.version;
	const state = await session.open(id, document.getText(), {
		path: document.uri.fsPath,
		...rules,
		settings,
	});
	if (request !== layoutRequest || !showing(document) || version !== document.version) return;
	report.settings = settings as Record<string, unknown>;
	report.opens += 1;
	const adopted = adopt(state);
	if (current?.id === id) {
		current.blocks = adopted.blocks;
		current.bands.clear();
	}
	rememberAnchors(adopted);
	report.documentHeight = state.height;
	report.openedHeight = state.height;
	report.blocks = state.blocks.length;
	hold();
	await publish(adopted);
	await restoreAnchor();
	scheduleRenderedText();
}

/** Stops watching the document the panel was showing. */
function unwatch(): void {
	for (const listener of watching) {
		listener.dispose();
	}
	watching = [];
	++scrollSequence;
	if (scrollTimer) clearTimeout(scrollTimer);
	scrollTimer = undefined;
	pendingScroll = undefined;
	previewRow = undefined;
}

export function closePreview(): void {
	panel?.dispose();
}

/**
 * What the webview asks the panel for.
 *
 * The panel is reloaded for each document while this handler stays, so it
 * resolves the document it is showing at the moment the message arrives.
 */
async function receive(message: {
	ready?: boolean;
    tileError?: string;
    background?: string;
	scrollHeight?: number;
	live?: number;
	viewport?: number;
	/** Where the view sits, sent with every report. */
	at?: number;
	/** The source offset at the top of the view, and how far into its block. */
	source?: number;
	into?: number;
	/** Which state the webview is reporting against. */
	generation?: number;
	epoch?: number;
	paintedVersion?: number;
	displayedTiles?: number;
	pixelVersions?: number[];
	refreshing?: boolean;
	tileBounds?: Array<{ width: number; height: number; expectedHeight: number; pixels: number; expectedPixels: number; scale: number; fit: number; viewportWidth: number; documentWidth: number; inset: number[] }>;
	/** What the webview did to restore the reader's place. */
	/** A scroll the reader made. */
	scroll?: number;
	tile?: number;
	band?: number;
	width?: number;
	height?: number;
	scale?: number;
	reveal?: { start?: number; end?: number };
	find?: boolean;
	link?: string;
	linkTarget?: string;
	linkUrl?: string;
	selection?: { text: string; source_start: number };
	/** The characters the text layer is holding. */
	text?: string;
	/** The reader reached for find. */
	needText?: boolean;
	/** Whether the drawn spans are in reading order and one per offset. */
	ordered?: boolean;
	unique?: boolean;
	/** The document's own text, by its ends, as the find widget sees it. */
	findable?: { length: number; head: string; tail: string };
	/** The rendered text the find widget searches. */
	rendered?: string;
	/** Whether the webview's own copy command wrote the selection out. */
	copied?: boolean;
	copyText?: string;
	copyError?: string;
	focused?: boolean;
	spans?: number;
	clicked?: unknown;
	selectionState?: unknown;

}): Promise<void> {
    if (message.tileError) report.tileError = message.tileError;
	if (message.ready === true) {
		report.ready = true;
		// The panel was made a moment ago and its own document has only now
		// run: the state sent before that is sent again, so a preview opened
		// after one was closed is not blank until the next edit.
		if (shownState && current) {
			void post({
				state: shownState,
				version: current.document.version,
				generation,
				resend: true,
			});
		}
	}
	const shown = current;
	if (!shown) {
		return;
	}
	if (message.paintedVersion !== undefined && message.generation === generation) {
		report.paintedVersion = message.paintedVersion;
        report.background = message.background ?? "";
		report.tileBounds = message.tileBounds ?? [];
		report.paintedAt = performance.now();
	}

	if (typeof message.scrollHeight === "number") {
		report.scrollHeight = message.scrollHeight;
	}
	if (message.pixelVersions) report.pixelVersions = message.pixelVersions;
	if (typeof message.displayedTiles === "number") report.displayedTiles = message.displayedTiles;
	if (typeof message.refreshing === "boolean") report.refreshing = message.refreshing;
	if (typeof message.live === "number") {
		report.live = message.live;
	}
	if (typeof message.viewport === "number") {
		report.viewport = message.viewport;
	}
	if (typeof message.at === "number") {
		report.scroll = message.at;
	}
	if (typeof message.source === "number" && message.source >= 0) {
		// The report names an offset in the text the webview is holding, which
		// may be the text before an edit the host has already counted. Both
		// parts of the anchor are taken from this one report and shifted
		// together, so the reader's place is never half old and half new.
		const held = shifts.get(message.generation ?? generation) ?? 0;
		report.source = message.source + held;
		report.into = message.into ?? 0;
		anchor = { offset: report.source, into: report.into };
	}
	if (typeof message.scroll === "number") {
		report.scrollMessages += 1;
		if (Date.now() < updatingUntil) {
			deferred.preview = true;
			report.stoodDown += 1;
			return;
		}
		const y = message.scroll;
		scheduleScroll(sequence => carryPreview(y, sequence));
	}
	if (typeof message.tile === "number") {
		if (message.generation !== generation) return;
		report.requested.push(message.tile);
		report.tile = message.tile;
		const view = Math.max(1, Math.floor(message.height ?? 1));
		const scale = message.scale ?? 1;
		try {
			// The pixels and the text over them are one screenful, so they
			// are asked for together and answered together.
			const [tile, layer] = await Promise.all([
				shown.session.tile(shown.id, {
					width: Math.max(1, Math.ceil((message.width ?? 1) * scale)),
					height: Math.ceil(view * scale),
					scale,
					scroll: Math.max(0, message.tile),
				}),
				shown.session.text(shown.id, message.tile, message.tile + view),
			]);
			if (shown !== current || message.generation !== generation) return;
			// The answer names its document, so a webview that has been
			// reloaded for another one in the meantime can drop it as stale.
			// A cheap fingerprint of the pixels: enough to see that a theme
			// changed them without carrying megabytes into a report.
			let hash = 2166136261;
			const png = tile.png;
			for (let index = 0; index < png.length; index += 1) {
				hash = ((hash ^ png[index]) * 16777619) >>> 0;
			}
			report.painted = hash;
			await post({
				tile: {
					...tile,
					scale,
					height: view,
					band: message.band,
					generation: message.generation, epoch: message.epoch,
					clusters: adoptClusters(layer.clusters),
					rows: adoptRows(layer.rows),
				},
			});
		} catch (error) {
            report.tileError = String(error);
			// The band is not coming, so the webview stops waiting on it and
			// asks again the next time the viewport moves.
			await post({
                notice: String(error),
				tile: { band: message.band,
					generation: message.generation, epoch: message.epoch, failed: true },
			});
		}
	}
	if (typeof message.reveal === "object" && message.reveal !== null) {
		// A click in the preview points at the bytes under it, and the editor
		// is shown and selected at that range. The preview goes to the
		// passage itself rather than waiting for the editor to scroll.
		const range = message.reveal as { start?: number; end?: number };
		const start = range.start ?? 0;
		const block = blockFor(shown.blocks, start);
		const y = await yOf(start) ?? block?.y;
		if (current !== shown) return;
		if (y !== undefined) {
			report.scroll = y;
			previewEcho = { y, until: Date.now() + ECHO_MS };
			void post({ scroll: y });
		}
		if (message.find) findEcho = { start, end: range.end ?? start };
		await revealRange(shown, start, range.end ?? start);
	}
	if (typeof message.link === "string") {
		await route(shown, {
			kind: message.link,
			target: message.linkTarget,
			url: message.linkUrl,
		});
	}
	if (typeof message.text === "string") {
		report.text = message.text;
	}
	if (typeof message.ordered === "boolean") {
		report.ordered = message.ordered;
	}
	if (typeof message.unique === "boolean") {
		report.unique = message.unique;
	}
	if (typeof message.copyText === "string") {
        try {
            await vscode.env.clipboard.writeText(message.copyText);
            report.copied = true;
            report.copyError = "";
        } catch (error) {
            report.copied = false;
            report.copyError = String(error);
        }
    }
	if (typeof message.copied === "boolean") {
		report.copied = message.copied;
	}
	if (message.selectionState !== undefined) report.selectionState = message.selectionState;
	if (message.clicked !== undefined) {
		report.clicked = message.clicked;
	}
	if (typeof message.spans === "number") {
		report.spans = message.spans;
	}
	if (typeof message.focused === "boolean") {
		report.focused = message.focused;
	}
	if (typeof message.copyError === "string") {
		report.copyError = message.copyError;
	}
	if (typeof message.rendered === "string") {
		report.rendered = message.rendered;
	}
	if (message.findable !== undefined) {
		report.findable = message.findable as {
			length: number;
			head: string;
			tail: string;
		};
	}
	if (message.selection !== undefined) {
		report.selection = message.selection ?? undefined;
	}
}

/**
 * Shows a range of the document in the editor and selects it.
 *
 * A click in the preview is a pointer: the reader is asking for that text, so
 * the editor is focused with the range selected rather than only scrolled to.
 */
async function revealRange(
	shown: { document: vscode.TextDocument },
	start: number,
	end: number,
): Promise<void> {
	// A click in the preview is a request to see the source, so a source that
	// is not on screen is shown rather than the click being dropped.
	const editor =
		editorFor(shown) ??
		(await vscode.window.showTextDocument(shown.document, {
			viewColumn: vscode.ViewColumn.Beside,
			preserveFocus: true,
			preview: false,
		}));
	if (!editor) {
		return;
	}
	const range = new vscode.Range(
		editor.document.positionAt(start),
		editor.document.positionAt(Math.max(start, end)),
	);
	// The editor is being asked to show this byte, so the view it answers with
	// is the panel's own doing: carrying it back would drag the preview away
	// from the place the click already sent it to. A range the editor already
	// shows does not move it, and a move that does not happen is answered by no
	// report, so nothing is armed then — an armed expectation would swallow the
	// reader's next scroll instead.
	if (!editor.visibleRanges.some((shown) => shown.contains(range))) {
		expectReveal = start;
		revealGraceUntil = Date.now() + 200;
	}
	editor.revealRange(
		range,
		vscode.TextEditorRevealType.InCenterIfOutsideViewport,
	);
	editor.selection = new vscode.Selection(range.start, range.end);
	report.revealed = start;
}

/**
 * Follows a link the reader clicked.
 *
 * The engine classified it, so a local document is opened in the editor, a
 * remote address is handed to the browser, and a fragment is a place the engine
 * already resolved. The preview opens no tab of its own. A target the engine
 * would not open on its own — anything that is neither Markdown nor a known
 * inert file — is confirmed here first, because the document names it and the
 * desktop is what would run it.
 */
async function route(
	shown: { document: vscode.TextDocument; blocks: Block[] },
	link: { kind: string; target?: string; url?: string },
): Promise<void> {
	report.routed.push({ kind: link.kind, target: link.target ?? "" });
	const target = link.target ?? "";
	if (link.kind === "document") {
		void vscode.window.showTextDocument(vscode.Uri.file(target));
		return;
	}
	if (link.kind === "file") {
		void vscode.env.openExternal(vscode.Uri.file(target));
		return;
	}
	if (link.kind === "confirm") {
		let open = false;
		try {
			open =
				(await vscode.window.showWarningMessage(
					`Open ${path.basename(target)}?`,
					{ modal: true, detail: target },
					"Open",
				)) === "Open";
		} catch {
			// A host with no dialog surface refuses instead of answering, and
			// the target is then not opened.
			open = false;
		}
		if (open) {
			void vscode.env.openExternal(vscode.Uri.file(target));
		}
		return;
	}
	if (link.kind === "remote") {
		void vscode.env.openExternal(vscode.Uri.parse(target));
		return;
	}
	if (link.kind === "anchor") {
		const y = report.anchors[link.url ?? ""];
		if (y === undefined) {
			return;
		}
		report.scroll = y;
		previewEcho = { y, until: Date.now() + ECHO_MS };
		void post({ scroll: y });
		// The reader has moved, so the place a later layout would put back is
		// the fragment rather than where they were before the click: the
		// webview reports its new place once it is there, and the one held here
		// would otherwise drag them back the moment an image settled.
		anchor = undefined;
		const block = blockShown(shown.blocks, y);
		if (block) {
			void revealRange(shown, block.source_start, block.source_end);
		}
	}
}

/**
 * Shows a byte of the document in the editor, without moving the caret.
 *
 * The byte the editor is being asked to show is remembered so that its answer
 * can be recognised: only a report with that byte in view is the answer, and
 * nothing is armed when there is no editor to show it in.
 */
function reveal(
	shown: { document: vscode.TextDocument },
	offset: number,
	asked = -1,
): void {
	const editor = editorFor(shown);
	if (!editor) {
		return;
	}
	if (asked >= 0) {
		expectReveal = asked;
		revealGraceUntil = Date.now() + 200;
	}
	const position = editor.document.positionAt(offset);
	// `revealRange` moves the view without moving the caret, which is what
	// following the other surface should do.
	editor.revealRange(
		new vscode.Range(position, shown.document.validatePosition(new vscode.Position(position.line + 1, 0))),
		vscode.TextEditorRevealType.AtTop,
	);
}

/**
 * Hands the preview the rendered text of the whole document.
 *
 * The find widget searches what is rendered, and the pixels cannot be
 * searched, so the characters the engine drew are made available to it —
 * stripped of their geometry, which the widget has no use for, and with the
 * source range each run was drawn from, so a match can be followed.
 */
async function sendRenderedText(shown: {
	session: Session;
	id: string;
}): Promise<void> {
    const held = generation;
    const version = current?.document.version;
    const rendered = await shown.session.rendered(shown.id);
    if (current?.id !== shown.id || held !== generation || version !== current.document.version) return;
    const table = tableFor(current.document);
    const ranges = rendered.ranges.map(([start, end, length]) => [utf16Of(table, start), utf16Of(table, end), length]);
    await post({ findText: { text: rendered.text, ranges }, generation: held });
}

/** How long the panel waits before asking for the rendered text. */
const RENDERED_MS = 400;
let renderedTimer: NodeJS.Timeout | undefined;

/**
 * Asks for the rendered text once the document has settled.
 *
 * It is what the find widget searches, and the reader can open that widget by
 * any keybinding there is, so it is fetched for every document rather than
 * when some particular key is pressed. Waiting a moment keeps a burst of edits
 * from asking for it over and over.
 */
function scheduleRenderedText(): void {
	if (renderedTimer) {
		clearTimeout(renderedTimer);
	}
	renderedTimer = setTimeout(() => {
		renderedTimer = undefined;
		if (current) {
			void sendRenderedText(current);
		}
	}, RENDERED_MS);
}

/** Searches the preview's rendered text, as the find widget would. */
export async function findInPreview(needle: string, previous = false): Promise<void> {
	await post({ find: needle, previous });
}

/** Records that the panel exported to a path, for a test to read. */
export function panelExported(path: string): void {
	report.exported = path;
}

/**
 * Records the appearance the panel sent and has the preview redraw.
 *
 * The appearance is a palette, so what is already on screen was drawn in the
 * old one: the tiles are asked for again rather than left until the reader
 * happens to scroll.
 */
export function panelAppearance(theme: string, reflow: boolean): void {
	report.appearance = { theme, reflow };
	void post({ redraw: true });
}

/** Clears the preview's selection, so the next click points rather than ends. */
export async function clearPreviewSelection(): Promise<void> {
	await post({ clearSelection: true });
}

/** Clicks the preview at a point in the document, as a reader would. */
export async function clickPreviewAt(x: number, y: number): Promise<void> {
	await post({ clickAt: { x, y } });
}

/** Drags across the preview between two points in the document. */
export async function dragPreviewAt(
	from: { x: number; y: number },
	to: { x: number; y: number },
): Promise<void> {
	await post({ dragAt: { from, to } });
}

/** Copies the preview's selection, as the reader's Copy does. */
export async function copyPreviewSelection(): Promise<void> {
    panel?.reveal(panelColumn, false);
    report.copied = false;
    report.copyError = "";
    await post({ copySelection: true });
}

/** Moves the preview's view without touching the editor. */
export async function scrollPreviewTo(offset: number): Promise<void> {
	await post({ scroll: Math.max(0, offset) });
}

/** Reveals the editor selection in the preview. */
export async function revealSelection(): Promise<void> {
	const editor = vscode.window.activeTextEditor;
	if (!editor || !panel || !current) {
		return;
	}
	const offset = editor.document.offsetAt(editor.selection.active);
	const block = blockFor(current.blocks, offset);
	if (block) {
		report.syncs.editor += 1;
		report.scroll = Math.max(0, block.y - 40);
		previewEcho = { y: report.scroll, until: Date.now() + ECHO_MS };
		await post({ scroll: report.scroll });
	}
}

export type { Cluster };
