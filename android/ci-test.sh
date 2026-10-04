#!/usr/bin/env bash
set -euo pipefail

layout=$1
boundary_width=$2
serial=emulator-5554
artifacts="artifacts/android/$layout"
mkdir -p "$artifacts"
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

python3 android/test.py --serial "$serial" --layout "$layout" --timeout 600
adb -s "$serial" shell am force-stop io.github.szdytom.markview
boundary_size="${boundary_width}x1600"
if [[ $layout == tablet ]]; then
  boundary_size="1600x${boundary_width}"
fi
adb -s "$serial" shell wm size "$boundary_size"
adb -s "$serial" shell wm density 320
python3 android/test.py --serial "$serial" --layout "$layout" --layout-only
