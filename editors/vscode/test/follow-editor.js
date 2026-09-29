// Drive real editor changes and delay an engine reply to check latest-tab wins.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, label) {
    for (let i = 0; i < 150; i++) { if (check()) return; await delay(100); }
    assert.fail(label);
}
exports.run = async extension => {
    const api = extension.exports;
    const folders = vscode.workspace.workspaceFolders;
    const first = path.join(folders[0].uri.fsPath, 'follow-first.md');
    const second = path.join(folders[1].uri.fsPath, 'follow-second.md');
    const plain = path.join(folders[0].uri.fsPath, 'follow.txt');
    fs.writeFileSync(first, '# First tab\n\nThe first document.\n');
    fs.writeFileSync(second, '# Second tab\n\nThe second document.\n');
    fs.writeFileSync(plain, 'A plain text editor.');
    const a = await vscode.workspace.openTextDocument(first);
    const b = await vscode.workspace.openTextDocument(second);
    const text = await vscode.workspace.openTextDocument(plain);
    const show = document => vscode.window.showTextDocument(document, {viewColumn:vscode.ViewColumn.One, preview:false});
    const previewTabs = () => vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(tab => tab.input?.viewType?.endsWith('markview.preview'));
    const painted = document => api.panelReport().documentUri === document.uri.toString() && api.panelReport().paintedVersion === document.version && api.panelReport().displayedTiles > 0;
    await vscode.commands.executeCommand('markview.closePreview');
    await show(a);
    await delay(300);
    assert.equal(previewTabs().length, 0, 'switching editors does not open a closed preview');
    await vscode.commands.executeCommand('markview.openPreview', a.uri);
    await until(() => painted(a), 'first document painted');
    const group = vscode.window.tabGroups.all.find(group => group.tabs.some(tab => previewTabs().includes(tab))).viewColumn;
    const firstSize = api.panelReport().settings.font_size;
    const bEditor = await show(b);
    assert.ok(await bEditor.edit(edit => edit.insert(new vscode.Position(0, 0), 'Unsaved second.\n\n')));
    await until(() => painted(b) && api.panelReport().text.includes('Unsaved second.'), 'switch follows dirty buffer without another preview command');
    assert.notEqual(api.panelReport().settings.font_size, firstSize, 'new document resolves its own folder settings');
    assert.equal(vscode.window.activeTextEditor.document.uri.toString(), b.uri.toString(), 'following does not steal focus');
    assert.equal(previewTabs().length, 1, 'one panel reused');
    assert.equal(previewTabs()[0].label, 'Preview follow-second.md');
    assert.equal(vscode.window.tabGroups.all.find(group => group.tabs.some(tab => previewTabs().includes(tab))).viewColumn, group);
    await show(text);
    await delay(300);
    assert.ok(painted(b), 'non-Markdown editor leaves the last preview visible');

    const { Session } = require(path.join(extension.extensionPath, 'out/shared/sidecar.js'));
    const original = Session.prototype.open;
    let release;
    let entered = false;
    let originalEngine;
    const gate = new Promise(resolve => { release = resolve; });
    Session.prototype.open = async function(id, content, options) {
        originalEngine = this;
        const state = await original.call(this, id, content, options);
        if (options?.path === first && !entered) { entered = true; await gate; }
        return state;
    };
    try {
        await show(a);
        await until(() => entered, 'first tab open is in flight');
        await show(b);
        await delay(100);
        release();
        await until(() => painted(b), 'latest tab wins after older response arrives');
        assert.ok(api.panelReport().text.includes('Unsaved second.'));
        assert.equal(api.panelReport().settings.font_size, vscode.workspace.getConfiguration('markview', b).get('fontSize'));
    } finally {
        release();
        Session.prototype.open = original;
    }
    originalEngine.dispose();
    assert.equal(originalEngine.running, false);
    await vscode.commands.executeCommand('markview.openPreview', b.uri);
    await until(() => painted(b), 'explicit reopen replaces a dead engine');
    await show(a);
    await until(() => painted(a), 'following uses the replacement engine');
    assert.equal(previewTabs().length, 1, 'engine replacement keeps the same panel');
    await vscode.commands.executeCommand('markview.closePreview');
    await show(a);
    await delay(300);
    assert.equal(previewTabs().length, 0, 'following stops when panel closes');
    console.log('MARKVIEW-FOLLOW ok dirty-buffer scoped-settings focus reuse non-md rapid-switch engine-replacement close');
};
