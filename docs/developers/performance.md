# Measure performance

Use paired measurements to decide whether a change improves the reader. Historical
absolute numbers are evidence about a particular build and machine, not a current
performance guarantee.

## Choose the metric

| Question | Tool | Boundary |
| --- | --- | --- |
| When can a native user first read? | `smoke` | Process entry to a readable GPU-completed frame; excludes compositor presentation |
| What does complete layout cost? | `bench` | Offscreen complete geometry; not native first-frame latency |
| How fast does a file edit appear? | `latency`, `scripts/smoke_watch.py` | Offscreen worker publication versus native watched-file delivery |
| Does memory grow during reuse? | Repeated edits and scrolling | Record RSS, peak RSS and tracked GPU allocations separately |

Build the diagnostic commands explicitly:

```sh
cargo build --release --locked --features diagnostics
target/release/markview bench tests/fixtures/ordinary-10k.md --output artifacts/ordinary.json
target/release/markview latency tests/fixtures/ordinary-10k.md --output artifacts/latency.json
target/release/markview smoke examples/welcome.md --output artifacts/window.png
python3 scripts/smoke_watch.py target/release/markview
```

Use `--scale 2` on `bench` or `latency` for a high-DPI comparison. Native smoke
needs a desktop; offscreen commands still need a GPU backend. Software Vulkan
is useful for correctness checks but cannot stand in for hardware performance.

## Choose representative inputs

Include ordinary prose, CJK, math, code, images and cold font fallback. Repeated
paragraphs favor content caches. Generate unique and cache-stressing inputs with
`python3 scripts/generate_stress_fixtures.py` before making scaling claims.
Include `tests/fixtures/emoji-fallback.md` in cold-process comparisons.

`MARKVIEW_PROFILE=1` adds diagnostic spans to benchmark reports. These spans can
be inclusive and can accumulate parallel worker time; do not add them together
or interpret them as wall time. Leave profiling off for acceptance measurements.

## Refactor with a preserved baseline

Before changing a performance-sensitive path, build the current release and copy
its binary outside `target/release`. Keep its source revision with it. After the
change, build the candidate with the same lockfile, toolchain and release profile.
Compare the preserved and candidate binaries on the same machine with hardware
GPU access, under the same power mode and operating conditions. Battery power
and `power-saver` are valid for regression checks; AC power and the `performance`
profile are not prerequisites. Alternate the binaries across repeated process
groups, keeping power mode, CPU affinity, fonts, DPI and driver consistent.
Avoid competing workloads and record changes in thermal or power conditions.
Power mode alone neither establishes nor excuses
a regression; investigate differences using paired measurements and A/A controls.

Refresh the absolute performance figures in the README only during a release,
using a recorded, reproducible measurement environment. Ordinary refactors
validate relative performance against their preserved baseline and do not need
to reproduce or update the README's absolute numbers.

```sh
python3 scripts/compare_performance.py \
  --baseline artifacts/refactor/baseline/markview \
  --baseline-revision BASELINE_COMMIT \
  --candidate target/release/markview \
  --output artifacts/refactor/comparison
```

The output directory must be new. By default the script alternates baseline and
candidate order across five groups, using 100 full-layout samples and 100 cached
samples per process, on all four 10 KiB fixtures, the local image example and
the small Emoji fallback fixture.
It preserves individual JSON reports, binary hashes, font inventory hash,
platform/backend metadata, CPU affinity, and a Markdown/JSON comparison. An incompatible
adapter, input, viewport or semantic result fails the comparison.

First-open medians come from independent processes; they are not warm-reflow P95s
and do not flush the OS file cache. The acceptance metrics are first open, full
and cached P50/P95, post-scroll RSS/peak RSS, and tracked GPU bytes. Each uses the
median of its per-process measurements; an increase over 5% fails. Initialization
and individual stage medians remain diagnostic alongside raw samples because
very small stage durations are sensitive to timer and scheduling noise. Missing
memory measurements are reported as unavailable, not a pass. Repeat an unstable
comparison and investigate outliers rather than changing the threshold.
The benchmark also separates `gpu_prepare_and_submit_ms` from `gpu_wait_ms`;
the latter includes blocking driver completion and host scheduling, not just
GPU execution. Both remain included in the original end-to-end timing.

For unexplained P95 failures, first run an A/A control by supplying the same
preserved binary as both sides. A failed A/A gate demonstrates measurement
variability, not a code regression. On hybrid CPUs, compare both versions with
the same affinity (for example, `taskset -c CPU_LIST python3 scripts/compare_performance.py …`
on Linux, after identifying the local core topology). Use `--iterations 500`
or more to increase tail samples. Preserve these runs separately when affinity
or iteration counts differ; do not merge incompatible experiments or relax the
5% gate. A controlled pass does not erase an earlier failed measurement.

Keep visual checks alongside timing checks. Compare deterministic offscreen
exports with the preserved binary, then run the rendering tests and native
window/watch smoke tests. Native Windows/macOS CI checks remain necessary;
Linux GPU measurements do not establish runtime behavior on those platforms.

When a noisy fixture needs additional groups, retain the original groups and
merge them with the follow-up instead of selecting the more favorable run:

```sh
python3 scripts/compare_performance.py \
  --merge artifacts/refactor/acceptance artifacts/refactor/long-code-followup \
  --output artifacts/refactor/combined
```

Merging verifies identical binary hashes, toolchain, fonts, CPU affinity, backend,
profile and iteration counts; it reports the number of groups for each fixture.

## Record a result

Keep the source revisions, binary hashes, commands, raw results, toolchain,
fixtures, fonts, viewport/DPI, GPU/driver, operating system and power conditions
with the result. Label unavailable measurements explicitly. A report should say
which decision its evidence supports and what remains unmeasured.

Do not append each experiment to this guide. Store durable investigations in
[history](history/README.md), with dates and revisions; keep transient run outputs
under `artifacts/`. Existing [baseline tables](history/performance-baselines.md),
[latency analysis](history/performance-analysis.md),
[Windows startup study](history/windows-startup.md) and
[document memory study](history/document-memory.md) retain their original scope.
