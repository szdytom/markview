#!/usr/bin/env bash
# Keep every pinned P-core slot busy with the highest-value target available.
#
#   campaign-supervisor.sh [hours]
#
# `hours` may be fractional, so an overnight window can end on a wall-clock
# deadline instead of the nearest whole hour.
#
# The Lead runs this for the overnight window. It is a *fill* loop, not a
# scheduler: each tick it counts the free slots, starts exactly that many
# blocks, and returns. Blocks go through `campaign.sh`, so pinning, the RSS
# ceiling and the per-run record are identical to a hand-started run.
#
# Steady state never exceeds the eight slots, which is the user's stated
# budget: at most eight threads, pinned to the P-cores. Builds and triage are
# transient and are not started from here.
#
# `slot.sh --try` is the allocation primitive: it exits 75 when no slot is
# free and otherwise runs the command in the slot it claimed. Counting free
# slots first keeps this loop from queueing work on top of a running block.
set -uo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
hours=${1:-6}
deadline=$(( $(date +%s) + $(awk -v h="$hours" 'BEGIN { printf "%d", h * 3600 }') ))
log="$root/artifacts/fuzz-next/supervisor.log"
# `.slots` is gitignored, so a fresh checkout lacks it and every lock open in
# `free_slots` would fail, reporting zero free slots for the whole window.
mkdir -p "$root/artifacts/fuzz-next/.slots"

# Priority order, most productive per slot-second first. `layout`,
# `layout_diff` and `highlight` pay 45-75 s of warm-up per process, so they
# get shorter blocks.
rotation=(
	"parse:1200"
	"reparse:1200"
	"refdef:1200"
	"anchors:900"
	"sourcepos_content:900"
	"details_structure:900"
	"prefix:600"
	"layout:600"
	"layout_diff:600"
	"layout_parallel:600"
	"pdf:600"
	"mvss:600"
	"geometry:600"
	"math:1200"
	"fonts:600"
	"shaping:600"
	"highlight:600"
	"highlight_schedule:600"
	"reading_pipeline:900"
	"webpage_seam:900"
	"webpage:900"
	"webpage_layout:900"
	"paginate_pipeline:600"
	"image_decode:600"
	"interaction_sequence:600"
	"font_archive:600"
)
idx=0

free_slots() {
	local n=0 i
	for ((i = 0; i < 8; i++)); do
		# Append rather than truncate: a held lock file records its holder for
		# `slot.sh --list`, and opening with `>` would erase it.
		if (exec 9>>"$root/artifacts/fuzz-next/.slots/slot$i.lock"; flock -n 9) 2>/dev/null; then
			n=$((n + 1))
		fi
	done
	printf '%s' "$n"
}

while (($(date +%s) < deadline)); do
	free=$(free_slots)
	for ((k = 0; k < free; k++)); do
		# Walk the rotation until one lands on a free slot; `--try` refuses
		# rather than blocking, so a lost race costs one retry, not a stall.
		for ((attempt = 0; attempt < ${#rotation[@]}; attempt++)); do
			entry=${rotation[$((idx % ${#rotation[@]}))]}
			idx=$((idx + 1))
			target=${entry%%:*}
			secs=${entry##*:}
			[[ -x "fuzz/target/x86_64-unknown-linux-gnu/release/$target" ]] || continue
			printf '[%s] start %s %ss\n' "$(date +%H:%M:%S)" "$target" "$secs" >>"$log"
			nohup fuzz/scripts/campaign.sh "$target" "$secs" >>"$log" 2>&1 &
			break
		done
		sleep 4
	done
	sleep 60
done
