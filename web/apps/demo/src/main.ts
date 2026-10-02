import "./style.css";
import { Editor } from "@markview/editor";
import { browserResources } from "@markview/resources";
import type { MarkviewStats } from "@markview/viewer";
import { loadDemoFonts, type FontDownload } from "./fonts.js";
import { documents } from "./documents.js";

const dom = {
	copy: document.querySelector<HTMLButtonElement>("#copy-selection")!,
	workspace: document.querySelector<HTMLElement>(".workspace")!,
	desk: document.querySelector<HTMLElement>("#desk")!,
	loadingText: document.querySelector<HTMLElement>("#loading-text")!,
	loadingDetail: document.querySelector<HTMLElement>("#loading-detail")!,
	sample: document.querySelector<HTMLSelectElement>("#sample")!,
	file: document.querySelector<HTMLInputElement>("#file")!,
	name: document.querySelector<HTMLElement>("#document-name")!,
	meta: document.querySelector<HTMLElement>("#document-meta")!,
	theme: document.querySelector<HTMLButtonElement>("#theme")!,
	contents: document.querySelector<HTMLButtonElement>("#contents")!,
	error: document.querySelector<HTMLElement>("#error")!,
	errorMessage: document.querySelector<HTMLElement>("#error-message")!,
	retry: document.querySelector<HTMLButtonElement>("#retry")!,
	notice: document.querySelector<HTMLElement>("#notice")!,
	engine: document.querySelector<HTMLElement>("#engine-state")!,
	engineText: document.querySelector<HTMLElement>("#engine-text")!,
	hint: document.querySelector<HTMLElement>("#mode-hint")!,
	empty: document.querySelector<HTMLElement>(".empty-state")!,
};
const drafts = new Map<string, { name: string; markdown: string }>(
	Object.entries(documents).map(([id, doc]) => [id, { ...doc }]),
);
const listeners = new AbortController();
const narrow = window.matchMedia("(max-width: 800px)");
let editor: Editor | undefined;
let selected = "welcome";
let dark = false;
let mode: "read" | "edit";
let noticeTimer = 0;
let fileRequest = 0;
let metadataTimer = 0;
let disposed = false;

function setText(node: HTMLElement, text: string): void {
	if (node.textContent !== text) node.textContent = text;
}

function setLoading(text: string, detail: string): void {
	if (disposed || dom.engine.dataset.state === "error") return;
	setText(dom.loadingText, text);
	setText(dom.loadingDetail, detail);
	setText(dom.engineText, text);
}

function downloadProgress(downloads: readonly FontDownload[]): void {
	const completed = downloads.filter((font) => font.complete).length;
	if (completed === downloads.length) {
		const cached = downloads.filter((font) => font.cached).length;
		setLoading(
			"Preparing fonts and renderer…",
			`${cached ? `${cached}/${downloads.length} fonts loaded from cache.` : "All fonts downloaded."} Initializing the renderer and registering fonts.`,
		);
		return;
	}
	const current =
		downloads.find((font) => !font.complete && font.received > 0) ??
		downloads.find((font) => !font.complete)!;
	const bytes = (value: number) =>
		value < 1_000_000
			? `${Math.round(value / 1000)} KB`
			: `${(value / 1_000_000).toFixed(1)} MB`;
	const received = current.total
		? `${Math.min(99, Math.floor((current.received / current.total) * 100))}% received · ${bytes(current.received)} / ${bytes(current.total)}`
		: current.received
			? `${bytes(current.received)} received`
			: "Waiting for download…";
	setLoading(
		`Loading fonts · ${completed}/${downloads.length} complete`,
		`${current.name} · ${received}`,
	);
}

function notify(message: string): void {
	clearTimeout(noticeTimer);
	setText(dom.notice, message);
	noticeTimer = window.setTimeout(() => setText(dom.notice, ""), 4000);
}

function updateDocument(): void {
	const draft = drafts.get(selected)!;
	setText(dom.name, draft.name);
	const characters = Array.from(draft.markdown).length;
	const lines = draft.markdown ? draft.markdown.split("\n").length : 0;
	setText(
		dom.meta,
		`${lines.toLocaleString("en-US")} lines · ${characters.toLocaleString("en-US")} characters`,
	);
	const empty = !draft.markdown.trim();
	dom.workspace.classList.toggle("is-empty", empty);
	dom.empty.hidden = !empty || mode !== "read";
}

function setMode(): void {
	const sourceFocused = editor?.view.dom.contains(document.activeElement);
	mode = location.hash === "#edit" ? "edit" : "read";
	document.documentElement.dataset.mode = mode;
	for (const link of document.querySelectorAll<HTMLAnchorElement>(
		".mode-switch a",
	)) {
		if (link.dataset.mode === mode)
			link.setAttribute("aria-current", "page");
		else link.removeAttribute("aria-current");
	}
	dom.hint.hidden = mode === "read";
	if (editor) {
		editor.view.requestMeasure();
		// Keep keyboard focus on the visible pane.
		if (mode === "read" && sourceFocused) {
			editor.viewer.canvas.focus({ preventScroll: true });
		}
	}
	updateDocument();
}

function renderStats(stats: MarkviewStats): void {
	if (!editor || disposed) return;
	dom.copy.hidden = stats.selectionLength === 0;
	const state = stats.pending ? "layout" : "ready";
	dom.engine.dataset.state = state;
	setText(
		dom.engineText,
		stats.pending ? "Composing page…" : "Ready to read",
	);
	if (!stats.pending && stats.frames > 0) {
		document.body.dataset.ready = "true";
		dom.workspace.setAttribute("aria-busy", "false");
	}
}

function showError(message: string, startup = false): void {
	setText(dom.errorMessage, message);
	dom.error.hidden = false;
	dom.retry.hidden = !startup;
	if (startup) {
		dom.desk.querySelector(".loading")?.remove();
		dom.engine.dataset.state = "error";
		setText(dom.engineText, "Reader unavailable");
		dom.workspace.setAttribute("aria-busy", "false");
	}
}

function setContents(show: boolean): void {
	dom.contents.setAttribute("aria-pressed", String(show));
	dom.workspace.dataset.contents = String(show);
	editor!.setOptions({ toc: show });
}

function replaceDocument(id: string): void {
	selected = id;
	const draft = drafts.get(id)!;
	editor!.setMarkdown(draft.markdown);
	editor!.viewer.scrollToSource(0, 0);
	dom.error.hidden = true;
	updateDocument();
}

async function openFile(): Promise<void> {
	const file = dom.file.files?.[0];
	if (!file) return;
	const request = ++fileRequest;
	try {
		const markdown = await file.text();
		if (request !== fileRequest || disposed) return;
		drafts.set("file", { name: file.name, markdown });
		let option =
			dom.sample.querySelector<HTMLOptionElement>('[value="file"]');
		if (!option) {
			option = new Option(file.name, "file");
			dom.sample.add(option);
		}
		option.textContent = file.name;
		dom.sample.value = "file";
		replaceDocument("file");
		notify(`Opened ${file.name}`);
	} catch {
		showError(`Could not read ${file.name}. Open the file again to retry.`);
	} finally {
		dom.file.value = "";
	}
}

function download(): void {
	const draft = drafts.get(selected)!;
	const url = URL.createObjectURL(
		new Blob([editor!.getMarkdown()], {
			type: "text/markdown;charset=utf-8",
		}),
	);
	const link = document.createElement("a");
	link.href = url;
	link.download = /\.(md|markdown)$/i.test(draft.name)
		? draft.name
		: `${draft.name}.md`;
	link.click();
	window.setTimeout(() => URL.revokeObjectURL(url), 1000);
	notify(`Downloaded ${link.download}`);
}

async function boot(): Promise<void> {
	let fontSet: Awaited<ReturnType<typeof loadDemoFonts>> | undefined;
	try {
		fontSet = await loadDemoFonts(downloadProgress);
		if (disposed) {
			fontSet.destroy();
			return;
		}
		setLoading(
			"Preparing page…",
			"Building the document outline and laying out Markdown.",
		);
		editor = await Editor.mount(dom.desk, {
			markdown: documents.welcome.markdown,
			toc: !narrow.matches,
			viewer: {
				fonts: fontSet,
				markview: { width: 760, fontSize: 18 },
				resources: browserResources({
					baseUrl: document.baseURI,
					onError: () =>
						notify(
							"An image could not load. Check its URL and try opening the document again.",
						),
				}),
				onStats: renderStats,
				onLink: (target) => {
					const url = new URL(
						target,
						selected === "component-guide"
							? "https://github.com/szdytom/markview/blob/main/docs/"
							: document.baseURI,
					);
					if (["https:", "http:", "mailto:"].includes(url.protocol))
						window.open(url.href, "_blank", "noopener,noreferrer");
				},
				onError: () =>
					showError(
						"The renderer stopped. Reload the page to try again.",
						true,
					),
			},
			onChange: ({ markdown }) => {
				drafts.get(selected)!.markdown = markdown;
				clearTimeout(metadataTimer);
				metadataTimer = window.setTimeout(updateDocument, 150);
			},
		});
		// The mounted viewer retains its own font handle.
		fontSet.destroy();
		fontSet = undefined;
		if (disposed) {
			editor.destroy();
			return;
		}
		dom.desk.querySelector(".loading")!.remove();
		editor.viewer.canvas.setAttribute("aria-label", "Rendered Markdown");
		editor.view.contentDOM.setAttribute("aria-label", "Markdown source");
		for (const control of dom.workspace.querySelectorAll<
			HTMLButtonElement | HTMLSelectElement
		>("button, select"))
			control.disabled = false;
		const signal = listeners.signal;
		dom.copy.addEventListener(
			"click",
			() => {
				void editor!.viewer.reader.markview.copy().then(
					(copied) =>
						notify(
							copied
								? "Copied selection"
								: "Select text on the page first.",
						),
					() =>
						notify(
							"Could not copy. Select the text and try again.",
						),
				);
			},
			{ signal },
		);
		dom.sample.addEventListener(
			"change",
			() => {
				++fileRequest;
				replaceDocument(dom.sample.value);
			},
			{ signal },
		);
		dom.file.addEventListener(
			"change",
			() => {
				void openFile();
			},
			{ signal },
		);
		document
			.querySelector("#open")!
			.addEventListener("click", () => dom.file.click(), { signal });
		document
			.querySelector("#download")!
			.addEventListener("click", download, { signal });
		dom.theme.addEventListener(
			"click",
			() => {
				dark = !dark;
				editor!.setOptions({ theme: dark ? "dark" : "light" });
				document.documentElement.dataset.theme = dark
					? "dark"
					: "light";
				dom.theme.setAttribute("aria-pressed", String(dark));
				setText(dom.theme, dark ? "Light paper" : "Dark paper");
			},
			{ signal },
		);
		dom.contents.addEventListener(
			"click",
			() => {
				const show =
					dom.contents.getAttribute("aria-pressed") !== "true";
				setContents(show);
			},
			{ signal },
		);
		setContents(!narrow.matches);
		narrow.addEventListener("change", () => setContents(!narrow.matches), {
			signal,
		});
		dom.desk.addEventListener(
			"pointerdown",
			(event) => {
				if (
					!narrow.matches ||
					dom.workspace.dataset.contents !== "true"
				)
					return;
				if (!(event.target as Element).closest(".mv-toc")) {
					setContents(false);
					event.preventDefault();
					event.stopPropagation();
				}
			},
			{ capture: true, signal },
		);
		dom.desk.addEventListener(
			"click",
			(event) => {
				if (
					narrow.matches &&
					(event.target as Element).closest(".mv-toc button")
				)
					setContents(false);
			},
			{ signal },
		);
		dom.desk.addEventListener(
			"keydown",
			(event) => {
				if (narrow.matches && event.key === "Escape") {
					setContents(false);
					editor!.viewer.canvas.focus({ preventScroll: true });
				}
			},
			{ signal },
		);
		setMode();
		renderStats(editor.viewer.reader.markview.stats());
	} catch (error) {
		fontSet?.destroy();
		if (disposed) return;
		console.error("Markview startup:", error);
		showError(
			"The reading room could not start. Check your connection and WebGL2 support, then try again.",
			true,
		);
	}
}

dom.retry.addEventListener("click", () => location.reload(), {
	signal: listeners.signal,
});
window.addEventListener("hashchange", setMode, { signal: listeners.signal });
window.addEventListener("pagehide", (event) => {
	if (event.persisted) return;
	disposed = true;
	listeners.abort();
	clearTimeout(noticeTimer);
	clearTimeout(metadataTimer);
	editor?.destroy();
});
setMode();
void boot();
