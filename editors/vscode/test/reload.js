// Runs twice in the same isolated window, on either side of a real Reload Window.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, label) {
    for (let i = 0; i < 200; i++) { if (check()) return; await delay(100); }
    assert.fail(label);
}
exports.run = async () => {
    const folder = vscode.workspace.workspaceFolders[0].uri.fsPath;
    const phase = path.join(folder, 'reload-phase.json');
    const previewColumn = () => vscode.window.tabGroups.all.find(group => group.tabs.some(tab => tab.input?.viewType?.endsWith('markview.preview')))?.viewColumn;
    const extension = vscode.extensions.getExtension('Stevvven.markview-export');
    assert.ok(extension);
    if (!fs.existsSync(phase)) {
        await vscode.workspace.getConfiguration('markview').update('enginePath', process.env.MARKVIEW_PACKAGED_EXTENSION ? '' : process.env.MARKVIEW_TEST_BINARY, vscode.ConfigurationTarget.Global);
        const file = path.join(folder, 'reload.md');
        fs.writeFileSync(file, '# Reload preview\n\n' + 'A paragraph that keeps the page tall enough to scroll.\n\n'.repeat(200));
        const template = path.join(folder, 'reload.mvss.toml');
        fs.writeFileSync(template, 'format_version = 2\nversion = 1\ntargets = ["pdf"]\n[[rule]]\nwhen = ["body"]\nbackground = "#123456"\n');
        const document = await vscode.workspace.openTextDocument(file);
        const editor = await vscode.window.showTextDocument(document, vscode.ViewColumn.One);
        await vscode.workspace.getConfiguration('markview', document).update('template', template, vscode.ConfigurationTarget.WorkspaceFolder, true);
        const api = await extension.activate();
        assert.ok(await editor.edit(edit => edit.insert(new vscode.Position(0, 0), 'Unsaved restored text.\n\n')));
        assert.ok(document.isDirty && document.getText().startsWith('Unsaved restored text.'));
        await vscode.commands.executeCommand('markview.openPreview', document.uri);
        await until(() => api.panelReport().paintedVersion === document.version && api.panelReport().background === 'rgb(18, 52, 86)', 'custom background reaches the visible preview');
        await api.scrollPreviewTo(600);
        await until(() => Math.abs(api.panelReport().scroll - 600) < 5, 'preview scrolls before reload');
        await delay(500);
        fs.writeFileSync(phase, JSON.stringify({uri:document.uri.toString(), scroll:api.panelReport().scroll, column:previewColumn()}));
        console.log('MARKVIEW-RELOAD before real reload');
        // Reload cancels outstanding calls in the outgoing extension host.
        try { await vscode.commands.executeCommand('workbench.action.reloadWindow'); }
        catch (error) { if (error.name !== 'Canceled') throw error; }
        await new Promise(() => {});
    }
    const saved = JSON.parse(fs.readFileSync(phase));
    await until(() => extension.isActive, 'saved webview activates the extension without a preview command');
    const api = extension.exports;
    try {
        await until(() => api.panelReport().restored && api.panelReport().paintedVersion >= 0 && !api.panelReport().refreshing, 'restored panel receives decoded pixels');
    } catch (error) {
        throw new Error(`${error}; report=${JSON.stringify(api.panelReport())}; tabs=${JSON.stringify(vscode.window.tabGroups.all.map(group => group.tabs.map(tab => ({label:tab.label, type:tab.input?.viewType}))))}`);
    }
    assert.equal(previewColumn(), saved.column, 'restore keeps the original editor group');
    const document = await vscode.workspace.openTextDocument(vscode.Uri.parse(saved.uri));
    await until(() => document.getText().startsWith('Unsaved restored text.'), 'restore uses the editor buffer, including unsaved changes');
    assert.equal(api.panelReport().background, 'rgb(18, 52, 86)');
    assert.ok(Math.abs(api.panelReport().scroll - saved.scroll) < 5, 'restore retains reading position');
    const before = api.panelReport().paintedVersion;
    const edit = new vscode.WorkspaceEdit();
    edit.insert(document.uri, new vscode.Position(0, 0), 'After reload.\n\n');
    await vscode.workspace.applyEdit(edit);
    await until(() => api.panelReport().paintedVersion > before, 'restored panel continues following edits');
    console.log('MARKVIEW-RELOAD ok actual reload, unsaved buffer, scroll, background and subsequent edit');
    console.log('MARKVIEW-EXT ok true');
};
