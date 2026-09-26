/** The export host's newline-delimited JSON client for the bundled engine. */
import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { createInterface, type Interface } from "node:readline";

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
	private readonly lines: Interface;
	private readonly pending: Pending[] = [];
	private closed = false;

	constructor(binary: string, args: readonly string[] = []) {
		this.child = spawn(binary, ["serve", ...args], {
			stdio: ["pipe", "pipe", "pipe"],
		});
		this.child.stderr.resume();
		this.child.on("error", (error) => this.fail(error));
		this.child.on("exit", () =>
			this.fail(new Error("the engine ended")),
		);
		this.lines = createInterface({ input: this.child.stdout });
		this.lines.on("line", (line) => this.receive(line));
	}

	/** Whether the engine process is still running. */
	get running(): boolean {
		return !this.closed && this.child.exitCode === null;
	}

	/** Retains the client's buffer until it is exported or closed. */
	open(
		id: string,
		text: string,
		options: {
			path: string;
		},
	): Promise<{ id: string }> {
		return this.request<{ opened: { id: string } }>({
			open: { id, text, ...options },
		}).then((answer) => answer.opened);
	}

	/** Releases a document's state in the engine. */
	close(id: string): Promise<void> {
		return this.request({ close: { id } }).then(() => undefined);
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
		this.lines.close();
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
			this.child.stdin.write(`${JSON.stringify(message)}\n`);
		});
	}

	/** Matches replies in the order requests were written. */
	private receive(line: string): void {
		let answer: Record<string, unknown>;
		try {
			answer = JSON.parse(line);
		} catch {
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
		for (const waiting of this.pending) {
			waiting.reject(error);
		}
		this.pending.length = 0;
	}
}
