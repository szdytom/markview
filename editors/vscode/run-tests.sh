#!/usr/bin/env bash
# Run the extension's tests inside a real VS Code.
#
# The tests run in an extension host, so `vscode` is the editor's own API and a
# webview is a real webview. The editor is the one installed on this machine
# rather than a downloaded copy: `--extensionDevelopmentPath` loads this
# directory and `--extensionTestsPath` runs `test/run.js`, whose exported
# `run` is called in the host. The extension host's exit code is the result,
# and a test that throws fails it.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
editor="${VSCODE_EXEC_PATH:-/Applications/Visual Studio Code.app/Contents/MacOS/Code}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/mv-vscode.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
engine="$(cd "$here/../.." && pwd)/target/release/markview"

# The host loads `out/`, so the sources are compiled here rather than left to
# whoever ran the tests last.
(cd "$here" && ./node_modules/.bin/tsc -p tsconfig.json)
node --test "$here/../shared/test/frames.cjs"
node "$here/test/pickers.js"

# The tests need folders to resolve settings against: the workspace-folder and
# language-scoped layers of EXT-8 only exist inside one, and two of them show
# that the resolution is per document rather than per window.
mkdir -p "$scratch/ws/.vscode" "$scratch/ws2/.vscode" "$scratch/user/User"
printf '%s\n' '{"update.mode":"none","extensions.autoUpdate":false}' > "$scratch/user/User/settings.json"
cat >"$scratch/ws/.vscode/settings.json" <<'JSON'
{
	"markview.fontSize": 30,
	"editor.tabSize": 3,
	"[markdown]": { "markview.columnWidth": 500, "editor.tabSize": 7 }
}
JSON
cat >"$scratch/ws2/.vscode/settings.json" <<'JSON'
{
	"markview.fontSize": 12
}
JSON
cat >"$scratch/ws.code-workspace" <<JSON
{
	"folders": [
		{ "path": "ws" },
		{ "path": "ws2" }
	]
}
JSON
development="$here"
if [ -n "${MARKVIEW_VSIX:-}" ]; then
    cli="${VSCODE_CLI_PATH:-$(dirname "$editor")/../Resources/app/bin/code}"
    if [[ "$OSTYPE" == linux* ]]; then cli="${VSCODE_CLI_PATH:-$(dirname "$editor")/bin/code}"; fi
    "$cli" --user-data-dir "$scratch/user" --extensions-dir "$scratch/ext" --install-extension "$MARKVIEW_VSIX" --force
    development="$(node -e 'const fs=require("node:fs"),p=process.argv[1]; const name=fs.readdirSync(p).find(n=>n.startsWith("stevvven.markview-export-")); if(!name) process.exit(1); console.log(require("node:path").join(p,name));' "$scratch/ext")"
    platform="$(node -p 'process.platform+"-"+process.arch')"
    engine="$development/bin/$platform/markview"
    export MARKVIEW_PACKAGED_EXTENSION=1
fi
export MARKVIEW_TEST_BINARY="$engine"
# Linux desktop handoffs are recorded without launching a persistent browser.
if [[ "$OSTYPE" == linux* ]]; then
    mkdir -p "$scratch/bin"
    export MARKVIEW_EXTERNAL_LOG="$scratch/external.log"
    cat >"$scratch/bin/xdg-open" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$@" >> "$MARKVIEW_EXTERNAL_LOG"
SH
    chmod +x "$scratch/bin/xdg-open"
    export PATH="$scratch/bin:$PATH"
fi
set +e
python3 "$here/test/run-host.py" --log "$scratch/run.log" -- "$editor" "$scratch/ws.code-workspace" \
	--extensionDevelopmentPath="$development" \
	--extensionTestsPath="$here/test/run.js" \
	--user-data-dir="$scratch/user" \
	--extensions-dir="$scratch/ext" \
	--disable-gpu --skip-welcome --skip-release-notes --no-sandbox \
	--disable-workspace-trust
status=$?
set -e
if ! grep -q 'MARKVIEW-EXT ok true' "$scratch/run.log"; then status=1; fi

# The window's engine ends with the window. The host disposes it on the way
# out, and the engine would end anyway when its pipe closes, so nothing may be
# left holding the user's desktop.
left=""
for attempt in {1..30}; do
    left="$(pgrep -f "$engine serve" || true)"
    [ -z "$left" ] && break
    sleep 0.2
done
if [ -n "$left" ]; then
    echo "FAIL an engine outlived the window: $left" >&2
    exit 1
fi
echo "no engine outlived the window"
exit "$status"
