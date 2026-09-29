// Download a real VS Code host, then run the same suite used locally.
const { downloadAndUnzipVSCode } = require('@vscode/test-electron');
const { spawnSync } = require('node:child_process');
(async () => {
  const editor = await downloadAndUnzipVSCode(process.env.VSCODE_VERSION || 'stable');
  const result = spawnSync('bash', ['run-tests.sh'], {
    cwd: require('node:path').resolve(__dirname, '..'), stdio: 'inherit',
    env: { ...process.env, VSCODE_EXEC_PATH: editor },
  });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
