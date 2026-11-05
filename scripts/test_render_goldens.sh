#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
export MARKVIEW_UPDATE_RENDER_GOLDENS=0
case "${1:-}" in
  '') ;;
  --update) export MARKVIEW_UPDATE_RENDER_GOLDENS=1 ;;
  *) echo "Usage: $0 [--update]" >&2; exit 2 ;;
esac
VK_DRIVER_FILES="${VK_DRIVER_FILES:-$(find /usr/share/vulkan/icd.d -name 'lvp_icd*.json' -print -quit)}"
export VK_DRIVER_FILES
test -n "$VK_DRIVER_FILES"
export WGPU_BACKEND=vulkan
export LP_NUM_THREADS=1
cargo test -p markview-render --locked --test golden rendering_baselines -- --nocapture
cargo test -p markview --locked --lib rendering_baselines -- --nocapture
