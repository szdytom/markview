#!/usr/bin/env bash
# Install and exercise the VSIX using disposable editor storage.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
node "$here/test/pickers.js"
editor="${VSCODE_EXEC_PATH:-/Applications/Visual Studio Code.app/Contents/MacOS/Code}"
cli="$(dirname "$editor")/../Resources/app/bin/code"
scratch="$(mktemp -d "$here/.vscode-test.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/ws/.vscode"
cat > "$scratch/ws/.vscode/settings.json" <<'JSON'
{"markviewExport.template":"no-such-template","[markdown]":{"markviewExport.template":"house.mvss.toml"}}
JSON
"$cli" --user-data-dir "$scratch/user" --extensions-dir "$scratch/ext" \
	--install-extension "$here/dist/markview-export-darwin-arm64.vsix" --force
extension_id="$(node -p "const p = require('$here/package.json'); (p.publisher + '.' + p.name).toLowerCase()")"
installed="$(find "$scratch/ext" -maxdepth 1 -type d -name "$extension_id-*" -print)"
test -n "$installed"
"$editor" "$scratch/ws" --extensionDevelopmentPath="$installed" \
	--extensionTestsPath="$here/test/run.js" \
	--user-data-dir="$scratch/user" --extensions-dir="$scratch/ext" \
	--disable-gpu --skip-welcome --skip-release-notes --no-sandbox --disable-workspace-trust
for attempt in {1..30}; do
	if ! pgrep -f "$installed/bin/darwin-arm64/markview" >/dev/null; then
		mkdir -p "$here/dist/samples"
		cp "$scratch/ws/"*.pdf "$scratch/ws/"*.png "$here/dist/samples/"
		echo "no engine outlived the window"
		exit 0
	fi
	sleep 0.2
done
echo "FAIL an engine outlived the window" >&2
exit 1
