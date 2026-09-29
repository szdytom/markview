import * as vscode from "vscode";
import * as path from "node:path";

/** An explicit preview setting wins, including an empty value. */
export function configuredTemplate(document: vscode.TextDocument): string {
	const configuration = vscode.workspace.getConfiguration("markview", document);
	const value = configuration.inspect<string>("template");
	const explicit = value && [value.globalValue, value.workspaceValue,
		value.workspaceFolderValue, value.globalLanguageValue,
		value.workspaceLanguageValue, value.workspaceFolderLanguageValue]
		.some((entry) => entry !== undefined);
	return (explicit ? configuration.get<string>("template") :
		vscode.workspace.getConfiguration("markviewExport", document).get<string>("template"))?.trim() ?? "";
}

/** Resolve one template for both preview and export; the engine resolves MVSS. */
export async function templateFor(
	request: { template?: string | null; stylesheet?: string },
	document: vscode.TextDocument,
): Promise<{ template?: string; stylesheet?: string }> {
	if (request.stylesheet !== undefined) return { stylesheet: request.stylesheet };
	const name = (request.template !== undefined ? request.template ?? "" : configuredTemplate(document)).trim();
	if (!name) return {};
	if (name.endsWith(".mvss.toml") || name.includes("/") || name.includes("\\")) {
		const base = document.isUntitled
			? vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath
			: path.dirname(document.uri.fsPath);
		if (!path.isAbsolute(name) && !base) throw new Error("Save the document or use an absolute template path.");
		const bytes = await vscode.workspace.fs.readFile(vscode.Uri.file(path.resolve(base ?? "", name)));
		if (request.template === undefined && name !== configuredTemplate(document)) {
			return templateFor(request, document);
		}
		return { stylesheet: new TextDecoder().decode(bytes) };
	}
	return { template: name };
}
