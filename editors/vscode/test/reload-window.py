#!/usr/bin/env python3
"""Exercise actual window reload outside VS Code's auto-exiting test host.

Requires `vsce` on PATH and the packaged darwin-arm64 preview in dist/.
Both extensions and editor storage live in a disposable directory.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

here = Path(__file__).resolve().parent.parent
editor = '/Applications/Visual Studio Code.app/Contents/MacOS/Code'
cli = '/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code'
with tempfile.TemporaryDirectory(prefix='mv-reload-') as scratch:
    root = Path(scratch)
    workspace = root / 'workspace'
    workspace.mkdir()
    driver = root / 'driver'
    driver.mkdir()
    (driver / 'package.json').write_text(json.dumps({
        'name': 'markview-reload-test', 'publisher': 'test', 'version': '1.0.0',
        'engines': {'vscode': '^1.95.0'}, 'main': 'index.js',
        'description': 'Isolated Markview reload integration driver', 'license': 'UNLICENSED',
        'activationEvents': ['onStartupFinished'],
    }))
    shutil.copyfile(here / 'test/reload.js', driver / 'reload.js')
    (driver / 'index.js').write_text('''
const fs = require('node:fs');
const vscode = require('vscode');
exports.activate = () => {
    require('./reload.js').run().then(() => finish({ok:true}), error => finish({error:String(error.stack || error)}));
};
function finish(result) {
    fs.writeFileSync(process.env.MARKVIEW_RELOAD_RESULT, JSON.stringify(result));
    vscode.commands.executeCommand('workbench.action.quit');
}
''')
    user = root / 'user'
    (user / 'User').mkdir(parents=True)
    (user / 'User/settings.json').write_text(json.dumps({
        'update.mode': 'none', 'extensions.autoUpdate': False,
        'window.restoreWindows': 'all', 'files.hotExit': 'onExitAndWindowClose',
    }))
    common = ['--user-data-dir', str(user), '--extensions-dir', str(root/'extensions')]
    subprocess.run([cli, *common, '--install-extension', str(here/'dist/markview-export-darwin-arm64.vsix'), '--force'], check=True)
    subprocess.run(['vsce', 'package', '--no-dependencies', '--allow-missing-repository', '--skip-license', '--out', str(root/'driver.vsix')], cwd=driver, check=True)
    subprocess.run([cli, *common, '--install-extension', str(root/'driver.vsix'), '--force'], check=True)
    result = root / 'result.json'
    env = dict(os.environ, MARKVIEW_PACKAGED_EXTENSION='1', MARKVIEW_RELOAD_RESULT=str(result))
    process = subprocess.Popen([editor, str(workspace), *common,
        '--skip-welcome', '--skip-release-notes', '--disable-workspace-trust', '--no-sandbox'], env=env)
    try:
        deadline = time.monotonic() + 100
        while not result.exists() and time.monotonic() < deadline:
            time.sleep(0.2)
        assert result.exists(), 'Reload driver timed out'
        outcome = json.loads(result.read_text())
        if not outcome.get('ok'):
            print('Backup files:', [str(p.relative_to(root)) for p in root.glob('user/Backups/**/*') if p.is_file()], flush=True)
        assert outcome.get('ok'), outcome
        process.wait(timeout=15)
        print('MARKVIEW-RELOAD ok actual window reload; restored buffer, scroll, background and live edits', flush=True)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        # Only this isolated installation's engine is relevant.
        for _ in range(30):
            engines = subprocess.run(['pgrep', '-f', str(root/'extensions') + '/.*markview serve'], capture_output=True)
            if engines.returncode != 0:
                break
            time.sleep(0.2)
        assert engines.returncode != 0, 'an engine outlived the reload window'
        print('no engine outlived the window', flush=True)
