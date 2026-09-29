import * as path from "node:path";
import * as vscode from "vscode";
import { templateFor } from "./template.js";
import { Session } from "../../shared/sidecar.js";

interface ExportRequest {
	uri?: vscode.Uri;
	target?: vscode.Uri;
	format?: "pdf" | "png";
	template?: string | null;
	stylesheet?: string;
}

let sequence = 0;

export function registerExports(context: vscode.ExtensionContext, getSession: () => Promise<Session>, exported: (path: string) => void) {
	async function chooseTemplate() {
		const templates = await (await getSession()).styles();
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

	async function exportDocument(request: ExportRequest, pick = false) {
		const document = request.uri ? await vscode.workspace.openTextDocument(request.uri) : vscode.window.activeTextEditor?.document;
		if (!document || document.languageId !== "markdown") throw new Error("Open a Markdown document first.");
		if (!document.isUntitled && document.uri.scheme !== "file") throw new Error("Export supports local Markdown documents only.");
		// Capture the buffer before a picker changes the active editor.
		const text = document.getText();
		let format = request.format ?? "pdf";
		if (pick) {
			const chosen = await chooseTemplate();
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
		const active = await getSession();
		const id = `export-${sequence++}`;
		await active.open(id, text, { path: document.isUntitled && !base ? path.join(path.dirname(target.fsPath), "Untitled.md") : source });
		try {
			const result = await active.export(id, target.fsPath, { format, ...rules });
			exported(target.fsPath);
			await vscode.commands.executeCommand("revealFileInOS", target);
			void vscode.window.showInformationMessage(`Exported ${path.basename(target.fsPath)} (${Math.round(result.bytes / 1024)} KB)`);
			return result;
		} finally {
			await active.close(id);
		}
	}


	for (const prefix of ["markview", "markviewExport"]) {
		for (const [name, format, pick] of [
			["exportPdf", "pdf", false], ["exportPng", "png", false], ["exportWithTemplate", "pdf", true],
		] as const) {
			context.subscriptions.push(vscode.commands.registerCommand(`${prefix}.${name}`, async (argument: ExportRequest | vscode.Uri = {}) => {
				const request = argument instanceof vscode.Uri ? { uri: argument } : argument;
				try {
					return await exportDocument({ ...request, format }, pick);
				} catch (error) {
					void vscode.window.showErrorMessage(`Export failed: ${error instanceof Error ? error.message : String(error)}`);
					throw error;
				}
			}));
		}
	}
	return (request: ExportRequest) => exportDocument(request);
}
