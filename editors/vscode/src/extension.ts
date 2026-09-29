/**
 * The extension: one engine per window, and the commands that drive it.
 *
 * The engine is a process, and starting it is the expensive part of a
 * preview. It is therefore created on first use and kept for the life of the
 * window, shared by every document the window previews, and ended when the
 * window ends. The extension host's own death ends it too: the engine exits
 * when its pipe closes, so a host that crashes cannot leave one behind.
 */
import * as vscode from "vscode";
import { Session, type DocumentSettings } from "./sidecar.js";
import { registerExports } from "./export.js";
import { panelReport, panelExported } from "./panel.js";

let engine: Session | undefined;
let starting: Promise<Session> | undefined;
const contextKey = "markview.engineFailed";

/** The engine executable: the configured one, or the one shipped here. */
function enginePath(context: vscode.ExtensionContext): string {
	const configured = vscode.workspace
		.getConfiguration("markview")
		.get<string>("enginePath");
	if (configured && configured.trim() !== "") {
		return configured;
	}
	const platform = `${process.platform}-${process.arch}`;
	const name = process.platform === "win32" ? "markview.exe" : "markview";
	return context.asAbsolutePath(`bin/${platform}/${name}`);
}

/**
 * The window's engine, started on first use.
 *
 * A second caller while the first is starting waits for the same promise, so a
 * burst of previews cannot start two engines.
 */
async function sidecar(
	context: vscode.ExtensionContext,
): Promise<Session> {
	if (engine?.running) {
		return engine;
	}
	if (starting) {
		return starting;
	}
	starting = (async () => {
		try {
			// The engine keeps its own settings, styles, fonts and image cache
			// under the extension's storage, so nothing it writes can be
			// confused with a reader the user runs themselves.
			const session = new Session(enginePath(context), [
				"--state-dir",
				context.globalStorageUri.fsPath,
			]);
			engine = session;
			await vscode.commands.executeCommand(
				"setContext",
				contextKey,
				false,
			);
			return session;
		} catch (error) {
			await vscode.commands.executeCommand(
				"setContext",
				contextKey,
				true,
			);
			throw error;
		} finally {
			starting = undefined;
		}
	})();
	return starting;
}

/** The editor's own light or dark, which is what the preview follows. */
function themeName(): "light" | "dark" {
	const kind = vscode.window.activeColorTheme.kind;
	return kind === vscode.ColorThemeKind.Light ||
		kind === vscode.ColorThemeKind.HighContrastLight
		? "light"
		: "dark";
}

/** Sends the editor's current appearance to the engine. */
async function followTheme(
	context: vscode.ExtensionContext,
): Promise<void> {
	const session = engine?.running ? engine : undefined;
	if (session) {
		const theme = themeName();
		const answer = await session.appearance({ theme });
		const { panelAppearance } = await import("./panel.js");
		panelAppearance(theme, answer.reflow);
	}
	void context;
}

export function activate(context: vscode.ExtensionContext): unknown {
	const exportDocument = registerExports(context, () => sidecar(context), panelExported);
	context.subscriptions.push(
		vscode.window.registerWebviewPanelSerializer("markview.preview", {
			async deserializeWebviewPanel(panel, saved) {
				const state = saved as { uri?: unknown; scroll?: unknown } | undefined;
				try {
					if (typeof state?.uri !== "string") throw new Error("The preview has no saved document.");
					const document = await vscode.workspace.openTextDocument(vscode.Uri.parse(state.uri));
					const session = await sidecar(context);
					await followTheme(context);
					const { openPreview } = await import("./panel.js");
					await openPreview(context, session, document, {
						panel,
						scroll: typeof state.scroll === "number" && Number.isFinite(state.scroll) ? Math.max(0, state.scroll) : 0,
					});
				} catch (error) {
					panel.dispose();
					void vscode.window.showErrorMessage(`Could not restore Markview preview: ${String(error)}`);
				}
			},
		}),
		vscode.commands.registerCommand(
			"markview.openPreview",
			async (uri?: vscode.Uri) => {
				// The editor title hands over the resource it was clicked on;
				// the palette hands over nothing, and the document in front of
				// the reader is what is meant then.
				const document = uri
					? await vscode.workspace.openTextDocument(uri)
					: vscode.window.activeTextEditor?.document;
				if (!document) {
					void vscode.window.showInformationMessage(
						"Open a Markdown document first.",
					);
					return;
				}
				const session = await sidecar(context);
				await followTheme(context);
				// The panel is what draws; it is added in `panel.ts`.
				const { openPreview } = await import("./panel.js");
				await openPreview(context, session, document);
			},
		),
		vscode.commands.registerCommand("markview.copySelection", async () => {
			const { copyPreviewSelection } = await import("./panel.js");
			await copyPreviewSelection();
		}),
		vscode.commands.registerCommand("markview.closePreview", async () => {
			const { closePreview } = await import("./panel.js");
			closePreview();
		}),
		vscode.commands.registerCommand("markview.revealSource", async () => {
			const { revealSelection } = await import("./panel.js");
			await revealSelection();
		}),
		vscode.window.onDidChangeActiveColorTheme(async () => {
			await followTheme(context);
		}),
	);
	// What the panel has observed, so a test can assert against it rather than
	// against a second account of it. `scrollPreviewTo` is the same entry
	// point the reveal command uses to move the view.
	return {
		panelReport,
		engineTraffic: () => ({ ...engine?.traffic }),
		exportDocument,
		templates: async () => {
			const session = await sidecar(context);
			return session.styles();
		},
		scrollPreviewTo: async (offset: number) => {
			const { scrollPreviewTo } = await import("./panel.js");
			await scrollPreviewTo(offset);
		},
		clearPreviewSelection: async () => {
			const { clearPreviewSelection } = await import("./panel.js");
			await clearPreviewSelection();
		},
		clickPreviewAt: async (x: number, y: number) => {
			const { clickPreviewAt } = await import("./panel.js");
			await clickPreviewAt(x, y);
		},
		dragPreviewAt: async (
			from: { x: number; y: number },
			to: { x: number; y: number },
		) => {
			const { dragPreviewAt } = await import("./panel.js");
			await dragPreviewAt(from, to);
		},
		copyPreviewSelection: async () => {
			const { copyPreviewSelection } = await import("./panel.js");
			await copyPreviewSelection();
		},
		findInPreview: async (needle: string, previous = false) => {
			const { findInPreview } = await import("./panel.js");
			await findInPreview(needle, previous);
		},
	};
}

export function deactivate(): void {
	// The engine's own pipe closing would end it, but a window that closes
	// cleanly can say so rather than making it wait.
	engine?.dispose();
	engine = undefined;
}
