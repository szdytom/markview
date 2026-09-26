// Runs against the installed VSIX in a real VS Code extension host.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

exports.run = async () => {
	const vscode = require("vscode");
	const folder = vscode.workspace.workspaceFolders[0].uri.fsPath;
	const manifest = require("../package.json");
	const extension = vscode.extensions.getExtension(`${manifest.publisher}.${manifest.name}`);
	assert.ok(extension);
	assert.ok(extension.extensionPath.includes("/ext/"), "test the installed package");
	assert.ok(fs.existsSync(path.join(extension.extensionPath, "bin/darwin-arm64/markview")));
	assert.ok(!fs.existsSync(path.join(extension.extensionPath, "out/vscode/src/panel.js")));
	const api = await extension.activate();
	const source = path.join(folder, "sample.md");
	fs.writeFileSync(source, "# Disk version\n\nOriginal text.\n");
	const document = await vscode.workspace.openTextDocument(source);
	await vscode.window.showTextDocument(document);
	const templates = await api.templates();
	assert.ok(templates.some((entry) => entry.id === "mondrian"));
	assert.ok(templates.every((entry) => !entry.error));
	const output = (name) => vscode.Uri.file(path.join(folder, name));
	await vscode.commands.executeCommand("markviewExport.exportPng", { target: output("disk.png"), template: null });
	const edit = new vscode.WorkspaceEdit();
	edit.replace(document.uri, new vscode.Range(0, 0, document.lineCount, 0), "# Unsaved buffer\n\n中文、café and **native typography**.\n\n" + "An extra paragraph.\n\n".repeat(8));
	assert.ok(await vscode.workspace.applyEdit(edit));
	assert.ok(document.isDirty);
	await vscode.commands.executeCommand("markviewExport.exportPng", { target: output("buffer.png"), template: null });
	assert.notDeepEqual(fs.readFileSync(output("disk.png").fsPath), fs.readFileSync(output("buffer.png").fsPath));
	assert.equal(fs.readFileSync(source, "utf8"), "# Disk version\n\nOriginal text.\n");
	await vscode.commands.executeCommand("markviewExport.exportPdf", { target: output("mondrian.pdf"), template: "mondrian" });
	await vscode.commands.executeCommand("markviewExport.exportPng", { target: output("mondrian.png"), template: "mondrian" });
	for (const name of ["mondrian.pdf", "mondrian.png"]) {
		const bytes = fs.readFileSync(output(name).fsPath);
		assert.ok(bytes.length > 1000);
		assert.equal(bytes.subarray(0, 4).toString("hex"), name.endsWith("pdf") ? "25504446" : "89504e47");
	}
	const template = path.join(folder, "house.mvss.toml");
	fs.writeFileSync(template, 'format_version = 2\ntargets = ["pdf"]\nversion = 1\n[[rule]]\nwhen = ["body"]\ncolor = "#0B3D2E"\nline_height = 1.9\n');
	// The workspace's `[markdown]` layer names this relative template.
	assert.equal(vscode.workspace.getConfiguration("markviewExport", document).get("template"), "house.mvss.toml");
	await vscode.commands.executeCommand("markviewExport.exportPdf", { target: output("custom.pdf") });
	await vscode.commands.executeCommand("markviewExport.exportPng", { target: output("custom.png") });
	await api.exportDocument({ target: output("none.png"), format: "png", template: null });
	assert.deepEqual(fs.readFileSync(output("none.png").fsPath), fs.readFileSync(output("buffer.png").fsPath), "None bypasses the configured template");
	assert.notDeepEqual(fs.readFileSync(output("custom.png").fsPath), fs.readFileSync(output("none.png").fsPath));
	await assert.rejects(api.exportDocument({ target: output("unknown.pdf"), template: "no-such-template" }));
	assert.ok(!fs.existsSync(output("unknown.pdf").fsPath));
	await assert.rejects(api.exportDocument({ target: document.uri }), /different from/);
	const untitled = await vscode.workspace.openTextDocument({ language: "markdown", content: "# Untitled\n\nExport without saving.\n" });
	await api.exportDocument({ uri: untitled.uri, target: output("untitled.pdf"), template: null });
	// Equivalent SVGs must export identically after physical raster demand settles.
	for (const [name, size] of [["tiny", 10], ["large", 300]]) {
		fs.writeFileSync(path.join(folder, `${name}.svg`), `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 10 10"><rect width="10" height="10" fill="white"/><path d="M1 1L9 9M1 9L9 1" stroke="black" stroke-width=".1"/></svg>`);
		const source = path.join(folder, `${name}.md`);
		fs.writeFileSync(source, `<img src="${name}.svg" width="300" height="300">\n\n${"paragraph\n\n".repeat(300)}`);
		await api.exportDocument({ uri: vscode.Uri.file(source), target: output(`${name}.png`), format: "png", template: null });
	}
	assert.deepEqual(fs.readFileSync(output("tiny.png").fsPath), fs.readFileSync(output("large.png").fsPath), "SVG rasterization settles before later tiles replace demand");
	const sheet = 'format_version = 2\nversion = 1\ntargets = ["pdf"]\n[page]\nsize = "a5"\nmargin = [5, 5, 5, 5]\nheader_center = "{path}"\n';
	await api.exportDocument({ uri: document.uri, target: output("a5.pdf"), stylesheet: sheet });
	await api.exportDocument({ uri: document.uri, target: output("a5.png"), format: "png", stylesheet: sheet });
	assert.equal(fs.readFileSync(output("a5.png").fsPath).readUInt32BE(16), Math.round(148 / 25.4 * 96 * 2));
	const readonly = path.join(folder, "readonly");
	fs.mkdirSync(readonly);
	const readonlySource = path.join(readonly, "source.md");
	fs.writeFileSync(readonlySource, "# Read-only source\n\nExport outside this directory.");
	fs.chmodSync(readonly, 0o555);
	try {
		for (const format of ["pdf", "png"]) await api.exportDocument({ uri: vscode.Uri.file(readonlySource), target: output(`readonly.${format}`), format, template: null });
	} finally { fs.chmodSync(readonly, 0o755); }
	const processes = execFileSync("pgrep", ["-fl", path.join(extension.extensionPath, "bin/darwin-arm64/markview")], { encoding: "utf8" }).trim().split("\n");
	assert.equal(processes.length, 1, "one bundled engine is reused");
	assert.ok(!vscode.window.tabGroups.all.flatMap((group) => group.tabs).some((tab) => tab.input instanceof vscode.TabInputWebview));
	console.log("MARKVIEW-EXPORT ok true " + JSON.stringify({ templates: templates.length, files: folder, engine: processes[0] }));
};
