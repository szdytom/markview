#!/usr/bin/env bash
# Fail fast if the shared `mvfuzz` library does not compile.
#
# `mvfuzz` is linked by every fuzz target, so one broken struct or a
# half-finished signature anywhere under `fuzz/fuzzlib` stops *the whole
# team's* builds — not just the author's. That happened three times in the
# first forty minutes of the overnight campaign and cost roughly forty
# minutes of wall clock each time, because the author was mid-edit and the
# other three had to discover the break independently.
#
#   guard.sh          check the shared lib only (fast, ~2 s warm)
#   guard.sh --full   check the lib and every registered target
#
# Intended to be run before parking a task, before a long fuzz block, and by
# any loop that edits `fuzz/fuzzlib`.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root/fuzz"

# The log directory is gitignored and absent on a fresh checkout, and `TMPDIR`
# may point at a path that does not exist yet; `tee` would then fail and, with
# `pipefail`, report a green build as a compile error.
log_dir="${TMPDIR:-$root/artifacts/fuzz-next}"
mkdir -p "$log_dir"
log="$log_dir/guard.log"

if ! cargo +nightly check -p mvfuzz 2>&1 | tee "$log" | tail -5; then
	echo "guard.sh: the shared mvfuzz library does not compile." >&2
	echo "guard.sh: every fuzz target is blocked until this is fixed." >&2
	exit 1
fi

if grep -q '^error' "$log"; then
	echo "guard.sh: mvfuzz reported errors; see the log above." >&2
	exit 1
fi

if [[ ${1:-} == --full ]]; then
	cargo +nightly build --release \
		--bin parse --bin reparse --bin prefix --bin mvss --bin layout \
		--bin layout_diff --bin layout_parallel --bin math --bin highlight --bin shaping \
		--bin fonts --bin pdf --bin geometry --bin refdef \
		--bin sourcepos_content --bin details_structure --bin anchors \
		--bin webpage --bin webpage_seam \
		2>&1 | tail -5
fi

echo "guard.sh: mvfuzz is green."
