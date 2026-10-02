import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
	EditorView,
	keymap,
	lineNumbers,
	highlightActiveLine,
	highlightActiveLineGutter,
	drawSelection,
	dropCursor,
	type ViewUpdate,
} from "@codemirror/view";
import {
	defaultKeymap,
	history,
	historyKeymap,
	indentWithTab,
} from "@codemirror/commands";
import { markdown, markdownKeymap } from "@codemirror/lang-markdown";
import {
	defaultHighlightStyle,
	syntaxHighlighting,
	indentOnInput,
	bracketMatching,
} from "@codemirror/language";
import {
	Viewer,
	type ViewerOptions,
	type ReadingPosition,
} from "@markview/viewer";
import {
	ScrollSync,
	sourceToAnchor,
	anchorToSource,
	type SourceRange,
	type SourceExtent,
} from "@markview/scroll-sync";
import { styles } from "./style.js";

export interface ContentChange {
	markdown: string;
	documentVersion: number;
}
export interface EditorOptions {
	markdown?: string;
	viewer?: ViewerOptions;
	theme?: "light" | "dark";
	orientation?: "horizontal" | "vertical" | "auto";
	/** Initial source-pane fraction, clamped to `0.15..0.85`. */
	split?: number;
	toc?: boolean;
	extensions?: Extension;
	onChange?: (change: ContentChange) => void;
}

/** Markdown editing and automatic source-based bidirectional preview following. */
export class Editor {
	readonly element: HTMLDivElement;
	readonly view: EditorView;
	readonly viewer: Viewer;
	#options: EditorOptions;
	#theme = new Compartment();
	#extensions = new Compartment();
	#listeners = new AbortController();
	#unsubscribe: () => void;
	#toc: HTMLElement;
	#split: HTMLElement;
	#divider: HTMLElement;
	#disposed = false;
	#sync: ScrollSync;
	#tocVersion = -1;

	private constructor(
		element: HTMLDivElement,
		write: HTMLElement,
		viewer: Viewer,
		toc: HTMLElement,
		split: HTMLElement,
		divider: HTMLElement,
		options: EditorOptions,
	) {
		this.element = element;
		this.viewer = viewer;
		this.#toc = toc;
		this.#split = split;
		this.#divider = divider;
		this.#options = options;
		this.#sync = new ScrollSync(viewer.outline().documentVersion);
		this.view = new EditorView({
			parent: write,
			doc: viewer.getMarkdown(),
			extensions: [
				markdown(),
				history(),
				lineNumbers(),
				highlightActiveLineGutter(),
				highlightActiveLine(),
				drawSelection(),
				dropCursor(),
				indentOnInput(),
				bracketMatching(),
				EditorView.lineWrapping,
				syntaxHighlighting(defaultHighlightStyle),
				keymap.of([
					...markdownKeymap,
					indentWithTab,
					...defaultKeymap,
					...historyKeymap,
				]),
				this.#theme.of(this.#editorTheme(options.theme ?? "light")),
				this.#extensions.of(options.extensions ?? []),
				EditorView.updateListener.of((update) => this.#update(update)),
			],
		});
		this.#unsubscribe = viewer.onReadingPosition((position) =>
			this.#previewMoved(position),
		);
		this.#configureDOM();
		this.#bindInput();
		this.#renderTOC();
	}

	static async mount(
		container: HTMLElement,
		options: EditorOptions = {},
	): Promise<Editor> {
		const element = document.createElement("div");
		element.className = "markview-editor";
		const style = document.createElement("style");
		style.textContent = styles;
		const toc = document.createElement("nav");
		toc.className = "mv-toc";
		toc.setAttribute("aria-label", "Document outline");
		const split = document.createElement("div");
		split.className = "mv-split";
		const pane = (name: string, label: string): HTMLElement => {
			const pane = document.createElement("section");
			pane.className = `mv-pane ${name}`;
			const header = document.createElement("div");
			header.className = "mv-label";
			header.textContent = label;
			const content = document.createElement("div");
			content.className = "mv-content";
			pane.append(header, content);
			split.append(pane);
			return content;
		};
		const write = pane("mv-write", "Markdown");
		const divider = document.createElement("div");
		divider.className = "mv-divider";
		divider.tabIndex = 0;
		divider.setAttribute("role", "separator");
		divider.setAttribute("aria-label", "Resize editor panes");
		split.append(divider);
		const read = pane("mv-read", "Preview");
		element.append(style, toc, split);
		container.append(element);
		let editor: Editor | undefined;
		let viewer: Viewer | undefined;
		const source = EditorState.create({
			doc: options.markdown ?? "",
		}).doc.toString();
		const theme = options.theme ?? "light";
		try {
			viewer = await Viewer.mount(read, {
				...options.viewer,
				markdown: source,
				markview: { ...options.viewer?.markview, theme },
				onUserInput: () => {
					if (editor) {
						editor.#sync.takeControl("preview");
					}
					options.viewer?.onUserInput?.();
				},
			});
			editor = new Editor(
				element,
				write,
				viewer,
				toc,
				split,
				divider,
				options,
			);
			return editor;
		} catch (error) {
			viewer?.destroy();
			element.remove();
			throw error;
		}
	}

	getMarkdown(): string {
		this.#live();
		return this.view.state.doc.toString();
	}
	setMarkdown(markdown: string): void {
		this.#live();
		this.view.dispatch({
			changes: {
				from: 0,
				to: this.view.state.doc.length,
				insert: markdown,
			},
		});
	}
	/** Reconfigures layout/theme/extensions without replacing the editor or its history. */
	setOptions(
		options: Partial<
			Pick<
				EditorOptions,
				| "theme"
				| "orientation"
				| "split"
				| "toc"
				| "extensions"
				| "onChange"
			>
		>,
	): void {
		this.#live();
		this.#options = { ...this.#options, ...options };
		const effects = [];
		if (options.extensions !== undefined)
			effects.push(this.#extensions.reconfigure(options.extensions));
		if (options.theme !== undefined) {
			effects.push(
				this.#theme.reconfigure(this.#editorTheme(options.theme)),
			);
			this.viewer.setOptions({
				...this.#options.viewer?.markview,
				theme: options.theme,
			});
		}
		this.view.dispatch({ effects });
		this.#configureDOM();
		this.#renderTOC();
	}
	destroy(): void {
		if (this.#disposed) return;
		this.#disposed = true;
		this.#sync.cancel();
		this.#listeners.abort();
		this.#unsubscribe();
		this.view.destroy();
		this.viewer.destroy();
		this.element.remove();
	}

	#update(update: ViewUpdate): void {
		if (this.#disposed) return;
		if (update.docChanged) {
			const position = this.viewer.readingPosition();
			const offset = update.changes.mapPos(position?.offset ?? 0);
			this.viewer.setMarkdown(update.state.doc.toString(), offset);
			this.#sync.setDocumentVersion(
				this.viewer.outline().documentVersion,
			);
			this.#renderTOC();
			this.#options.onChange?.({
				markdown: this.getMarkdown(),
				documentVersion: this.viewer.outline().documentVersion,
			});
		}
		if (update.docChanged || update.geometryChanged) {
			if (this.#sync.owner === "source") this.#followEditor();
			else {
				const position = this.viewer.readingPosition();
				if (position) this.#previewMoved(position);
			}
		}
	}
	#followEditor(): void {
		if (this.#disposed) return;
		const request = this.#sync.begin("source");
		if (!request) return;
		this.view.requestMeasure({
			key: this,
			read: (view) => {
				const top =
					view.scrollDOM.getBoundingClientRect().top +
					view.documentPadding.top;
				const offset =
					view.posAtCoords(
						{
							x: view.contentDOM.getBoundingClientRect().left + 1,
							y: top + 1,
						},
						false,
					) ?? 0;
				return sourceToAnchor(
					{ offset, top },
					this.viewer.sourceToPreview(offset)?.source ?? null,
					(source) => this.#sourceExtent(view, source),
				);
			},
			write: (position) => {
				if (!this.#disposed && this.#sync.isCurrent(request)) {
					this.viewer.scrollToSource(
						position.offset,
						position.fraction,
					);
				}
			},
		});
	}
	#sourceExtent(view: EditorView, source: SourceRange): SourceExtent {
		const end = Math.max(source.start, source.end - 1);
		return {
			top:
				view.coordsAtPos(source.start, 1)?.top ??
				view.documentTop + view.lineBlockAt(source.start).top,
			bottom:
				view.coordsAtPos(source.end, source.end > source.start ? -1 : 1)
					?.bottom ?? view.documentTop + view.lineBlockAt(end).bottom,
		};
	}
	#previewMoved(position: ReadingPosition): void {
		if (
			this.#disposed ||
			position.documentVersion !== this.viewer.outline().documentVersion
		)
			return;
		this.#renderTOC();
		for (const button of this.#toc.querySelectorAll("button")) {
			if (button.dataset.anchor === position.heading?.anchor)
				button.setAttribute("aria-current", "location");
			else button.removeAttribute("aria-current");
		}
		const request = this.#sync.begin("preview", position.documentVersion);
		if (!request) return;
		const offset = Math.min(position.offset, this.view.state.doc.length);
		this.view.requestMeasure({
			key: this,
			read: (view) => ({
				target: anchorToSource(
					{ offset, fraction: position.fraction },
					this.viewer.sourceToPreview(offset)?.source ?? null,
					(source) => this.#sourceExtent(view, source),
				),
				top:
					view.scrollDOM.getBoundingClientRect().top +
					view.documentPadding.top,
			}),
			write: ({ target, top }, view) => {
				if (
					!this.#disposed &&
					this.#sync.isCurrent(request) &&
					target !== null
				) {
					view.scrollDOM.scrollTop += target - top;
				}
			},
		});
	}
	#bindInput(): void {
		const signal = this.#listeners.signal;
		for (const name of ["wheel", "pointerdown", "keydown"] as const) {
			this.view.dom.addEventListener(
				name,
				() => {
					this.#sync.takeControl("source");
					this.viewer.cancelNavigation();
				},
				{ capture: true, signal },
			);
		}
		this.view.scrollDOM.addEventListener(
			"scroll",
			() => this.#followEditor(),
			{ signal },
		);
		let pointer: number | null = null;
		this.#divider.addEventListener(
			"pointerdown",
			(event) => {
				if (event.button !== 0) return;
				pointer = event.pointerId;
				this.#divider.setPointerCapture(pointer);
				event.preventDefault();
			},
			{ signal },
		);
		this.#divider.addEventListener(
			"pointermove",
			(event) => {
				if (event.pointerId !== pointer) return;
				const rect = this.#split.getBoundingClientRect();
				const vertical =
					getComputedStyle(this.#split).flexDirection === "column";
				this.#options.split = vertical
					? (event.clientY - rect.top) / rect.height
					: (event.clientX - rect.left) / rect.width;
				this.#configureDOM();
			},
			{ signal },
		);
		for (const name of [
			"pointerup",
			"pointercancel",
			"lostpointercapture",
		] as const) {
			this.#divider.addEventListener(
				name,
				() => {
					pointer = null;
				},
				{ signal },
			);
		}
		this.#divider.addEventListener(
			"keydown",
			(event) => {
				const direction = ["ArrowLeft", "ArrowUp"].includes(event.key)
					? -1
					: ["ArrowRight", "ArrowDown"].includes(event.key)
						? 1
						: 0;
				if (!direction) return;
				event.preventDefault();
				this.#options.split =
					(this.#options.split ?? 0.5) + direction * 0.02;
				this.#configureDOM();
			},
			{ signal },
		);
	}
	#configureDOM(): void {
		this.element.dataset.theme = this.#options.theme ?? "light";
		this.element.dataset.orientation = this.#options.orientation ?? "auto";
		const split = Math.max(
			0.15,
			Math.min(0.85, this.#options.split ?? 0.5),
		);
		this.#options.split = split;
		this.element.style.setProperty("--mv-split", `${split * 100}%`);
		this.#toc.hidden = this.#options.toc === false;
		this.#toc.style.display = this.#toc.hidden ? "none" : "";
		this.#divider.setAttribute(
			"aria-valuenow",
			String(Math.round(split * 100)),
		);
		this.#divider.setAttribute("aria-valuemin", "15");
		this.#divider.setAttribute("aria-valuemax", "85");
		this.#divider.setAttribute(
			"aria-orientation",
			getComputedStyle(this.#split).flexDirection === "column"
				? "horizontal"
				: "vertical",
		);
	}
	#renderTOC(): void {
		const outline = this.viewer.outline();
		if (outline.documentVersion === this.#tocVersion) return;
		this.#tocVersion = outline.documentVersion;
		this.#toc.replaceChildren();
		const label = document.createElement("div");
		label.className = "mv-label";
		label.textContent = "Contents";
		this.#toc.append(label);
		if (!outline.entries.length) {
			const empty = document.createElement("div");
			empty.className = "mv-empty";
			empty.textContent = "Headings appear here";
			this.#toc.append(empty);
		}
		for (const heading of outline.entries) {
			const button = document.createElement("button");
			button.type = "button";
			button.textContent = heading.text;
			button.dataset.anchor = heading.anchor;
			button.style.paddingLeft = `${8 + (heading.level - 1) * 10}px`;
			button.onclick = () => {
				this.#sync.takeControl("preview");
				this.viewer.navigateHeading(heading.anchor);
			};
			this.#toc.append(button);
		}
	}
	#editorTheme(theme: "light" | "dark"): Extension {
		return EditorView.theme(
			{
				"&": {
					height: "100%",
					color: "var(--mv-ink)",
					backgroundColor: "var(--mv-source)",
				},
				".cm-scroller": {
					overflow: "auto",
					fontFamily: '"Noto Sans Mono",Consolas,monospace',
					fontSize: "14px",
					lineHeight: "1.65",
				},
				".cm-content": { padding: "14px 0" },
				".cm-line": { padding: "0 16px" },
				".cm-gutters": {
					backgroundColor: "var(--mv-source)",
					color: "var(--mv-muted)",
					border: "none",
				},
				".cm-cursor": { borderLeftColor: "var(--mv-accent)" },
				".cm-activeLine,.cm-activeLineGutter": {
					backgroundColor: "transparent",
				},
			},
			{ dark: theme === "dark" },
		);
	}
	#live(): void {
		if (this.#disposed) throw new Error("this Editor has been destroyed");
	}
}
