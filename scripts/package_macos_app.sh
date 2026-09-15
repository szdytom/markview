#!/usr/bin/env bash
# Wraps the released macOS binary in a `.app` bundle and zips it.
#
# Usage: scripts/package_macos_app.sh EXTRACTED_DIR VERSION OUTDIR
#
# `EXTRACTED_DIR` is the directory holding the binary from the macOS release
# archive. The bundle is ad-hoc signed only, so Gatekeeper requires an explicit
# user override on first launch; see `docs/packaging.md`.
set -euo pipefail

extracted=${1:?usage: package_macos_app.sh EXTRACTED_DIR VERSION OUTDIR}
version=${2:?usage: package_macos_app.sh EXTRACTED_DIR VERSION OUTDIR}
outdir=${3:?usage: package_macos_app.sh EXTRACTED_DIR VERSION OUTDIR}

root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$outdir"
outdir=$(realpath "$outdir")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app=$work/Markview.app

install -Dm755 "$extracted/markview" "$app/Contents/MacOS/markview"
install -Dm644 "$root/assets/icons/markview.icns" \
	"$app/Contents/Resources/markview.icns"
sed "s/@VERSION@/$version/g" "$root/packaging/info.plist.in" \
	>"$app/Contents/Info.plist"
install -Dm644 "$root/LICENSE" "$app/Contents/Resources/LICENSE"
install -Dm644 "$root/THIRD_PARTY.md" \
	"$app/Contents/Resources/THIRD_PARTY.md"
install -Dm644 "$root/licenses/KaTeX-OFL.txt" \
	"$app/Contents/Resources/KaTeX-OFL.txt"

# Ad-hoc signing keeps the bundle internally consistent for local execution.
codesign --force --deep --sign - "$app"

# `--norsrc` drops .DS_Store and `-d` drops resource forks; both confuse a
# browser-unzipped bundle on a machine that did not create the archive.
ditto -c -k --norsrc --keepParent "$app" \
	"$outdir/markview-${version}-aarch64.app.zip"
