#!/usr/bin/env bash
# Builds a Debian package around an already compiled `markview` binary.
#
# Usage: scripts/package_deb.sh BINARY VERSION ARCH OUTDIR
#
# `BINARY` is the stripped release executable, `ARCH` is a Debian architecture
# such as `amd64`, and `OUTDIR` receives the `.deb`. The package carries the
# desktop entry, the hicolor icon, and the license notices.
set -euo pipefail

binary=${1:?usage: package_deb.sh BINARY VERSION ARCH OUTDIR}
version=${2:?usage: package_deb.sh BINARY VERSION ARCH OUTDIR}
arch=${3:?usage: package_deb.sh BINARY VERSION ARCH OUTDIR}
outdir=${4:?usage: package_deb.sh BINARY VERSION ARCH OUTDIR}

root=$(cd "$(dirname "$0")/.." && pwd)
binary=$(realpath "$binary")
notices=${MARKVIEW_NOTICES:-}
mkdir -p "$outdir"
outdir=$(realpath "$outdir")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pkg=$work/markview

install -Dm755 "$binary" "$pkg/usr/bin/markview"
install -Dm644 "$root/packaging/markview.desktop" \
	"$pkg/usr/share/applications/markview.desktop"
install -Dm644 "$root/assets/icons/markview-512.png" \
	"$pkg/usr/share/icons/hicolor/512x512/apps/markview.png"

docs=$pkg/usr/share/doc/markview
install -Dm644 "$root/LICENSE" "$docs/copyright"
install -Dm644 "$root/THIRD_PARTY.md" "$docs/THIRD_PARTY.md"
install -Dm644 "$root/licenses/KaTeX-OFL.txt" "$docs/KaTeX-OFL.txt"
if [[ -n "$notices" ]]; then
	install -Dm644 "$notices" "$docs/third-party-notices.html"
fi

# `libvulkan1` is the loader wgpu needs; the X11 and Wayland libraries come
# from winit's runtime dlopen. `rfd` uses the desktop portal for file dialogs,
# so no GTK toolkit is required.
mkdir -p "$pkg/DEBIAN"
cat >"$pkg/DEBIAN/control" <<EOF
Package: markview
Version: $version
Architecture: $arch
Maintainer: szdytom <szdytom@users.noreply.github.com>
Installed-Size: $(du -sk "$pkg" | cut -f1)
Homepage: https://github.com/szdytom/markview
Section: editors
Priority: optional
Depends: libc6 (>= 2.35), libfontconfig1, libvulkan1, libx11-6, libxcursor1, libxi6, libxkbcommon0, libwayland-client0, xdg-desktop-portal
Recommends: fonts-noto-cjk
Description: Native, read-only Markdown reader
 Markview renders Markdown, math, code, tables, links, and images in a
 native window without a browser, WebView, JavaScript, or an external TeX
 process. It is read-only and watches the open document for changes.
EOF

dpkg-deb --build --root-owner-group "$pkg" \
	"$outdir/markview_${version}_${arch}.deb"
