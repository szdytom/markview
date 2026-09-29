const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
// Convert one physical pixel into native layout coordinates.
const pixel = tile => 1 / (tile.scale * tile.fit);
async function until(check, label) {
	for (let i = 0; i < 100; i++) { if (check()) return; await delay(100); }
	assert.fail(label);
}
exports.run = async (extension, scratch) => {
	const api = extension.exports;
	const file = path.join(scratch, "fit.md");
	fs.writeFileSync(file, "# Fit preview\n\n" + "A paragraph with [a link](https://example.com/fit) and enough text to select.\n\n".repeat(150));
	const document = await vscode.workspace.openTextDocument(file);
	const editor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
	const config = vscode.workspace.getConfiguration("markview", document);
	const beforeWidth = config.inspect("columnWidth").workspaceValue;
	const beforeTemplate = config.inspect("template").workspaceValue;
	let toggled = false;
	try {
		await config.update("columnWidth", 1600, vscode.ConfigurationTarget.Workspace);
		await config.update("template", "", vscode.ConfigurationTarget.Workspace);
		await vscode.commands.executeCommand("markview.openPreview", document.uri);
		await until(() => api.panelReport().paintedVersion === document.version && api.panelReport().tileBounds[0]?.documentWidth === 1600, "wide document painted");
		const before = api.panelReport();
		const tile = before.tileBounds[0];
		assert.deepEqual(tile.inset, [16, 16, 16, 16], "preview keeps a fixed 16px inset on all sides");
		assert.ok(tile.fit > 0 && tile.fit < 1, "wide layout shrinks to fit");
		assert.ok(Math.abs(tile.width - tile.viewportWidth) < 1, "whole page width fits the viewport");
		assert.equal(tile.pixels, tile.expectedPixels, "scaled raster matches physical display density");
		editor.selection = new vscode.Selection(2, 0, 2, 11);
		await until(() => api.panelReport().selection?.text === "A paragraph", "selection overlay follows scaled pixels");
		await api.clearPreviewSelection();
		const link = api.panelReport().links.find(link => link.kind === "remote");
		assert.ok(link, "native link geometry available");
		await api.clickPreviewAt(link.x + link.width / 2, link.y + link.height / 2);
		await until(() => api.panelReport().clicked?.kind === "remote", "scaled hit test resolves link");
		await api.scrollPreviewTo(1000);
		await until(() => Math.abs(api.panelReport().scroll - 1000) <= pixel(tile) + 0.01, "scroll uses layout coordinates");
		await delay(700);
		const old = api.panelReport();
		await vscode.commands.executeCommand("workbench.action.toggleSidebarVisibility");
		toggled = true;
		await until(() => api.panelReport().tileBounds[0]?.fit !== tile.fit, "panel resize updates fit");
		await delay(700);
		const resized = api.panelReport();
		assert.equal(resized.opens, old.opens, "window resizing does not request native reflow");
		assert.equal(resized.documentHeight, old.documentHeight, "native layout height is unchanged");
		assert.ok(Math.abs(resized.scroll - old.scroll) <= (pixel(tile) + pixel(resized.tileBounds[0])) / 2 + 0.01, `resizing preserves reading position: ${old.scroll} -> ${resized.scroll}; fit ${tile.fit} -> ${resized.tileBounds[0].fit}`);
		for (const band of resized.tileBounds) {
			assert.ok(Math.abs(band.width - band.viewportWidth) < 1, "resized page stays within the pane");
			assert.equal(band.pixels, band.expectedPixels);
			assert.deepEqual(band.inset, [16, 16, 16, 16], "pane resizing preserves the inset");
		}
		console.log("MARKVIEW-FIT ok width density selection link scroll resize no-reflow", {before:tile.fit, after:resized.tileBounds[0].fit});
	} finally {
		if (toggled) await vscode.commands.executeCommand("workbench.action.toggleSidebarVisibility");
		await config.update("columnWidth", beforeWidth, vscode.ConfigurationTarget.Workspace);
		await config.update("template", beforeTemplate, vscode.ConfigurationTarget.Workspace);
	}
};
