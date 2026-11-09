#!/usr/bin/env bash
# Run one fuzz target for a bounded time inside a pinned P-core slot.
#
#   campaign.sh <target> <seconds> [extra libFuzzer flags...]
#
# Everything the campaign record needs is written next to the log: the exact
# command line, the binary's mtime, the seed corpus state, the libFuzzer exit
# summary, and the peak RSS. Crashes land in the target's artifact directory,
# never as bare `crash-*` files at the crate root.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
target=${1:?usage: campaign.sh <target> <seconds> [flags...]}
secs=${2:?usage: campaign.sh <target> <seconds> [flags...]}
shift 2

crate="$root/fuzz"
out="$root/artifacts/fuzz-next"
bin="$crate/target/x86_64-unknown-linux-gnu/release/$target"
[[ -x $bin ]] || { echo "no binary: $bin (build it first)" >&2; exit 1; }

run_dir="$out/runs/$(date +%m%d-%H%M%S)-$target"
mkdir -p "$run_dir"
corpus="$crate/corpus/$target"
mkdir -p "$corpus"

# A crash this target has already produced must not end the run: the campaign
# is looking for something new, and libFuzzer re-derives known inputs fast.
# `-ignore_crashes` and `-ignore_ooms` only take effect in fork mode, so
# `-fork` is not optional here: without it the first known crash ends the
# block. One worker is enough to fill the single pinned slot.
#
# `-keep_seed` skips fork mode's initial set-cover merge, which runs before
# libFuzzer looks at `-max_total_time`: the accumulated `parse` corpus
# (229k units) made a 5 s block run 18 minutes without it, while the same
# block exits in 5.6 s with it.
#
# `-timeout` must clear the target's one-time warm-up, which libFuzzer counts
# as an input: the three targets that call `pipeline::warmup` pay 45-75 s on a
# pinned P-core *before* the first fuzz iteration, and a hard-coded 30 s
# timeout fails on an idle machine. WARMUP_SECS states the target's real cost
# in seconds; the default 90 covers the measured 75 s maximum, and the timeout
# is three times it plus a minute so a descheduled process does not read as a
# hang.
timeout_s=${TIMEOUT_SECS:-$(( ${WARMUP_SECS:-90} * 3 + 60 ))}
flags=(
	-fork="${FORK_JOBS:-1}"
	-keep_seed=1
	-ignore_crashes=1
	-ignore_ooms=1
	-timeout="$timeout_s"
	-rss_limit_mb=${RSS_LIMIT_MB:-3072}
	-malloc_limit_mb=${MALLOC_LIMIT_MB:-2048}
	-max_total_time="$secs"
	-artifact_prefix="$run_dir/"
	-print_final_stats=1
)
# A target that reads no corpus bytes still needs a directory to write new
# units into; `-reload=0` keeps long runs from re-reading it constantly.
[[ ${RELOAD:-0} == 1 ]] || flags+=(-reload=0)
flags+=("$@")

log="$run_dir/$target.log"
{
	echo "== command =="
	printf 'slot.sh --wait %q ' "$bin"
	printf '%q ' "${flags[@]}"
	printf '%q\n' "$corpus"
	echo "== binary =="
	stat -c '%n %s bytes mtime=%y' "$bin"
	echo "== corpus =="
	echo "$target: $(find "$corpus" -type f | wc -l) units"
	echo "== start $(date -Is) =="
} >"$log"

export ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:allocator_may_return_null=1}"
export UBSAN_OPTIONS="${UBSAN_OPTIONS:-print_stacktrace=1}"
export LLVM_SYMBOLIZER_PATH="${LLVM_SYMBOLIZER_PATH:-$(command -v llvm-symbolizer || true)}"

# GNU `time -v` supplies the peak RSS; the shell keyword cannot. The binary is
# not always at `/usr/bin/time` (NixOS ships it in the system profile), and a
# plain `time` rejects `-v`, so resolve it and drop the timer if it is missing.
timer=()
if time_bin=$(type -P time || true) &&
	[[ -n $time_bin ]] && "$time_bin" -v true 2>&1 | grep -q 'Maximum resident'; then
	timer=("$time_bin" -v)
fi

set +e
"${timer[@]}" "$root/fuzz/scripts/slot.sh" --wait \
	"$bin" "${flags[@]}" "$corpus" >>"$log" 2>&1
status=$?
set -e

{
	echo "== end $(date -Is) status=$status =="
} >>"$log"

# Keep the interesting tail in one place for the campaign log.
grep -E '^(#|==|stat::|statistics|SUMMARY|ERROR|WARNING|.*peak RSS|\s*Maximum resident)' "$log" |
	tail -40 >"$run_dir/summary.txt" || true
echo "$run_dir"
