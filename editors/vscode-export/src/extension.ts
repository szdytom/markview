import * as path from "node:path";
import * as vscode from "vscode";
import { Session } from "../../shared/sidecar.js";

interface ExportRequest {
	uri?: vscode.Uri;
	target?: vscode.Uri;
	format?: "pdf" | "png";
	template?: string | null;
	stylesheet?: string;
}

let engine: Session | undefined;
let sequence = 0;

function session(context: vscode.ExtensionContext): Session {
	if (!engine?.running) {
		const platform = `${process.platform}-${process.arch}`;
		const name = process.platform === "win32" ? "markview.exe" : "markview";
		engine = new Session(context.asAbsolutePath(`bin/${platform}/${name}`), [
			"--state-dir", context.globalStorageUri.fsPath,
		]);
	}
	return engine;
}

async function chooseTemplate(context: vscode.ExtensionContext) {
	const templates = await session(context).styles();
	const picked = await vscode.window.showQuickPick([
		{ label: "None", description: "Bundled print sheet", id: "" },
		...templates.filter((entry) => !entry.error).map((entry) => ({
			label: entry.name || entry.id, description: entry.id, id: entry.id,
		})),
		{ label: "Choose a template file…", description: ".mvss.toml", id: undefined },
	], { title: "Markview: Choose export template" });
	if (!picked) return undefined;
	if (picked.id !== undefined) return { template: picked.id || null };
	const files = await vscode.window.showOpenDialog({
		canSelectMany: false, filters: { "MVSS template": ["toml"] },
	});
	if (!files?.[0]) return undefined;
	return { stylesheet: new TextDecoder().decode(await vscode.workspace.fs.readFile(files[0])) };
}

async function templateFor(request: ExportRequest, document: vscode.TextDocument) {
	if (request.stylesheet !== undefined) return { stylesheet: request.stylesheet };
	const name = (request.template !== undefined ? request.template :
		vscode.workspace.getConfiguration("markviewExport", document).get<string>("template"))?.trim();
	if (!name) return {};
	if (name.endsWith(".mvss.toml")) {
		const base = document.isUntitled
			? vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
			: path.dirname(document.uri.fsPath);
		if (!path.isAbsolute(name) && !base) throw new Error("Save the document or use an absolute template path.");
		const file = vscode.Uri.file(path.resolve(base ?? "", name));
		return { stylesheet: new TextDecoder().decode(await vscode.workspace.fs.readFile(file)) };
	}
	return { template: name };
}

async function exportDocument(context: vscode.ExtensionContext, request: ExportRequest, pick = false) {
	const document = request.uri ? await vscode.workspace.openTextDocument(request.uri) : vscode.window.activeTextEditor?.document;
	if (!document || document.languageId !== "markdown") throw new Error("Open a Markdown document first.");
	if (!document.isUntitled && document.uri.scheme !== "file") throw new Error("Export supports local Markdown documents only.");
	// Capture the buffer before a picker changes the active editor.
	const text = document.getText();
	let format = request.format ?? "pdf";
	if (pick) {
		const chosen = await chooseTemplate(context);
		if (!chosen) return;
		const selected = await vscode.window.showQuickPick(["PDF", "PNG"], { title: "Markview: Export as" });
		if (!selected) return;
		format = selected === "PNG" ? "png" : "pdf";
		request = { ...request, ...chosen };
	}
	const base = document.isUntitled
		? vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
		: path.dirname(document.uri.fsPath);
	const source = document.isUntitled ? path.join(base ?? "", "Untitled.md") : document.uri.fsPath;
	const defaultPath = source.replace(/\.(md|markdown)$/i, "") + `.${format}`;
	const target = request.target ?? await vscode.window.showSaveDialog({
		defaultUri: vscode.Uri.file(path.resolve(defaultPath)),
		filters: format === "pdf" ? { PDF: ["pdf"] } : { PNG: ["png"] },
	});
	if (!target) return;
	if (target.scheme !== "file") throw new Error("Choose a local output file.");
	if (!document.isUntitled && path.resolve(target.fsPath) === path.resolve(source)) throw new Error("Choose an output file different from the Markdown source.");
	const rules = await templateFor(request, document);
	const active = session(context);
	const id = `export-${sequence++}`;
	await active.open(id, text, { path: document.isUntitled && !base ? path.join(path.dirname(target.fsPath), "Untitled.md") : source });
	try {
		const result = await active.export(id, target.fsPath, { format, ...rules });
		await vscode.commands.executeCommand("revealFileInOS", target);
		void vscode.window.showInformationMessage(`Exported ${path.basename(target.fsPath)} (${Math.round(result.bytes / 1024)} KB)`);
		return result;
	} finally {
		await active.close(id);
	}
}

export function activate(context: vscode.ExtensionContext) {
	for (const [name, format, pick] of [
		["exportPdf", "pdf", false], ["exportPng", "png", false], ["exportWithTemplate", "pdf", true],
	] as const) {
		context.subscriptions.push(vscode.commands.registerCommand(`markviewExport.${name}`, async (argument: ExportRequest | vscode.Uri = {}) => {
			const request = argument instanceof vscode.Uri ? { uri: argument } : argument;
			try {
				return await exportDocument(context, { ...request, format }, pick);
			} catch (error) {
				void vscode.window.showErrorMessage(`Export failed: ${error instanceof Error ? error.message : String(error)}`);
				throw error;
			}
		}));
	}
	return {
		exportDocument: (request: ExportRequest) => exportDocument(context, request),
		templates: () => session(context).styles(),
	};
}

export function deactivate() {
	engine?.dispose();
	engine = undefined;
}
