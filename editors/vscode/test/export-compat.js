// Exercise both command namespaces against the bundled engine with a preview open.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vscode = require('vscode');
exports.run = async (extension, scratch) => {
  const source = path.join(scratch, 'compat.md');
  fs.writeFileSync(source, '# Original\n');
  const document = await vscode.workspace.openTextDocument(source);
  const edit = new vscode.WorkspaceEdit();
  edit.insert(document.uri, new vscode.Position(1, 0), '\nUnsaved export text.\n');
  assert.ok(await vscode.workspace.applyEdit(edit));
  const preview = extension.exports.panelReport().documentUri;
  for (const [command, format, magic] of [['exportPdf', 'pdf', '25504446'], ['exportPng', 'png', '89504e47']]) {
    const outputs = [];
    for (const prefix of ['markview', 'markviewExport']) {
      const target = vscode.Uri.file(path.join(scratch, `${prefix}.${format}`));
      await vscode.commands.executeCommand(`${prefix}.${command}`, { uri: document.uri, target, template: null });
      const bytes = fs.readFileSync(target.fsPath);
      assert.equal(bytes.subarray(0, 4).toString('hex'), magic);
      outputs.push(bytes);
    }
    assert.deepEqual(outputs[0], outputs[1], 'legacy and canonical commands produce the same artifact');
  }
  assert.equal(extension.exports.panelReport().documentUri, preview, 'export does not replace the preview');
  assert.equal(fs.readFileSync(source, 'utf8'), '# Original\n', 'export does not save the editor buffer');
  await assert.rejects(extension.exports.exportDocument({ uri: document.uri, target: document.uri }), /different from/);
  const untitled = await vscode.workspace.openTextDocument({ language: 'markdown', content: '# Untitled\n' });
  const target = vscode.Uri.file(path.join(scratch, 'untitled.pdf'));
  await extension.exports.exportDocument({ uri: untitled.uri, target, template: null });
  assert.equal(fs.readFileSync(target.fsPath).subarray(0, 4).toString(), '%PDF');
  console.log('MARKVIEW-COMPAT ok legacy canonical dirty-buffer preview-preserved source-protection untitled');
};
