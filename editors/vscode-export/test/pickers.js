// Verify the command's dialog sequence; real exports are covered by `run.js`.
const assert = require("node:assert/strict");
const Module = require("node:module");
const path = require("node:path");
const commands = new Map();
const calls = [];
const answers = [];
class Uri {
	constructor(file) { this.scheme = "file"; this.fsPath = file; }
	static file(file) { return new Uri(file); }
}
const uri = Uri.file;
const document = { uri: uri("/workspace/doc.md"), languageId: "markdown", isUntitled: false, getText: () => "# Buffer" };
const vscode = {
	Uri,
	workspace: {
		openTextDocument: async (resource) => {
			assert.equal(resource, document.uri);
			return document;
		},
		getConfiguration: (name, scope) => {
			assert.equal(name, "markviewExport");
			assert.equal(scope, document);
			return { get: () => "configured-template" };
		},
		fs: { readFile: async () => Buffer.from("custom rules") },
	},
	window: {
		activeTextEditor: { document },
		showQuickPick: async (items) => {
			const answer = answers.shift();
			return typeof answer === "number" ? items[answer] : undefined;
		},
		showOpenDialog: async () => [uri("/workspace/house.mvss.toml")],
		showSaveDialog: async () => uri("/workspace/output.png"),
		showInformationMessage: () => {},
		showErrorMessage: (message) => { throw new Error(message); },
	},
	commands: {
		registerCommand: (name, callback) => { commands.set(name, callback); return { dispose() {} }; },
		executeCommand: async (name, target) => calls.push({ reveal: name, target }),
	},
};
class Session {
	running = true;
	async styles() { return [{ id: "mondrian", name: "Mondrian" }]; }
	async open(id, text) { calls.push({ open: id, text }); }
	async export(id, target, options) { calls.push({ target, options }); return { bytes: 2048 }; }
	async close(id) { calls.push({ close: id }); }
	dispose() {}
}
const original = Module._load;
Module._load = function (name, ...args) {
	if (name === "vscode") return vscode;
	if (name.endsWith("/sidecar.js")) return { Session };
	return original.call(this, name, ...args);
};
const extension = require("../out/vscode-export/src/extension.js");
Module._load = original;
extension.activate({ subscriptions: [], globalStorageUri: uri("/storage"), asAbsolutePath: (name) => path.resolve(name) });

(async () => {
	const run = commands.get("markviewExport.exportWithTemplate");
	answers.push(1, 0);
	await run();
	assert.deepEqual(calls.find((call) => call.options).options, { format: "pdf", template: "mondrian" });
	assert.equal(calls.find((call) => call.reveal).reveal, "revealFileInOS");
	calls.length = 0;
	answers.push(0, 1);
	await run();
	assert.deepEqual(calls.find((call) => call.options).options, { format: "png" }, "None bypasses the default");
	calls.length = 0;
	answers.push(2, 1);
	await run();
	assert.deepEqual(calls.find((call) => call.options).options, { format: "png", stylesheet: "custom rules" });
	for (const choices of [[undefined], [1, undefined]]) {
		calls.length = 0;
		answers.push(...choices);
		await run();
		assert.equal(calls.length, 0, "cancel does not open or export a document");
	}
	// A context-menu URI must win over another editor's active document.
	vscode.window.activeTextEditor = { document: { languageId: "plaintext" } };
	for (const [command, format] of [["exportPdf", "pdf"], ["exportPng", "png"]]) {
		calls.length = 0;
		await commands.get(`markviewExport.${command}`)(document.uri);
		assert.equal(calls.find((call) => call.open).text, "# Buffer");
		assert.deepEqual(calls.find((call) => call.options).options, { format, template: "configured-template" });
		assert.equal(answers.length, 0, "direct export does not ask for a template");
	}
	extension.deactivate();
	const NativeSession = require("../out/shared/sidecar.js").Session;
	const failed = new NativeSession(path.join(__dirname, "missing-engine"));
	await assert.rejects(failed.styles());
	assert.equal(failed.running, false, "a failed engine must be recreated on the next export");
	console.log("MARKVIEW-EXPORT picker sequence ok");
})().catch((error) => { console.error(error); process.exitCode = 1; });
