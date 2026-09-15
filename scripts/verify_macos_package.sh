#!/usr/bin/env bash
# Verify the shipped bundle after unpacking, including its ad-hoc signature.
set -euo pipefail
outdir=$(realpath "${1:?usage: verify_macos_package.sh OUTDIR}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ditto -x -k "$outdir"/*.app.zip "$work"
app=$work/Markview.app
plutil -lint "$app/Contents/Info.plist"
codesign --verify --deep --strict "$app"
"$app/Contents/MacOS/markview" --help
test -s "$app/Contents/Resources/markview.icns"
test -s "$app/Contents/Resources/THIRD_PARTY.md"
