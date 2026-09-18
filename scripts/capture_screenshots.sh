#!/usr/bin/env bash
# Regenerate the screenshots embedded in the READMEs.
#
# This drives the real window on a KDE/Wayland desktop: it launches Markview,
# waits for it to paint, and asks Spectacle for the active window. The capture
# is then cropped to the window frame and downscaled to a 2x image.
#
# Usage: scripts/capture_screenshots.sh [NAME ...]
# Set MARKVIEW to use a binary other than target/release/markview.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
binary=${MARKVIEW:-$root/target/release/markview}
source_dir=$root/docs/screenshots/source
output_dir=$root/docs/screenshots
work=$(mktemp -d)
trap 'rm -rf "$work"; pkill -x markview 2>/dev/null || true' EXIT

readonly width=1280
readonly height=840
requested=("$@")

wanted() {
	[ ${#requested[@]} -eq 0 ] && return 0
	printf '%s\n' "${requested[@]}" | grep -qx "$1"
}

# `shot NAME SOURCE [ARG ...]` captures one window.
shot() {
	local name=$1 source=$2
	shift 2
	echo "capturing $name from $source"
	pkill -x markview 2>/dev/null || true
	sleep 0.5
	RUST_LOG=info "$binary" --width "$width" --height "$height" "$@" \
		"$source_dir/$source" >"$work/$name.log" 2>&1 &
	for _ in $(seq 1 60); do
		grep -q 'framebuffer' "$work/$name.log" && break
		sleep 0.25
	done
	sleep 2.5
	spectacle -b -n -a -o "$work/$name.png" >/dev/null
	pkill -x markview 2>/dev/null || true
	# The heredoc body is flush left so that Python keeps its indentation.
	python3 - "$work/$name.png" "$output_dir/$name.png" "$((width * 2 + 4))" <<'PY'
import sys

import numpy as np
from PIL import Image

src, dst, expected = sys.argv[1], sys.argv[2], int(sys.argv[3])
image = Image.open(src).convert("RGBA")
alpha = np.array(image)[:, :, 3]
# Spectacle captures the window plus its shadow; keep the opaque frame and a
# small transparent halo, then scale the frame to 1600 px wide.
ys, xs = np.where(alpha >= 250)
frame = int(xs.max()) - int(xs.min()) + 1
if abs(frame - expected) > 40:
    sys.exit(f"{src}: window is {frame} px wide, expected ~{expected}")
pad = 44
box = (int(xs.min()) - pad, int(ys.min()) - pad,
       int(xs.max()) + pad + 1, int(ys.max()) + pad + 1)
crop = image.crop(box)
scale = 1600 / frame
out = crop.resize((round(crop.width * scale), round(crop.height * scale)),
                  Image.LANCZOS)
out.save(dst, optimize=True)
print(f"  wrote {dst} ({out.width}x{out.height})")
PY
}

if wanted en-typography;   then shot en-typography   en/typography.md   --light; fi
if wanted en-mathematics;  then shot en-mathematics  en/mathematics.md  --light; fi
if wanted en-structure;    then shot en-structure    en/structure.md    --dark --scroll 40; fi
if wanted zh-typography;   then shot zh-typography   zh/typography.md   --light; fi
if wanted zh-mathematics;  then shot zh-mathematics  zh/mathematics.md  --light; fi
if wanted zh-structure;    then shot zh-structure    zh/structure.md    --dark --scroll 40; fi
