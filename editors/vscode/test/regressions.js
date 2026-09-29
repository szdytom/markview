// Exercises browser find and races through a real extension host.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(check, label) {
	for (let i = 0; i < 100; i++) {
		if (check()) return;
		await delay(100);
	}
	assert.fail(label);
}
exports.run = async (extension, scratch) => {
	const api = extension.exports;
	const documentPath = path.join(scratch, "native-find.md");
	const paragraph = "A normal paragraph gives the preview enough vertical space to test navigation.\n\n";
	const text = "# Native find\n\nAudit Needle first.\n\n" + paragraph.repeat(80) + "Audit Needle second.\n\n" + paragraph.repeat(80) + "Audit Needle third.\n";
	fs.writeFileSync(documentPath, text);
	const document = await vscode.workspace.openTextDocument(documentPath);
	const sourceEditor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
	await vscode.commands.executeCommand("markview.openPreview", document.uri);
	await until(() => api.panelReport().rendered.includes("Audit Needle third."), "find index ready");
	await until(() => api.panelReport().paintedVersion === document.version, "preview tiles painted");
	const bounds = api.panelReport().tileBounds;
	assert.ok(bounds.length > 0);
	for (const tile of bounds) {
		assert.ok(tile.width > 0 && tile.expectedHeight > 0);
		assert.ok(Math.abs(tile.height - tile.expectedHeight) < 0.1, "VS Code image defaults must not collapse preview tiles");
		assert.equal(tile.pixels, tile.expectedPixels, "native raster density matches the display");
	}
	console.log("MARKVIEW-TILE-BOUNDS", JSON.stringify(bounds));
	sourceEditor.selection = new vscode.Selection(2, 0, 2, 5);
	await until(() => api.panelReport().selection?.text === "Audit", "a selection exists in the visible overlay before find");
	const positions = [...text.matchAll(/Audit Needle/g)].map((match) => match.index);
	for (const [previous, index] of [[false, 0], [false, 1], [false, 2], [true, 1], [false, 2], [false, 0], [true, 2]]) {
		// VS Code's webview widget calls this same Chromium API. No match is
		// injected into a DOM Range, and no JavaScript text search is used.
		await api.findInPreview("Audit Needle", previous);
		await until(() => api.panelReport().revealed === positions[index], `native find ${previous ? "previous" : "next"} reaches ${index}`);
		await delay(200);
		assert.equal(api.panelReport().revealed, positions[index]);
		console.log("MARKVIEW-NATIVE-FIND", JSON.stringify({ previous, index, scroll: api.panelReport().scroll, source: positions[index] }));
	}

	const deepPath = path.join(scratch, "deep-find.md");
	const deepText = "A long paragraph with enough ordinary words to fill a line. ".repeat(500) + "DeepFindNeedle";
	fs.writeFileSync(deepPath, deepText);
	const deep = await vscode.workspace.openTextDocument(deepPath);
	await vscode.window.showTextDocument(deep, vscode.ViewColumn.One);
	await vscode.commands.executeCommand("markview.openPreview", deep.uri);
	await until(() => api.panelReport().rendered.endsWith("DeepFindNeedle"), "deep find text ready");
	await api.findInPreview("DeepFindNeedle");
	await until(() => api.panelReport().revealed === deepText.indexOf("DeepFindNeedle"), "deep source match revealed");
	await until(() => api.panelReport().scroll > api.panelReport().documentHeight - api.panelReport().viewport - 100, "deep match row is visible, not merely the paragraph start");
	console.log("MARKVIEW-NATIVE-FIND deep-row", api.panelReport().scroll);

	const entityPath = path.join(scratch, "entity-selection.md");
	fs.writeFileSync(entityPath, "a &fjlig; b");
	const entity = await vscode.workspace.openTextDocument(entityPath);
	const editor = await vscode.window.showTextDocument(entity, vscode.ViewColumn.One);
	await vscode.commands.executeCommand("markview.openPreview", entity.uri);
	await until(() => api.panelReport().text === "a fj b", "both entity clusters remain in the selection layer");
	editor.selection = new vscode.Selection(0, 2, 0, 9);
	await until(() => api.panelReport().selection?.text === "fj", "entity selection is complete");
	const clipboard = await vscode.env.clipboard.readText();
	try {
		await vscode.commands.executeCommand("markview.copySelection");
		await until(() => api.panelReport().copied, "entity selection copied");
		assert.equal(await vscode.env.clipboard.readText(), "fj");
	} finally { await vscode.env.clipboard.writeText(clipboard); }

	const racePath = path.join(scratch, "opening-race.md");
	fs.writeFileSync(racePath, "old text");
	const racing = await vscode.workspace.openTextDocument(racePath);
	const raceEditor = await vscode.window.showTextDocument(racing, vscode.ViewColumn.One);
	const { Session } = require(path.join(extension.extensionPath, "out/shared/sidecar.js"));
	const original = Session.prototype.open;
	let blocked;
	let release;
	const entered = new Promise((resolve) => { blocked = resolve; });
	const gate = new Promise((resolve) => { release = resolve; });
	let first = true;
	Session.prototype.open = async function (id, value, options) {
		const state = await original.call(this, id, value, options);
		if (options?.path === racePath && first) {
			first = false;
			blocked();
			await gate;
		}
		return state;
	};
	try {
		const opening = vscode.commands.executeCommand("markview.openPreview", racing.uri);
		await entered;
		assert.ok(await raceEditor.edit((edit) => edit.replace(new vscode.Range(0, 0, 0, 8), "new unsaved text")));
		release();
		await opening;
		await until(() => api.panelReport().paintedVersion === racing.version && api.panelReport().text === "new unsaved text", `an edit during initial open reaches pixels without a second edit: ${JSON.stringify({uri:api.panelReport().documentUri, version:api.panelReport().paintedVersion, tileError:api.panelReport().tileError, text:api.panelReport().text})}`);
	} finally {
		release();
		Session.prototype.open = original;
	}
	await require("./repaint.js").run(extension, scratch);
	await require("./templates.js").run(extension);
	if (process.env.MARKVIEW_SKIP_SCROLL_SYNC === "1") {
		console.log("MARKVIEW-SKIP EXT-5 scroll-top deferred; not validated");
	} else {
		await require("./scroll-top.js").run(extension, scratch);
	}
	await require("./fit.js").run(extension, scratch);
    await require("./follow-editor.js").run(extension);
    await require("./export-compat.js").run(extension, scratch);
	console.log("MARKVIEW-REGRESSIONS ok native-find entity-copy initial-open-race");
};
