// Exercises template settings through the real host, engine and export path.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vscode = require("vscode");
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(check, label) {
	for (let i = 0; i < 100; i++) { if (check()) return; await delay(100); }
	assert.fail(label);
}
exports.run = async (extension) => {
	const folder = vscode.workspace.workspaceFolders[0].uri.fsPath;
	const settingsPath = path.join(folder, ".vscode/settings.json");
	const saved = fs.readFileSync(settingsPath, "utf8");
	const settings = JSON.parse(saved);
	const file = path.join(folder, "template-preview.md");
	fs.writeFileSync(file, "# Template preview\n\nBody text with a shared template.\n");
	const document = await vscode.workspace.openTextDocument(file);
	const api = extension.exports;
	const { configuredTemplate, templateFor } = require(path.join(extension.extensionPath, "out/vscode/src/template.js"));
	const { Session } = require(path.join(extension.extensionPath, "out/shared/sidecar.js"));
	const original = Session.prototype.open;
	let sent;
	let templateId;
	let previewSession;
	Session.prototype.open = function(id, text, options) {
		if (options?.path === file && !id.startsWith("export-")) { sent = options; templateId = id; previewSession = this; }
		return original.call(this, id, text, options);
	};
	const workbench = vscode.workspace.getConfiguration("workbench");
	const theme = workbench.get("colorTheme");
	try {
		await workbench.update("colorTheme", "Default Light Modern", vscode.ConfigurationTarget.Global);
		await delay(1000);
		settings["markviewExport.template"] = "mondrian";
		fs.writeFileSync(settingsPath, JSON.stringify(settings));
		await until(() => configuredTemplate(document) === "mondrian", "legacy export template resolves without the export extension installed");
		await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
		await vscode.commands.executeCommand("markview.openPreview", document.uri);
		await until(() => api.panelReport().paintedVersion === document.version && sent?.template === "mondrian", "configured template reaches preview pixels");
		await delay(500);
		const mondrian = api.panelReport().painted;
		// Theme changes may resize the host viewport (for example its scrollbar).
		// Compare a fixed crop from the actual preview session instead.
		const crop = {width:800, height:400, scale:1, scroll:0};
		const beforeTheme = await previewSession.tile(templateId, crop);
		await workbench.update("colorTheme", "Default Dark Modern", vscode.ConfigurationTarget.Global);
		await delay(1500);
		assert.equal(api.panelReport().appearance.theme, "dark");
		const afterTheme = await previewSession.tile(templateId, crop);
		assert.deepEqual(afterTheme.background, beforeTheme.background);
        assert.equal(api.panelReport().background, `rgb(${afterTheme.background.join(", ")})`);
		assert.deepEqual(afterTheme.png, beforeTheme.png, "explicit template retains its pixels across a host theme change");
		settings["[markdown]"]["markview.template"] = "vangogh";
		fs.writeFileSync(settingsPath, JSON.stringify(settings));
		await until(() => sent?.template === "vangogh" && api.panelReport().painted !== mondrian, "language-scoped template change redraws the preview");
		const templateFile = path.join(folder, "custom.mvss.toml");
		const rules = 'format_version = 2\nversion = 1\ntargets = ["pdf"]\n[[rule]]\nwhen = ["h1"]\ncolor = "#AA1177"\n[[rule]]\nwhen = ["body"]\nbackground = "#123456"\n';
		fs.writeFileSync(templateFile, rules);
		settings["[markdown]"]["markview.template"] = "custom.mvss.toml";
		fs.writeFileSync(settingsPath, JSON.stringify(settings));
		await until(() => sent?.stylesheet === rules && api.panelReport().background === "rgb(18, 52, 86)", "custom template is read relative to Markdown");
		assert.deepEqual(await templateFor({}, document), { stylesheet: rules });
		const output = path.join(folder, "template-preview.pdf");
		await api.exportDocument({ uri: document.uri, target: vscode.Uri.file(output), format: "pdf" });
		assert.equal(fs.readFileSync(output).subarray(0, 4).toString(), "%PDF");
		settings["[markdown]"]["markview.template"] = "";
		fs.writeFileSync(settingsPath, JSON.stringify(settings));
		await until(() => configuredTemplate(document) === "" && sent?.template === undefined && sent?.stylesheet === undefined, "explicit empty disables legacy fallback and clears preview template");
		assert.deepEqual(await templateFor({}, document), {});
		assert.deepEqual(await templateFor({template:null}, document), {});
		const invalid = path.join(folder, "invalid.mvss.toml");
		fs.writeFileSync(invalid, "not valid MVSS");
		for (const [index, bad] of ["missing.mvss.toml", "no-such-template", "invalid.mvss.toml"].entries()) {
			settings["[markdown]"]["markview.template"] = bad;
			fs.writeFileSync(settingsPath, JSON.stringify(settings));
			await until(() => configuredTemplate(document) === bad, "invalid default is loaded for the regression");
			for (const template of [null, "mondrian"]) {
				const target = vscode.Uri.file(path.join(folder, `explicit-${index}-${template ?? "none"}.pdf`));
				await api.exportDocument({uri:document.uri, target, format:"pdf", template});
				assert.equal(fs.readFileSync(target.fsPath).subarray(0,4).toString(), "%PDF", "explicit export is independent of an invalid preview template");
			}
		}
		const manifest = extension.packageJSON;
		assert.equal(manifest.contributes.menus["editor/title"].find(item => item.command === "markview.openPreview").group, "navigation@0");
		for (const icon of Object.values(manifest.contributes.commands.find(item => item.command === "markview.openPreview").icon)) {
			assert.ok(fs.existsSync(path.join(extension.extensionPath, icon)), "toolbar icon is packaged");
		}
		console.log("MARKVIEW-TEMPLATES ok legacy scoped custom theme explicit-export toolbar");
	} finally {
		Session.prototype.open = original;
		fs.writeFileSync(settingsPath, saved);
		await workbench.update("colorTheme", theme, vscode.ConfigurationTarget.Global);
		await delay(500);
	}
};
