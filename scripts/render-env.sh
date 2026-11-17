#!/usr/bin/env bash
set -euo pipefail

case "$#:${1:-}" in
  0:|1:--update) ;;
  *) echo "Usage: $0 [--update]" >&2; exit 2 ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
image=$(cat "$root/ci/render/image")
engine=${MARKVIEW_CONTAINER_ENGINE:-}
if [[ -z "$engine" ]]; then
  if command -v podman >/dev/null; then engine=podman; else engine=docker; fi
fi
cache="${XDG_CACHE_HOME:-$HOME/.cache}/markview/render/${image##*@sha256:}"
mkdir -p "$cache/cargo" "$cache/target"
userns=()
if [[ "${engine##*/}" == podman ]]; then userns=(--userns=keep-id); fi
printf 'Rendering environment: %s\n' "$image"
exec "$engine" run --rm --platform linux/amd64 --network=host "${userns[@]}" \
  --user "$(id -u):$(id -g)" \
  --volume "$root:/workspace" \
  --volume "$cache/cargo:/cargo" \
  --volume "$cache/target:/target" \
  --env HTTP_PROXY --env HTTPS_PROXY --env ALL_PROXY --env NO_PROXY \
  --env http_proxy --env https_proxy --env all_proxy --env no_proxy \
  --workdir /workspace "$image" "$@"
