/**
 * The engine, as an editor host drives it.
 *
 * One session is one engine process with one pipe to it. A request is written
 * as a single JSON line; its answer comes back as a JSON header named for
 * the outcome, followed by raw PNG bytes for a tile. A `layout` line notifies
 * the client when geometry it already published has changed. This file deliberately imports
 * nothing from the editor, so a test can drive a real session.
 */
import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { Frames } from "./frames.js";

/** One block of the document, at block precision. */
/** One clickable fragment: where it is, what it points at, and where that is. */
export interface Link {
	url: string;
	kind: "anchor" | "remote" | "document" | "file" | "confirm" | "refused";
	target: string | null;
	x: number;
	y: number;
	width: number;
	height: number;
	/** The position a fragment names, which the engine resolved. */
	to: number | null;
}

export interface Block {
	id: number;
	source_start: number;
	source_end: number;
	y: number;
	height: number;
	links: Link[];
}

/** One rendered fragment: where it is, what it says, and what it was set from. */
export interface Cluster {
	id: string;
	x: number;
	y: number;
	width: number;
	height: number;
	text: string;
	source_start: number;
	source_end: number;
}

/** A drawn row, kept even where a wide block has it scrolled out of sight. */
export interface Row {
	source_start: number;
	source_end: number;
	y: number;
	height: number;
}

/** Where a document stands, in the shape the engine publishes it. */
export interface State {
	id: string;
	height: number;
	width: number;
	blocks: Block[];
	images: number;
	complete: boolean;
	degraded: number;
	math_errors: number;
	deferred: number;
}

export interface TextLayer {
	id: string;
	clusters: Cluster[];
	rows: Row[];
}

export interface Tile {
	id: string;
	width: number;
	height: number;
	scale: number;
	scroll: number;
    /** Opaque sRGB page clear color, matching the native renderer. */
    background: [number, number, number];
	/** Encoded PNG bytes, never a base64 string. */
	png: Uint8Array;
}

/** The settings a host resolves for one document and sends with it. */
export interface DocumentSettings {
	font_size?: number;
	width?: number;
	paragraph_indent?: number;
	justify?: boolean;
	hyphenate?: boolean;
	codeblock_wrap?: boolean;
}

export interface Exported {
	output: string;
	format: "pdf" | "png";
	bytes: number;
	width?: number;
	height?: number;
	elapsed_ms: number;
}

/** One export template the engine has, bundled or installed. */
export interface Template {
	id: string;
	name: string;
	installed: boolean;
	error?: string | null;
}

export interface Appearance {
	reflow: boolean;
	elapsed_ms: number;
}

/** What a host is told when a document's geometry moves. */
export type LayoutListener = (state: State) => void;

interface Pending {
	resolve: (answer: unknown) => void;
	reject: (error: Error) => void;
}

/**
 * One engine process.
 *
 * The spawn is lazy in the host rather than here: a session is created when a
 * window first needs one and lives until the window ends, so the cost of
 * starting the engine is paid once.
 */
export class Session {
	private readonly child: ChildProcessWithoutNullStreams;

	private readonly pending: Pending[] = [];
	private readonly layouts = new Set<LayoutListener>();
	private closed = false;
	readonly traffic = { sent: 0, received: 0 };

	constructor(binary: string, args: readonly string[] = []) {
		this.child = spawn(binary, ["serve", ...args], {
			stdio: ["pipe", "pipe", "pipe"],
		});
		this.child.stderr.resume();
		this.child.on("error", (error) => this.fail(error));
		this.child.on("exit", () =>
			this.fail(new Error("the engine ended")),
		);
        const frames = new Frames(answer => this.receive(answer));
        this.child.stdout.on("data", (chunk: Buffer) => {
            if (this.closed) return;
            this.traffic.received += chunk.length;
            try { frames.push(chunk); }
            catch (error) { this.fail(error as Error); }
        });
        this.child.stdout.on("end", () => {
            try { frames.finish(); }
            catch (error) { this.fail(error as Error); }
        });
	}

	/** Whether the engine process is still running. */
	get running(): boolean {
		return !this.closed && this.child.exitCode === null;
	}

	/** Registers a listener for the geometry the engine republishes. */
	onLayout(listener: LayoutListener): () => void {
		this.layouts.add(listener);
		return () => this.layouts.delete(listener);
	}

	/** Takes the client's text and answers with the document's geometry. */
	open(
		id: string,
		text: string,
		options: {
			path?: string;
			settings?: DocumentSettings;
			template?: string;
			stylesheet?: string;
			settle?: boolean;
		} = {},
	): Promise<State> {
		return this.request<{ opened: State }>({
			open: { id, text, ...options },
		}).then((answer) => answer.opened);
	}

	/** Releases a document's state in the engine. */
	close(id: string): Promise<void> {
		return this.request({ close: { id } }).then(() => undefined);
	}

	/** Renders a crop of the document, as the client asked for it. */
	tile(
		id: string,
		request: {
			width: number;
			height: number;
			scroll?: number;
			scale?: number;
		},
	): Promise<Tile> {
		return this.request<{ tile: Tile }>({ tile: { id, ...request } }).then(
			(answer) => answer.tile,
		);
	}

	/** The text layer over a band, for selection and for both mappings. */
	text(id: string, top = 0, bottom?: number): Promise<TextLayer> {
		return this.request<{ text: TextLayer }>({
			text: { id, top, ...(bottom === undefined ? {} : { bottom }) },
		}).then((answer) => answer.text);
	}

    /** Compact reading text for find; source offsets remain UTF-8 bytes. */
    rendered(id: string): Promise<{ text: string; ranges: [number, number, number][] }> {
        return this.request<{ rendered: { text: string; ranges: [number, number, number][] } }>({ rendered: { id } }).then((answer) => answer.rendered);
    }

	/** Sets the palette, or a whole stylesheet, and reports what it cost. */
	appearance(request: {
		theme?: "light" | "dark";
		style?: string;
	}): Promise<Appearance> {
		return this.request<{ appearance: Appearance }>({
			appearance: request,
		}).then((answer) => answer.appearance);
	}

	/** Reports that the document was written to disk. */
	saved(id: string): Promise<{ changed: boolean }> {
		return this.request<{ saved: { changed: boolean } }>({
			saved: { id },
		}).then((answer) => answer.saved);
	}

	/**
	 * Writes the document out, in the format and with the template asked for.
	 *
	 * `template` names a stylesheet the engine has, and `stylesheet` carries
	 * one the client holds without installing it; both are layered over the
	 * bundled print sheet, the carried rules last.
	 */
	export(
		id: string,
		output: string,
		options: {
			format?: "pdf" | "png";
			template?: string;
			stylesheet?: string;
		} = {},
	): Promise<Exported> {
		return this.request<{ exported: Exported }>({
			export: {
				id,
				output,
				format: options.format ?? "pdf",
				...(options.template ? { style: options.template } : {}),
				...(options.stylesheet
					? { stylesheet: options.stylesheet }
					: {}),
			},
		}).then((answer) => answer.exported);
	}

	/** The templates the engine can name, for a host to offer. */
	styles(): Promise<Template[]> {
		return this.request<{ styles: { templates: Template[] } }>({
			styles: {},
		}).then((answer) => answer.styles.templates);
	}

	/** Ends the session. The engine also ends when this pipe closes. */
	dispose(): void {
		if (this.closed) {
			return;
		}
		this.closed = true;
		this.child.stdin.end();
		this.child.kill();
		this.fail(new Error("the session ended"));
	}

	private request<T = unknown>(message: object): Promise<T> {
		if (!this.running) {
			return Promise.reject(new Error("the engine is not running"));
		}
		return new Promise<T>((resolve, reject) => {
			this.pending.push({
				resolve: resolve as (answer: unknown) => void,
				reject,
			});
			const line = `${JSON.stringify(message)}\n`;
			this.traffic.sent += Buffer.byteLength(line);
			this.child.stdin.write(line);
		});
	}

	/**
	 * Receives a complete frame: an answer, or a notification.
	 *
	 * The engine answers each request in the order it was written and writes
	 * the answer before any layout that request provoked, so answers are
	 * matched first in, first out. A `layout` answers nothing and may arrive at
	 * any time, which is why it is recognised by name rather than position.
	 */
	private receive(answer: Record<string, unknown>): void {
		if (answer.layout) {
			for (const listener of this.layouts) {
				listener(answer.layout as State);
			}
			return;
		}
		const waiting = this.pending.shift();
		if (!waiting) {
			return;
		}
		if (typeof answer.error === "string") {
			waiting.reject(new Error(answer.error));
			return;
		}
		waiting.resolve(answer);
	}

	private fail(error: Error): void {
		this.closed = true;
		this.child.kill();
		for (const waiting of this.pending) {
			waiting.reject(error);
		}
		this.pending.length = 0;
	}
}
