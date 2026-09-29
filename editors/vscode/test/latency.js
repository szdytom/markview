// Measure an editor buffer edit through decoded visible tiles and a paint frame.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function painted(api, version, deadline = 30000) {
	const end = performance.now() + deadline;
	while (performance.now() < end) {
		const report = api.panelReport();
		if (report.paintedVersion === version) return report.paintedAt;
		await pause(10);
	}
	assert.fail(`version ${version} did not reach the visible pixels`);
}

exports.run = async (api, scratch) => {
	const results = [];
	for (const bytes of [10000, 100000, 1000000]) {
		await vscode.commands.executeCommand("markview.closePreview");
		const file = path.join(scratch, `latency-${bytes}.md`);
		const paragraph = "A paragraph with **emphasis**, a [link](https://example.com), and words that wrap across a reading column.\n\n";
		fs.writeFileSync(file, "# Typing latency\n\n" + paragraph.repeat(Math.ceil(bytes / paragraph.length)));
		const document = await vscode.workspace.openTextDocument(file);
		const editor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
		await vscode.commands.executeCommand("markview.openPreview", document.uri);
		await painted(api, document.version);
		await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
		editor.selection = new vscode.Selection(2, 0, 2, 0);
		await vscode.commands.executeCommand("workbench.action.focusFirstEditorGroup");
		await pause(300);
		const samples = [];
		const traffic = [];
		for (let index = 0; index < 7; index += 1) {
			const version = document.version;
			const before = api.engineTraffic();
			const webviewBefore = api.panelReport().webviewBytes;
			const started = performance.now();
			await editor.edit((edit) => edit.insert(new vscode.Position(2, 0), String(index)));
			for (let attempt = 0; attempt < 100 && document.version === version; attempt += 1) await pause(10);
			assert.ok(document.version > version, "editing changed the buffer");
			const finished = await painted(api, document.version);
			assert.ok(finished >= started, "an old paint cannot satisfy this sample");
			samples.push(finished - started);
			const after = api.engineTraffic();
			traffic.push({ engine_sent: after.sent - before.sent, engine_received: after.received - before.received, webview_json: api.panelReport().webviewBytes - webviewBefore });
			await pause(500);
		}
		const sorted = [...samples].sort((a, b) => a - b);
		results.push({ bytes: Buffer.byteLength(document.getText()), samples_ms: samples, traffic_bytes: traffic, median_ms: sorted[3], p95_ms: sorted[6] });
	}
	console.log(`MARKVIEW-LATENCY ${JSON.stringify({ boundary: "VS Code TextEditor.edit call to decoded visible tiles after two animation frames, plus host acknowledgment; keyboard dispatch not measured", results })}`);
};
