// Source viewport boundaries must map to preview boundaries, not text rows.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, label) {
	for (let i = 0; i < 80; i++) { if (check()) return; await delay(100); }
	assert.fail(label);
}
exports.run = async (extension, scratch) => {
	const api = extension.exports;
	for (const [name, prefix] of [["heading", "# Heading\n\n"], ["leading-blank", "\n\n# Heading\n\n"]]) {
		const file = path.join(scratch, `scroll-top-${name}.md`);
		fs.writeFileSync(file, prefix + "A paragraph for scrolling.\n\n".repeat(160));
		const document = await vscode.workspace.openTextDocument(file);
		const editor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
		await vscode.commands.executeCommand("markview.openPreview", document.uri);
		await until(() => api.panelReport().paintedVersion === document.version, "preview painted");
		await delay(700);
		const reveal = line => editor.revealRange(new vscode.Range(line, 0, line, 0), vscode.TextEditorRevealType.AtTop);
		reveal(100);
		await until(() => api.panelReport().scroll > 500, "source scroll moves preview down");
		await delay(400);
		const caret = editor.selection.active;
		reveal(0);
		await until(() => editor.visibleRanges[0].start.line === 0, "source reaches the top");
		await until(() => api.panelReport().scroll === 0, `${name}: source top returns preview to pixel zero`);
		await delay(600);
		assert.equal(api.panelReport().scroll, 0, "preview stays at the top after echo suppression");
		assert.ok(editor.selection.active.isEqual(caret), "scroll synchronization leaves caret alone");
		await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
		reveal(100);
		await until(() => api.panelReport().scroll > 500, "source moves into prose");
		await delay(700);
		const beforeSmall = api.panelReport().scroll;
		const beforeLine = editor.visibleRanges[0].start.line;
		reveal(beforeLine + 7);
		await delay(500);
		console.log("MARKVIEW-SMALL-SCROLL", {beforeLine, afterLine:editor.visibleRanges[0].start.line, beforeSmall, after:api.panelReport().scroll});
		await until(() => api.panelReport().scroll > beforeSmall + 1, "small source scroll follows continuously");
		assert.ok(api.panelReport().scroll - beforeSmall < 120, "small move is inside the former dead zone");
		await delay(700);
		const sourceTop = editor.visibleRanges[0].start.line;
		const target = api.panelReport().scroll + 200;
		await api.scrollPreviewTo(target);
		await delay(700);
		console.log("MARKVIEW-SCROLL-FOLLOW", {sourceTop, after:editor.visibleRanges[0].start.line, target, scroll:api.panelReport().scroll, carried:api.panelReport().carried, syncs:api.panelReport().syncs});
		await until(() => editor.visibleRanges[0].start.line > sourceTop, "preview moves source even when target is already visible");
		await delay(700);
		const landed = api.panelReport();
		const tile = landed.tileBounds[0];
		assert.ok(tile?.scale > 0 && tile?.fit > 0, "painted display scale is known");
		const pixel = 1 / (tile.scale * tile.fit);
		assert.ok(Math.abs(landed.scroll - target) <= pixel + 0.01,
			`source follow stays within one display pixel: ${landed.scroll} against ${target}, pixel ${pixel}`);
		assert.ok(editor.selection.active.isEqual(caret), "continuous follow leaves caret alone");
		if (name === "heading") {
			const { Session } = require(path.join(extension.extensionPath, "out/shared/sidecar.js"));
			const original = Session.prototype.text;
			let entered = false;
			let release;
			const gate = new Promise(resolve => { release = resolve; });
			Session.prototype.text = async function(id, top, bottom) {
				const layer = await original.call(this, id, top, bottom);
				if (!entered && top > 1000) { entered = true; await gate; }
				return layer;
			};
			try {
				reveal(250);
				await until(() => entered, "a distant scroll waits for native row mapping");
				reveal(0);
				await until(() => editor.visibleRanges[0].start.line === 0, "newer scroll reaches source top");
				release();
				await delay(700);
				assert.equal(api.panelReport().scroll, 0, "an older mapping cannot override the top scroll");
			} finally { release(); Session.prototype.text = original; }
		}
	}
	console.log("MARKVIEW-SCROLL-TOP ok heading leading-blank near-top small-scroll visible-target caret no-bounce stale-mapping");
};
