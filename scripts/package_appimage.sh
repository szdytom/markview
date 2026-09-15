#!/usr/bin/env bash
# Builds a portable AppImage around an already compiled `markview` binary.
#
# Usage: scripts/package_appimage.sh BINARY VERSION OUTDIR
#
# Only the application is bundled. The Vulkan loader and driver, system fonts,
# the X11/Wayland client libraries, and the desktop portal all come from the
# host, so no glibc and no GTK stack is embedded. This keeps the image small
# and avoids the classic AppImage glibc mismatch.
set -euo pipefail

binary=${1:?usage: package_appimage.sh BINARY VERSION OUTDIR}
version=${2:?usage: package_appimage.sh BINARY VERSION OUTDIR}
outdir=${3:?usage: package_appimage.sh BINARY VERSION OUTDIR}

root=$(cd "$(dirname "$0")/.." && pwd)
binary=$(realpath "$binary")
mkdir -p "$outdir"
outdir=$(realpath "$outdir")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
appdir=$work/Markview.AppDir

install -Dm755 "$binary" "$appdir/usr/bin/markview"
install -Dm644 "$root/packaging/markview.desktop" \
	"$appdir/markview.desktop"
install -Dm644 "$root/assets/icons/markview-256.png" \
	"$appdir/markview.png"
# The desktop entry must also sit in the standard location for appimagetool.
install -Dm644 "$root/packaging/markview.desktop" \
	"$appdir/usr/share/applications/markview.desktop"

cat >"$appdir/AppRun" <<'EOF'
#!/bin/sh
# Resolves the directory of this AppRun, following symlinks.
here=$(dirname "$(readlink -f "$0")")
export PATH="$here/usr/bin:$PATH"
exec "$here/usr/bin/markview" "$@"
EOF
chmod 755 "$appdir/AppRun"

tool=${APPIMAGETOOL:-$work/appimagetool}
if [[ -z "${APPIMAGETOOL:-}" ]]; then
	url=https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
	curl --fail --silent --show-error --location \
		--output "$tool" "$url"
	chmod 755 "$tool"
fi

# `--appimage-extract-and-run` avoids the libfuse2 dependency of the tool.
ARCH=x86_64 VERSION="$version" \
	"$tool" --appimage-extract-and-run "$appdir" \
	"$outdir/markview-${version}-x86_64.AppImage"
