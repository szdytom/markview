#!/usr/bin/env bash
# Package one native engine per target; never substitute another architecture.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
platforms=(win32-x64 win32-arm64 linux-x64 linux-arm64 darwin-x64 darwin-arm64)
targets=(x86_64-pc-windows-msvc aarch64-pc-windows-msvc x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin)
check=0
if [ "${1:-}" = --check ]; then check=1; shift; fi
wanted=("$@")
if [ "${#wanted[@]}" -eq 0 ]; then wanted=("${platforms[@]}"); fi
for platform in "${wanted[@]}"; do
    case "$platform" in win32-x64|win32-arm64|linux-x64|linux-arm64|darwin-x64|darwin-arm64) ;; *) echo "Unknown platform: $platform" >&2; exit 2 ;; esac
done
cd "$here"
trap 'rm -rf "$here/bin"' EXIT
for platform in "${wanted[@]}"; do
    for index in "${!platforms[@]}"; do
        [ "$platform" = "${platforms[$index]}" ] || continue
        target="${targets[$index]}"
        name=markview
        [[ "$platform" != win32-* ]] || name=markview.exe
        binary="$repo/target/$target/release/$name"
        if [ ! -f "$binary" ]; then binary="$repo/target/release/$name"; fi
        node "$repo/editors/shared/check-engine.js" "$binary" "$platform"
        if [ "$check" = 1 ]; then continue; fi
        rm -rf "$here/bin"
        mkdir -p "$here/bin/$platform" "$here/dist"
        cp "$binary" "$here/bin/$platform/$name"
        chmod +x "$here/bin/$platform/$name"
        "$here/node_modules/.bin/vsce" package \
            --target "$platform" --out "$here/dist/markview-export-$platform.vsix"
    done
done
