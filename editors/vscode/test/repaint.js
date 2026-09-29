// Hold native replies to observe the real webview between two painted versions.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, label) {
    for (let i = 0; i < 100; i++) { if (check()) return; await delay(100); }
    assert.fail(label);
}
exports.run = async (extension, scratch) => {
    const api = extension.exports;
    const file = path.join(scratch, 'repaint.md');
    fs.writeFileSync(file, '# Repaint\n\n' + 'A paragraph that remains visible while the next version renders.\n\n'.repeat(150));
    const document = await vscode.workspace.openTextDocument(file);
    const editor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
    await vscode.commands.executeCommand('markview.openPreview', document.uri);
    await until(() => api.panelReport().paintedVersion === document.version, 'initial paint');
    await api.scrollPreviewTo(120);
    await delay(700);
    const before = api.panelReport();
    assert.ok(before.displayedTiles >= 2, 'viewport straddles two bands');
    const version = document.version;
    const { Session } = require(path.join(extension.extensionPath, 'out/shared/sidecar.js'));
    const original = Session.prototype.tile;
    const releases = [];
    Session.prototype.tile = async function (...args) {
        const tile = await original.apply(this, args);
        await new Promise(resolve => releases.push(resolve));
        return tile;
    };
    try {
        assert.ok(await editor.edit(edit => edit.insert(new vscode.Position(0, 9), ' updated')));
        await until(() => releases.length >= 2 && api.panelReport().refreshing, 'replacement viewport waits for two bands');
        assert.equal(api.panelReport().displayedTiles, before.displayedTiles, 'old pixels remain while rendering');
        assert.ok(api.panelReport().pixelVersions.every(v => v === version));
        releases.shift()();
        await delay(300);
        assert.ok(api.panelReport().refreshing, 'one decoded band cannot publish a partial generation');
        assert.equal(api.panelReport().displayedTiles, before.displayedTiles);
        assert.ok(api.panelReport().pixelVersions.every(v => v === version), 'no mixed old/new pixels');
    } finally {
        Session.prototype.tile = original;
        for (const release of releases) release();
    }
    await until(() => api.panelReport().paintedVersion === document.version && !api.panelReport().refreshing, 'complete replacement published');
    assert.ok(api.panelReport().pixelVersions.length >= 2);
    assert.ok(api.panelReport().pixelVersions.every(v => v === document.version), 'all displayed bands use the new version');
    console.log('MARKVIEW-REPAINT ok retained pixels and atomic visible replacement');
};
