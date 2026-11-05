#!/usr/bin/env bash
set -euo pipefail

layout=$1
boundary_width=$2
serial=emulator-5554
artifacts="artifacts/android/$layout"
mkdir -p "$artifacts"
rm -f "$RUNNER_TEMP/markview-android-tests-started"
retry() {
  local attempts=$1
  shift
  local attempt
  for ((attempt = 1; attempt <= attempts; attempt++)); do
    "$@" && return 0
    if ((attempt < attempts)); then
      echo "retrying (${attempt}/${attempts}): $*" >&2
      sleep $((attempt * 2))
    fi
  done
  return 1
}
# The boot property lands before `adbd` is always ready, so restarting it as
# root can race the daemon. A few short retries absorb that race.
retry 4 timeout 30 adb -s "$serial" wait-for-device
retry 4 timeout 30 adb -s "$serial" root
timeout 120 adb -s "$serial" wait-for-device
test "$(timeout 30 adb -s "$serial" shell id -u)" = 0
adb -s "$serial" shell cmd overlay enable-exclusive --category com.android.internal.systemui.navbar.gestural
adb -s "$serial" logcat -c
adb -s "$serial" logcat -v threadtime > "$artifacts/logcat.txt" &
logcat_pid=$!
finish() {
  status=$?
  if ((status != 0)); then
    adb -s "$serial" exec-out screencap -p > "$artifacts/after-test.png" || true
  fi
  kill "$logcat_pid" || true
  exit "$status"
}
trap finish EXIT

touch "$RUNNER_TEMP/markview-android-tests-started"
python3 android/test.py --serial "$serial" --layout "$layout" --shuffle 42 --timeout 600
adb -s "$serial" shell am force-stop io.github.szdytom.markview
boundary_size="${boundary_width}x1600"
if [[ $layout == tablet ]]; then
  boundary_size="1600x${boundary_width}"
fi
adb -s "$serial" shell wm size "$boundary_size"
adb -s "$serial" shell wm density 320
python3 android/test.py --serial "$serial" --layout "$layout" --layout-only
