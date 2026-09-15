#!/usr/bin/env bash
# Exercise the executables and resources inside the finished packages.
set -euo pipefail
outdir=$(realpath "${1:?usage: verify_linux_packages.sh OUTDIR}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

dpkg-deb --extract "$outdir"/*.deb "$work/deb"
"$work/deb/usr/bin/markview" --help
required=$(readelf --version-info "$work/deb/usr/bin/markview" |
	sed -n 's/.*Name: GLIBC_\([0-9.]*\).*/\1/p' | sort -V | tail -1)
test "$(printf '%s\n' "$required" 2.35 | sort -V | tail -1)" = 2.35
test -s "$work/deb/usr/share/applications/markview.desktop"
test -s "$work/deb/usr/share/icons/hicolor/512x512/apps/markview.png"
test -s "$work/deb/usr/share/doc/markview/THIRD_PARTY.md"

cd "$work"
images=("$outdir"/*.AppImage)
"${images[0]}" --appimage-extract >/dev/null
./squashfs-root/AppRun --help
test -s squashfs-root/markview.png
test -s squashfs-root/usr/share/doc/markview/THIRD_PARTY.md
