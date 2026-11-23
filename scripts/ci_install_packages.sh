#!/usr/bin/env bash
set -euo pipefail

scripts=$(dirname "$(realpath "$0")")
sources=$(mktemp -d)
trap 'rm -rf "$sources"' EXIT
options=(-o "Dir::Etc::sourceparts=$sources" -o Acquire::Retries=3
  -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
install_options=(-y)
if [[ -n ${MARKVIEW_APT_SNAPSHOT:-} ]]; then
  printf '%s\n' \
    'Types: deb' \
    "URIs: https://snapshot.ubuntu.com/ubuntu/$MARKVIEW_APT_SNAPSHOT/" \
    'Suites: noble noble-updates noble-security' \
    'Components: main universe' \
    'Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg' \
    > "$sources/ubuntu.sources"
  options+=(-o Dir::Etc::sourcelist=/dev/null)
  install_options+=(--allow-downgrades)
elif [[ -f /etc/apt/sources.list.d/ubuntu.sources ]]; then
  # Keep the image's Ubuntu mirrors and signature configuration for unpinned jobs.
  cp /etc/apt/sources.list.d/ubuntu.sources "$sources/"
fi
bash "$scripts/ci_retry.sh" sudo apt-get "${options[@]}" update --error-on=any
bash "$scripts/ci_retry.sh" sudo apt-get "${options[@]}" install "${install_options[@]}" "$@"
