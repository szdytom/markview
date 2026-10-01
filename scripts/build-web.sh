#!/usr/bin/env bash
# Builds the WebAssembly front end and its JavaScript bindings into the
# `@markview/viewer` package's `wasm/` directory.
#
# The `wasm-bindgen` CLI's version must equal the `wasm-bindgen` crate's, so
# the pin in `crates/markview-web/Cargo.toml` and the pinned CLI move together.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

out="web/packages/markview/wasm"

version="0.2.129"
pinned="$root/.tools/wasm-bindgen-$version/wasm-bindgen"
cli="${WASM_BINDGEN:-}"
if [[ -z "$cli" && -x "$pinned" ]]; then
	cli="$pinned"
fi
if [[ -z "$cli" ]]; then
	cli="$(command -v wasm-bindgen || true)"
fi
if [[ -z "$cli" ]]; then
	echo "wasm-bindgen $version not found; set \$WASM_BINDGEN" >&2
	exit 1
fi

cargo build -p markview-web --target wasm32-unknown-unknown --release --features woff

rm -rf "$out"
mkdir -p "$out"
"$cli" \
	--target web \
	--out-dir "$out" \
	--out-name markview_web \
	target/wasm32-unknown-unknown/release/markview_web.wasm

# The TypeScript package copies the binary beside its JavaScript under the
# plain name `init()` resolves; text fonts are separate host assets.
echo "built $out/markview_web.js ($(du -h "$out/markview_web_bg.wasm" | cut -f1))"
