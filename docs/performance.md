# Performance model

This page explains what Markview measures and how to interpret the current baseline. It is not a change log and does not define a performance guarantee.

The [latency and memory analysis](performance-analysis.md) is the companion diagnostic page: it measures the reader against explicit first-frame, edit-latency and memory targets, locates the responsible code, and ranks the optimization points. The fixtures shipped here repeat paragraph bodies, so a content-keyed block cache flatters them; that page adds unique-content stress fixtures.

## What the timings mean

- **Initialization** covers device, pipeline, font, and renderer setup before a document is opened.
- **Offscreen first frame** covers reading, parsing, full geometry layout, visible glyph preparation, and GPU completion. It does not include compositor presentation.
- **Window first readable frame** covers initialization and opening through GPU completion of the first visible document prefix. Remaining geometry can finish later; compositor presentation is not measured.
- **Full reflow** measures rebuilding document geometry after content or layout settings change. The block cache is cleared for this measurement, while font and shaping resources remain warm.
- **Block refresh** measures reusing unchanged blocks after a localized invalidation, such as an image completing or a file update affecting only part of the document.
- **RSS** is process resident memory after scrolling through the document. It is not GPU memory.
- **Scroll pacing** is the per-frame cost of moving through the whole document a screenful at a time, in three passes: `cold` starts with an empty glyph atlas, `warm` reuses what the first pass rasterized, and `prewarmed` gives the renderer the budgeted head start the window runs while the reader stays put. Each pass counts frames that miss a 120 Hz (8.33 ms) and a 60 Hz (16.7 ms) budget, and records how many glyphs or paths were rasterized, how many of those fell inside a frame the reader waited for, and how full the atlas ended. Compositor presentation is excluded, so a frame here is what Markview has to produce, not what the display shows.
- **Tracked GPU resources** are the capacities of resources Markview can account for; they are not a complete driver-memory report.

The benchmark separates a cold first open from repeated warm reflows. A P95 from repeated runs must not be presented as a cold-start P95.

## Current repository baseline

The 2026-09-12 refactor comparison used Linux x86_64, an Intel Core Ultra 5
125H with Intel Arc (MTL) through Vulkan, Rust 1.96.0-nightly (2026-03-26),
release mode with thin LTO and one codegen unit, bundled light styles, system
fonts, 18 px text, a 760 px column and a 1200 × 800 offscreen target at scale 1.
Both binaries used the default affinity across all 18 logical CPUs.

| Fixture | First open (ms) | Full pipeline P95 (ms) | Cached pipeline P50 (ms) | RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| ordinary-10k | 26.16 | 13.74 | 0.63 | 45.79 |
| math-10k | 28.93 | 13.76 | 0.72 | 47.74 |
| code-10k | 16.77 | 11.01 | 0.50 | 55.32 |
| long-code-10k | 17.03 | 11.34 | 0.66 | 51.77 |
| images | 46.73 | 4.09 | 0.50 | 47.95 |

The comparison preserved the release binary from commit
`2529179b65f63a32badf02a6e33dd46160ba8783` and alternated it with the candidate
for five groups of 100 full and 100 cached iterations per process. The long-code
fixture needed 15 additional groups because GPU wait times were noisy; all 20
groups were retained. Every acceptance metric stayed within the 5% regression
limit. Tracked GPU capacities were unchanged.

Computing the immutable stylesheet layout identity once per document pass,
instead of once per block lookup, reduced ordinary cached-pipeline P50 from
2.05 ms to 0.63 ms and ordinary full-pipeline P95 from 15.18 ms to 13.74 ms.
Cache identity and invalidation semantics did not change. These are whole-pipeline
measurements: `full_layout_reopens` and `cached_refreshes` summarize `total_ms`,
including read, parse and completed GPU work. Individual `layout_ms` samples
remain available for geometry-only diagnosis.

The local raw reports, environment and binary hashes, early diagnostic runs,
and merged comparison are under `artifacts/refactor/`; these generated artifacts
are ignored by Git. See the [development guide](development.md#refactor-with-a-preserved-baseline)
for the repeatable comparison and merge commands.

## Native baseline on the performance profile

The headline figures in the root README were measured on 2026-09-18 on one
ordinary laptop: an Intel Core Ultra 5 125H with integrated Intel Arc (MTL)
through Vulkan, 18 logical CPUs, Arch Linux with kernel 7.2.6, Rust
1.96.0-nightly (2026-03-26), release mode with thin LTO and one codegen unit,
system fonts, 18 px text, a 760 px column, a 1200 × 800 window at DPR 2 on a
2880 × 1920 display, and the `performance` power profile.

**First readable frame** is the `process entry→readable GPU frame` line that
`smoke-test` logs and that the window logs as well: process entry through
parsing, geometry, glyph preparation and GPU completion of the first readable
prefix, with window creation and initialization included and compositor
presentation excluded. Each row is fifteen independent processes:

| Fixture | Bytes | Median (ms) | Range (ms) |
| --- | ---: | ---: | ---: |
| ordinary-10k | 10240 | 75.9 | 66.6–87.8 |
| math-10k | 10240 | 82.0 | 69.6–91.3 |
| text-cjk-100k | 102400 | 80.8 | 69.1–87.3 |
| math-cjk-100k | 102400 | 83.0 | 69.9–89.8 |
| text-cjk-1000k | 1024000 | 78.8 | 71.3–89.4 |

The ranges overlap completely. Between runs the spread is wider than the
difference between a 10 KiB note and a 1 MiB book, so what this table supports
is that the first frame stopped scaling with the document, not that one fixture
is faster than another. `text-cjk-1000k` is `text-cjk-100k` concatenated ten
times; it is generated for the measurement and is not committed.

**Resident memory** comes from two modes that measure different work. The
`latency` column is `memory.after_scroll`, the resident set after
scrolling through the document, from five processes with twenty edit iterations
each at scale 1. The `bench` column is `memory_after_scroll` after full-layout
reopens, from five processes with ten iterations each:

| Fixture | `latency` RSS (MiB) | `bench` RSS (MiB) | `bench` first open (ms) |
| --- | ---: | ---: | ---: |
| ordinary-10k | 42.2 | 41.6 | 15.6 |
| math-10k | 44.6 | 44.4 | 18.3 |
| text-cjk-100k | 48.5 | 45.3 | 20.5 |
| math-cjk-100k | 50.0 | 47.9 | 23.5 |
| text-cjk-1000k | 82.7 | 70.9 | 71.7 |

The two agree within 1 MiB up to 100 KiB and differ by about 12 MiB on the
megabyte fixture, where the edit loop retains more than a full-layout pipeline
does. `bench` first open is the mean of ten iterations rather than a P95, and
it excludes initialization; it is listed for continuity with the older tables,
not as the interactive figure.

Both memory columns are far below the 2026-09-15 diagnostic, which recorded
82.4 MiB for `text-cjk-100k` and 79.7 MiB for `math-cjk-100k` through the same
`bench` mode on this host. The builds differ, so the two tables must not be
pooled, and this page does not attribute the difference to one change.

The power profile is part of the result rather than a detail: the progressive
window first frame table below recorded 148.97 ms for `ordinary-10k` under
`power-saver` where this run records 75.9 ms. Those measurements are days and
several changes apart, so the comparison sizes the effect instead of isolating
it; a number reported without its power state is not comparable.

Reproduce the latency column with `RUST_LOG=info target/release/markview
smoke-test FIXTURE --width 1200 --height 800 --offline`, once per process; the
two memory columns with `latency FIXTURE --iterations 20 --offline` and
`bench FIXTURE --iterations 10 --offline`. The raw reports and the generated
megabyte fixture are under `artifacts/readme-baseline/` (ignored by Git).

## Large-document reference

The 10 KiB fixtures bound the comparison work, not the reader's input. A
2026-09-15 diagnostic run on the machine above measured 100 KiB CJK/English
fixtures generated by `scripts/generate_large_fixture.py`, with the same
offscreen method as the table above. `text-cjk-100k` has no math;
`math-cjk-100k` has 833 inline/display spans:

| Fixture | First open (ms) | Full reflow P95 (ms) | Cached P50 (ms) | RSS (MiB) |
| --- | ---: | ---: | ---: | ---: |
| ordinary-10k (same session) | 24.9 | 13.3 | 0.8 | 45.5 |
| math-10k (same session) | 26.8 | 13.4 | 0.6 | 47.8 |
| text-cjk-100k | 133.3 | 119.7 | 2.4 | 82.4 |
| math-cjk-100k | 128.0 | 112.0 | 2.6 | 79.7 |

In that diagnostic baseline, layout is ~90 % of the first open, and about two thirds of layout is text
shaping: the same paragraph is shaped once to measure it (`units`) and again
per line (`line_clusters`), and `choose_font` re-scans font coverage per
grapheme cluster. Formula typesetting is negligible for repeated LaTeX. The
full stage-by-stage breakdown and method notes are in
`artifacts/performance-large-100k.md`.

Font selection now resolves each appearance's candidate set once per shaping
call and retains the selected face index for repeated grapheme clusters or
joining-script words. Coverage still requires the whole cluster/word; unsupported
text is cached too. Each candidate set retains at most 4096 choices of at most
128 UTF-8 bytes each, and setting the stylesheet clears all candidate sets and
choices. This avoids repeated font-list allocation, coverage scans, and face
cloning during the first document layout. Paragraph measurement and line-boundary
reshaping remain separate so kerning, ligatures, bidi, and inserted hyphens keep
their existing behavior. The font-selection comparison below predates progressive
window layout and therefore waits for full document geometry.

The same-day optimization comparison preserved release commit
`3a99ac40ec3266861044ce839843c07093f21587` and alternated it with the candidate
on the same Intel Arc/Vulkan host, this time with the existing `power-saver`
profile left unchanged. Fifteen process groups per large fixture (30 full and
30 cached samples each) produced these medians; these absolute times are not
comparable to the performance-profile reference above:

| Fixture | Baseline first open (ms) | Optimized first open (ms) | Change | Full reflow P95 before → after (ms) |
| --- | ---: | ---: | ---: | ---: |
| text-cjk-100k | 227.30 | 174.30 | −23.3% | 220.18 → 164.28 |
| math-cjk-100k | 227.77 | 178.83 | −21.5% | 208.23 → 160.16 |

Both large fixtures' cached timings and RSS stayed within the 5% regression
threshold; tracked GPU capacities were unchanged. Sixteen baseline/candidate
PNG pairs were byte-identical across six fixtures and two examples, at the top
at scale 1 and scrolled 1800 px at scale 2. Workspace tests, the GPU tests,
native-window opening, and watch smoke checks passed. Raw measurements, binary
hashes, environment metadata, and verification notes are under
`artifacts/open-optimization/` (ignored by Git).

The full comparison is not an all-metrics acceptance pass: after retaining 35
ordinary-10k process groups, cached-refresh P50 was 1.083 → 1.165 ms (+7.6%)
and P95 was 1.747 → 2.036 ms (+16.5%). Cached geometry medians were nearly
unchanged (0.069 → 0.071 ms), while GPU-completion medians increased
(0.764 → 0.806 ms). This does not establish the cause, and the small-file
cached-refresh regression remains unresolved under this power profile.

These numbers require the host to be in its normal power state. The same
session first measured ordinary-10k at 42.6 ms on battery with the
`power-saver` profile: the preserved 2026-09-12 binary reproduced that slow
result unchanged, and switching to the `performance` profile restored the
recorded values, so the difference was the platform power limit and not a
regression. Check the power profile before interpreting a comparison.

## Progressive window first frame

The window publishes complete-block prefixes for files of at least 32 KiB,
starting when the viewport and half a viewport of prefetch are covered. Smaller
files normally publish once, but after 32 ms of layout they may publish their
first nonempty prefix even if it does not yet cover the viewport. Subsequent
prefixes retain the coverage, geometric batching and 32 ms throttling rules;
changed viewport demand can bypass batching once covered. Checks occur only at
block boundaries, so a single expensive block still bounds first-frame latency.
Background completion shares the prefix's geometry;
the offscreen benchmark continues to measure full layout.

A same-host, power-saver, DPR-2 native comparison against the font-selection
commit `07fe6bb` used five alternating process pairs per fixture. These are
process-entry-to-first-readable-GPU-frame medians, including initialization,
not the offscreen first-open numbers above:

| Fixture | Baseline (ms) | Progressive (ms) |
| --- | ---: | ---: |
| ordinary-10k | 144.46 | 148.97 |
| text-cjk-100k | 265.03 | 138.65 |
| math-cjk-100k | 262.17 | 142.87 |
| text-cjk-1000k | 1654.30 | 150.48 |

The 1000 KiB fixture concatenates the 100 KiB text fixture ten times. First-frame
reading-area PNG crops matched the baseline exactly for all four fixtures;
chrome differs because the partial snapshot shows loading and no total-height
scrollbar. The smoke process continues through the final rendered snapshot.
In a separate native check, replacing the 1000 KiB file immediately after its
first readable frame produced the replacement's frame in 41.53 ms; only the
replacement published completion. Worker and session tests also cover target
changes, cancellation, prefix sharing, selection preservation, and delayed End.

The initial empty-state overlay now distinguishes opening from a confirmed
empty document. Full parsing, font/renderer initialization, and any single huge
top-level block still bound latency. This is not arbitrary-position virtual
layout: distant scrolling waits for preceding geometry. Raw logs, binary hashes,
screenshots, and scripts are in `artifacts/progressive/` (ignored by Git).

The supplemental offscreen comparison is not an all-metrics acceptance pass.
After retaining ten process groups for each large fixture (ten full and ten
cached samples per process), math-cjk-100k cached P50 was 2.512 → 3.124 ms and
P95 was 4.519 → 6.281 ms, above the 5% threshold. Parse, cached layout, and GPU
stages all increased in those samples; the cause has not been established.
The other measured acceptance metrics in `full-combined/comparison.md` passed.
These measurements remain separate from the native first-readable-frame result.

## Small documents with bold Emoji

The 3691-byte local `artifacts/test.md` exposed a cold font fallback cost:
the default Emoji candidate inherited weight 700 inside strong text, while
Noto Color Emoji only provided a regular face. Exact-face selection rejected
it, leaving Parley to find another font. Even `**✅ test**` reproduced the
delay. Bundled Emoji candidates now explicitly request weight 400, including
inside headings and emphasis. The deterministic font tests use registered
fixture fonts; `tests/fixtures/emoji-fallback.md` adds a portable manual and
performance regression input without depending on that local document.

On the same Intel Arc/Vulkan host in power-saver mode, five alternating release
process pairs against `1611936`, with 30 full and 30 cached iterations each,
gave these medians (offscreen full geometry plus completed first-viewport GPU
work, excluding initialization):

| Fixture | First open before → after | Full pipeline P95 before → after |
| --- | ---: | ---: |
| local test.md | 232.45 → 38.81 ms | 15.12 → 12.26 ms |
| emoji-fallback | 229.04 → 27.64 ms | 3.80 → 2.99 ms |
| ordinary-10k | 35.61 → 35.16 ms | 20.91 → 19.60 ms |

The local document's cold geometry median was 211.70 → 14.78 ms. A separate
single native DPR-2 pair measured process-entry-to-first-readable-GPU-frame at
342.68 → 146.41 ms; this is a smoke result, not a distribution. Screenshots
exposed an existing alpha-only color-glyph path that lost Emoji interior
details; the new color atlas preserves those pixels in both themes.

These first-open improvements are not an all-metrics acceptance pass. In the
five-group run, the local document's cached P50 was 0.984 → 1.251 ms and P95
1.559 → 1.660 ms; ordinary cached P50 was 1.147 → 1.256 ms. The image example's
first open and full P95 also exceeded the 5% threshold. Color-bearing fixtures
intentionally use one additional 1 MiB GPU atlas; ordinary, math, code and
image-only fixtures retained their tracked GPU capacities. Raw reports and
binary/environment metadata are in `artifacts/emoji-fix-final/`; the earlier
`emoji-fix-comparison/` reports predate correct color rendering and must not be
pooled with the final binary.

A supplemental five-pair run with 100 full/cached iterations per process is
retained separately in `artifacts/emoji-fix-followup/` (the iteration counts
differ, so the comparison tool does not merge them). Local first open was
244.37 → 36.79 ms, and cached P50 remained higher at 1.149 → 1.285 ms. Its
cached geometry medians were 0.0646 → 0.0727 ms while GPU-completion medians
were 0.8318 → 1.0127 ms. Ordinary passed all thresholds in this follow-up;
image first-open/full-layout thresholds passed, but image cached P95 was
1.306 → 1.599 ms. The inconsistent image/ordinary tail results do not establish
a cause; they remain recorded rather than treated as an all-metrics pass.

## Emoji face selection

A cluster that Unicode presents as Emoji now takes the definition marked
`emoji = true` ahead of the reading fonts, so a text family that happens to hold
an Emoji symbol (Noto Sans CJK holding `⚠`) no longer mixes monochrome and color
Emoji. The choice costs no layout work: on the same Intel Arc/Vulkan host, five
alternating release process pairs against `d258065`, with 30 full and 30 cached
iterations each, left the first-open `layout_ms` median unchanged (8.39 → 8.41 ms
and 8.25 → 8.22 ms in two runs) and moved full P50 by -1.81 % and -0.32 %. The
first-open GPU stage rose by about 1.2 ms in both runs, consistent with the
fixture rasterizing more color glyphs now that `⚠️` takes the color face, and
RSS fell about 3.4 %.

First-open total (+7.2 % and +7.5 %) and full P95 tripped the 5 % threshold in
one or both runs, and cached P95 in both. A same-binary noise-floor pair tripped
the same P95 metrics (+35.5 % and +52.4 %) and showed a 1.9 % first-open gap, so
those tails are not attributable to this change. Raw reports and metadata are in
`artifacts/emoji-order-repro/`; their `content_hash` matches the pre-change
reports because the document text is unchanged.

## Security-hardening change

The shared `Limits` budgets, relative-only image paths, the single link policy,
and the remote-image cap were compared against the preserved release binary of
`18158e1` on the same Intel Arc/Vulkan host, with the `performance` power
profile and default affinity. The change is deliberately budget-only: the
defaults are set so no ordinary document reaches them.

Twenty-six offscreen PNG pairs were byte-identical: every 10 KiB fixture at
default, dark, scale 2 and scrolled 1800 px, plus `welcome.md` and the image
example. No accepted document renders differently.

Four independent acceptance runs on the six standard fixtures — 5, 5, 5 and 15
process groups, 100 full and 100 cached iterations each — passed 43 or 44 of 48
metrics every time. Every flagged metric was a tail: full-pipeline P95 or
cached-refresh P95. The flagged set barely overlapped between runs, and one run
reported long-code-10k cached-refresh P95 at −48% while another reported
code-10k at +38%; a security change cannot produce either. First open,
full-pipeline P50, cached-refresh P50, RSS, peak RSS and tracked GPU bytes
passed in every run, with cached-refresh P50 within 0.4% of the baseline. This
is the unresolved small-file tail noise recorded above, now reproduced four
times, and no code path in the change is on the cached-refresh path. An earlier
45-group analysis of the immediately preceding build reached the same
conclusion.

One run was discarded: it was started while GPU tests and offscreen renders were
running on the same host, and it flagged a different set of tails again. Tail
comparisons require an idle machine.

The native watch smoke test measured 34.90 ms P50 and 59.78 ms P95 against
35.28 ms and 61.18 ms for the baseline. All ignored GPU tests pass, including a
new frame that renders the remote-image notice strip and the local-file
confirmation in both themes. Raw reports are under
`artifacts/security-hardening/` (ignored by Git).

## What can change the result

Color Emoji preserve their RGBA pixels through the image pipeline. The renderer
allocates a separate 512 × 512 sRGB atlas (1 MiB) only when a visible color glyph
is encountered; ordinary text continues to use the 4 MiB mask atlas. Both atlases
are bounded and reset together before rebuilding a frame when full. Tracked GPU
resource totals include the color atlas. This avoids losing interior details
(such as a white check on a green square) by reducing a color glyph to alpha only.

Font discovery and glyph coverage, language shaping, DPI, GPU backend, driver state, image dimensions, long unbreakable runs, and table or formula complexity all affect memory and time. The ordinary-document memory target is an optimization target, not a hard limit for arbitrary input.

Windows and macOS compile checks do not establish native runtime or performance behavior. When a performance-sensitive change is made, compare like-for-like fixtures and report the machine, backend, fonts, build profile, and whether the cache was warm.

Remote-image timing also depends on the on-disk image cache: a fresh entry avoids the request entirely, a stale one pays a conditional round trip, and only a miss downloads a body. The fixtures here reference local files, so their numbers are unaffected by cache state.
