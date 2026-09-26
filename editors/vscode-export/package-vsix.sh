#!/usr/bin/env bash
# Package an existing Apple Silicon engine without requiring Rust.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
binary="$repo/target/release/markview"
if ! file "$binary" | grep -q 'Mach-O 64-bit executable arm64'; then
	echo "Expected an Apple Silicon macOS engine at $binary" >&2
	exit 1
fi
cd "$here"
trap 'rm -rf "$here/bin"' EXIT
rm -rf bin
mkdir -p bin/darwin-arm64 dist
cp "$binary" bin/darwin-arm64/markview
chmod +x bin/darwin-arm64/markview
npm_config_cache="$here/.npm-cache" npx --yes @vscode/vsce@4.0.0 package \
	--target darwin-arm64 --out dist/markview-export-darwin-arm64.vsix
