// Runs in the real VS Code extension host against an installed VSIX.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
const delay = ms => new Promise(r => setTimeout(r, ms));
async function until(check, label) {
  for (let i=0; i<200; i++) {if(check()) return; await delay(100);}
  assert.fail(label);
}
exports.run = async () => {
  const extension = vscode.extensions.getExtension('Stevvven.markview-export');
  assert.ok(extension, 'the published identity is installed');
  assert.equal(extension.packageJSON.displayName, 'Markview Integration');
  const api = await extension.activate();
  const commands = await vscode.commands.getCommands(true);
  for(const command of ['markviewExport.exportPdf','markviewExport.exportPng',
    'markviewExport.exportWithTemplate','markview.openPreview']) assert.ok(commands.includes(command));
  const folder=vscode.workspace.workspaceFolders[0].uri.fsPath;
  const file=path.join(folder,'demo.md');
  fs.writeFileSync(file,'# Upgrade demo\n\nA paragraph with **bold** and $x^2$.\n\n'.repeat(30));
  const doc=await vscode.workspace.openTextDocument(file);
  const editor=await vscode.window.showTextDocument(doc);
  const config=vscode.workspace.getConfiguration('markview', doc);
  const oldConfig=vscode.workspace.getConfiguration('markviewExport', doc);
  await oldConfig.update('template','qibaishi',vscode.ConfigurationTarget.WorkspaceFolder,true);
  assert.equal((await api.configuredTemplate('pdf',doc.uri)).id,'qibaishi','[markdown] legacy export settings apply');
  await config.update('template','vangogh',vscode.ConfigurationTarget.WorkspaceFolder,true);
  assert.equal((await api.configuredTemplate('pdf',doc.uri)).id,'vangogh','[markdown] shared legacy setting wins');
  await config.update('template','',vscode.ConfigurationTarget.WorkspaceFolder,true);
  assert.equal((await api.configuredTemplate('pdf',doc.uri)).id,'default','explicit empty legacy setting suppresses fallback');
  await config.update('template',undefined,vscode.ConfigurationTarget.WorkspaceFolder,true);
  await oldConfig.update('template',undefined,vscode.ConfigurationTarget.WorkspaceFolder,true);

  // The preinstalled 0.1.2 fixture leaves this setting behind during upgrade.
  assert.equal(vscode.workspace.getConfiguration('markviewExport',doc.uri).get('template'),'mondrian');
  await vscode.commands.executeCommand('markview.openPreview',doc.uri);
  await until(()=>api.previewStatus(doc.uri.toString())?.version===doc.version,'WASM preview reaches the current document');
  assert.ok(api.previewStatus(doc.uri.toString()).blocks>0,'the engine laid out blocks');
  await editor.edit(edit=>edit.insert(new vscode.Position(0,0),'New unsaved text\n\n'));
  await until(()=>api.previewStatus(doc.uri.toString())?.version===doc.version,'unsaved changes reach the preview');
  for(const format of ['pdf','png']) {
    const output=path.join(folder,'legacy.'+format);
    await api.exportBuffer(doc.getText(),folder,format,output,doc.uri);
    const bytes=fs.readFileSync(output);
    assert.ok(format==='pdf' ? bytes.subarray(0,5).toString()==='%PDF-'
      : bytes.subarray(0,8).equals(Buffer.from([137,80,78,71,13,10,26,10])));
    assert.ok(doc.isDirty, 'export does not save the document');
  }
  await config.update('exportTemplate','default',vscode.ConfigurationTarget.WorkspaceFolder);
  assert.equal((await api.configuredTemplate('pdf',doc.uri)).id,'default','explicit new settings override legacy configuration');
  assert.equal(vscode.workspace.getConfiguration('markviewExport',doc.uri).get('template'),'mondrian','upgrade retains old settings');
  await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  console.log('MARKVIEW-INTEGRATION installed upgrade preview dirty-buffer PDF PNG ok');
};
