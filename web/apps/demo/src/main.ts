// `main.ts` is the demo's wiring file: attach a `CanvasReader`, feed it the
// textarea, and surface the engine's state to the page. Everything else —
// frame loop, DPR sizing, input, selection — lives in the package.

import { CanvasReader, init } from "@markview/web";
import type { Markview, MarkviewOptions, MarkviewStats } from "@markview/web";
import { fonts } from "../../assets/fonts.js";

const DEBOUNCE_MS = 120;
const NOTICE_MS = 2200;
const DONE_MS = 1400;
// The thumb never shrinks below this, so it stays grabbable on short viewports.
const MIN_THUMB_PX = 24;
const NUMBERS = new Intl.NumberFormat("en-US");

const dom = {
	source: document.querySelector("#source") as HTMLTextAreaElement,
	canvas: document.querySelector("#view") as HTMLCanvasElement,
	error: document.querySelector("#error") as HTMLDivElement,
	engine: document.querySelector("#engine-state") as HTMLElement,
	engineText: document.querySelector("#engine-text") as HTMLElement,
	backend: document.querySelector("#backend") as HTMLElement,
	sourceMeta: document.querySelector("#source-meta") as HTMLElement,
	notice: document.querySelector("#notice") as HTMLElement,
	copy: document.querySelector("#copy") as HTMLButtonElement,
	selectAll: document.querySelector("#select-all") as HTMLButtonElement,
	theme: document.querySelector("#theme") as HTMLButtonElement,
	rail: document.querySelector("#rail") as HTMLDivElement,
	thumb: document.querySelector("#thumb") as HTMLDivElement,
	stats: {
		blocks: document.querySelector("#stat-blocks") as HTMLElement,
		height: document.querySelector("#stat-height") as HTMLElement,
		layout: document.querySelector("#stat-layout") as HTMLElement,
		frame: document.querySelector("#stat-frame") as HTMLElement,
		reused: document.querySelector("#stat-reused") as HTMLElement,
		glyphs: document.querySelector("#stat-glyphs") as HTMLElement,
	},
};

const COPY_LABEL = dom.copy.textContent ?? "Copy";

// One handle and one config for every control below; `boot()` fills them in.
// The config is read inside `boot` so a malformed `MV_CONFIG` fails through
// the same error boundary as every other startup fault.
let markview: Markview | undefined;
let config: MarkviewOptions = {};

let noticeTimer = 0;
let doneTimer = 0;
let scrubbing = false;

function set(node: HTMLElement, text: string): void {
	if (node.textContent !== text) node.textContent = text;
}

// `notify` reports one-shot feedback in the pane head; a new message cancels
// the previous one's timer so they never stack.
function notify(message: string): void {
	clearTimeout(noticeTimer);
	set(dom.notice, message);
	noticeTimer = window.setTimeout(() => set(dom.notice, ""), NOTICE_MS);
}

function fail(error: unknown): void {
	window.__markviewError = String(error);
	dom.engine.dataset.state = "error";
	set(dom.engineText, "Engine failed");
	set(dom.error, `The Markview wasm module did not start: ${String(error)}. `
		+ "Build it with scripts/build-web.sh and serve web/dist over http.");
	dom.error.hidden = false;
	console.error("markview demo:", error);
}

// `MV_CONFIG` is injected by the page or a test; it may be an object or a JSON
// string. Unknown keys are the engine's business to ignore.
function readConfig(): MarkviewOptions {
	const injected = globalThis.MV_CONFIG;
	if (injected === undefined) return {};
	if (typeof injected === "string") return JSON.parse(injected) as MarkviewOptions;
	return injected;
}

function renderStats(stats: MarkviewStats): void {
	set(dom.stats.blocks, NUMBERS.format(stats.blocks));
	set(dom.stats.height, `${NUMBERS.format(Math.round(stats.contentHeight))} px`);
	set(dom.stats.layout, `${stats.layoutMs.toFixed(1)} ms`);
	set(dom.stats.frame, `${stats.frameMs.toFixed(1)} ms`);
	set(dom.stats.reused, NUMBERS.format(stats.reused));
	set(dom.stats.glyphs, NUMBERS.format(stats.glyphs));
}

// The rail mirrors the document's scroll: the thumb's height is the
// viewport-to-content ratio, its offset the scroll-to-max ratio.
function updateRail(): void {
	if (!markview) return;
	const max = markview.maxScroll();
	if (max <= 0.5) {
		dom.rail.hidden = true;
		return;
	}
	dom.rail.hidden = false;
	const railHeight = dom.rail.clientHeight;
	const thumbHeight = Math.max(
		MIN_THUMB_PX,
		Math.round((railHeight / markview.contentHeight()) * railHeight),
	);
	const travel = Math.max(0, railHeight - thumbHeight);
	dom.thumb.style.height = `${thumbHeight}px`;
	dom.thumb.style.transform =
		`translateY(${Math.round((markview.scroll() / max) * travel)}px)`;
}

// Maps a pointer y to a scroll position along the rail.
function scrollRailTo(clientY: number): void {
	if (!markview) return;
	const rect = dom.rail.getBoundingClientRect();
	const fraction = Math.min(1, Math.max(0, (clientY - rect.top) / rect.height));
	markview.setScroll(fraction * markview.maxScroll());
}

async function copySelection(): Promise<void> {
	if (!markview) return;
	const text = markview.selectedText();
	if (!text) {
		notify("Nothing selected");
		return;
	}
	const ok = await markview.copy();
	if (!ok) return;
	notify(`Copied ${NUMBERS.format(text.length)} characters`);
	set(dom.copy, "Copied");
	dom.copy.classList.add("is-done");
	clearTimeout(doneTimer);
	doneTimer = window.setTimeout(() => {
		set(dom.copy, COPY_LABEL);
		dom.copy.classList.remove("is-done");
	}, DONE_MS);
}

function toggleTheme(): void {
	if (!markview) return;
	const theme = config.theme === "dark" ? "light" : "dark";
	config = { ...config, theme };
	document.documentElement.dataset.theme = theme;
	dom.theme.setAttribute("aria-pressed", String(theme === "dark"));
	markview.setOptions(config);
}

dom.copy.addEventListener("click", () => {
	void copySelection();
});

dom.selectAll.addEventListener("click", () => {
	if (!markview) return;
	markview.selectAll();
	dom.canvas.focus({ preventScroll: true });
});

dom.theme.addEventListener("click", toggleTheme);

// The rail jumps on press and scrubs while held; the pointer capture keeps the
// drag alive when the pointer leaves the 11 px strip.
dom.rail.addEventListener("pointerdown", (event) => {
	if (event.button !== 0) return;
	scrubbing = true;
	dom.rail.setPointerCapture(event.pointerId);
	scrollRailTo(event.clientY);
});
dom.rail.addEventListener("pointermove", (event) => {
	if (scrubbing) scrollRailTo(event.clientY);
});
const endScrub = (): void => {
	scrubbing = false;
};
dom.rail.addEventListener("pointerup", endScrub);
dom.rail.addEventListener("pointercancel", endScrub);

function updateSourceMeta(): void {
	const text = dom.source.value;
	const lines = text ? text.split("\n").length : 0;
	set(dom.sourceMeta, `${NUMBERS.format(lines)} lines · ${NUMBERS.format(text.length)} chars`);
}

async function boot(): Promise<void> {
	try {
		dom.engine.dataset.state = "loading";
		config = readConfig();
		await init({ fonts });

		let ready = false;
		const reader = await CanvasReader.attach(dom.canvas, {
			markdown: dom.source.value,
			onLink: (target) => notify(`Link: ${target}`),
			onImage: (target) => notify(`Image: ${target}`),
			markview: config,
			onStats: (stats) => {
				renderStats(stats);
				updateRail();
				if (!ready && stats.frames > 0) {
					ready = true;
					window.__markviewReady = true;
					document.body.dataset.ready = "true";
					dom.engine.dataset.state = "ready";
					set(dom.engineText, "Ready");
					set(dom.backend, `${stats.backend} · ${stats.adapter}`);
					dom.backend.title = stats.adapter;
				}
			},
			onError: fail,
		});
		// `mv` is the package's `Markview`, so the page and its tests drive the
		// same public surface a host application would.
		markview = reader.markview;
		window.mv = reader.markview;
		window.mvStats = () => reader.markview.stats();
		window.mvReader = reader;
		document.querySelector("#scroll-mode")?.addEventListener("change", (event) => {
			reader.markview.setScrollMode((event.target as HTMLSelectElement).value === "external" ? "external" : "internal");
		});
		document.querySelector("#interaction-sample")?.addEventListener("click", () => {
			dom.source.value = "# Interaction sample\n\n[Jump to hidden heading](#hidden) · [External link](https://example.com)\n\n<details>\n<summary>Expandable section</summary>\n\n## Hidden\n\nThis heading is inside a disclosure.\n\n</details>\n\n```text\n" + "Wide block — ".repeat(30) + "\n```\n\n" + "A paragraph for wheel scrolling and selection.\n\n".repeat(80);
			reader.setMarkdown(dom.source.value); updateSourceMeta();
		});
		// The counts and the update share the debounce: a big paste must not run
		// a full-string scan once per inserted character.
		let timer = 0;
		dom.source.addEventListener("input", () => {
			clearTimeout(timer);
			timer = setTimeout(() => {
				updateSourceMeta();
				reader.setMarkdown(dom.source.value);
			}, DEBOUNCE_MS);
		});
		updateSourceMeta();
	} catch (error) {
		fail(error);
	}
}

void boot();
