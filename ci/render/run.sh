#!/usr/bin/env bash
set -euo pipefail

rustc --version
cargo --version
sed -n '/^model name/{p;q;}' /proc/cpuinfo
dpkg-query -W mesa-vulkan-drivers libllvm20 libfontconfig1 libfreetype6 libvulkan1 poppler-utils
vulkaninfo --summary
exec bash scripts/test_render_goldens.sh "$@"
