#!/usr/bin/env bash
# Verify the shipped bundle after unpacking, including its ad-hoc signature.
set -euo pipefail
outdir=$(realpath "${1:?usage: verify_macos_package.sh OUTDIR}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ditto -x -k "$outdir"/*.app.zip "$work"
app=$work/Markview.app
plutil -lint "$app/Contents/Info.plist"
# The desktop offers a bundle for a file only when it declares a type for it, so
# the registration is part of what ships rather than a detail of the template.
plutil -extract CFBundleDocumentTypes json -o - "$app/Contents/Info.plist" \
	| grep -q 'net.daringfireball.markdown'
codesign --verify --deep --strict "$app"
"$app/Contents/MacOS/markview" --help
test -s "$app/Contents/Resources/markview.icns"
test -s "$app/Contents/Resources/THIRD_PARTY.md"
