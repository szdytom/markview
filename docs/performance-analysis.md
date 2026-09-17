# Latency and memory analysis

This page is a diagnostic analysis, not a guarantee or a change log. It answers
three questions for the reader's own targets, locates the code responsible, and
ranks the optimization points. The measured facts are recorded here; the
repeatable commands and the environment they were taken in are below.

| Metric | Target | Current |
| --- | --- | --- |
| First readable frame | 50 ms for 10 KiB, 100 ms for 100 KiB, size-independent upper bound | **~101 ms for 10 KiB, ~104 ms for 100 KiB, ~189 ms for 1 MiB** (native); a fixed ~100 ms prologue dominates |
| Edit → updated frame | < 50 ms, size-independent | ~41–43 ms up to ~400 KiB, but ~30 ms of that is the watch debounce; **~144 ms at 1 MiB** because the whole file is re-parsed |
| Edit → complete re-layout | bounded, no size dependence | **O(document) beyond 256 unique blocks**: 39 ms / 100 KiB, 308 ms / 400 KiB, ~70 ms and never any reuse for >256 code blocks |
| RSS | 10 KiB < 80 MiB, 100 KiB < 100 MiB, linear | **43.9 MiB / 64.0 MiB**, roughly linear in glyphs; no leak found (plateaus) |

Two facts explain most of the result:

1. **A ~100 ms fixed prologue runs before anything is on screen.** It is two
   full system-font scans, renderer initialization, window creation and
   compositor handoff. Document content is almost free for the first frame
   (layout to the viewport is 5–11 ms), except that the whole file is parsed
   first, which adds ~100 ms per MiB.
2. **The layout block cache is capped at 256 entries and indexed by content.**
   The repository fixtures repeat a few paragraph bodies, so a handful of cache
   entries cover every block and the measured edit cost looks tiny (3 ms). Give
   every block distinct text and the same 100 KiB document costs 39 ms to
   re-layout after a one-character edit, and 400 KiB costs 308 ms: the cap makes
   edit cost O(document size) again.

This page uses **first frame** for process entry to the first GPU-completed
frame whose geometry covers the viewport, and **edit** for a small on-disk
change to an already-loaded document. Edit timings are reported from the moment
the write returns unless the native end-to-end number is named; the reader's
file-watch debounce (`QUIET = 30 ms`, `MAX_WAIT = 100 ms`, `src/watch.rs`)
adds a fixed ~30 ms to the interactive path.

## How it was measured

Linux x86_64, Intel Core Ultra 5 125H with Intel Arc (MTL) through Vulkan, Rust
1.96.0-nightly, release profile (thin LTO, one codegen unit), AC power,
`balanced` power profile. The native window ran at DPR 2 (1200 × 800 logical,
2400 × 1600 physical); the offscreen harness used 1200 × 800 at scale 1, a
760 px column and 18 px text. Timings below are medians of 2–5 independent
processes; edit distributions are medians over 8–50 edits per process.

```sh
# First frame, edit-to-frame, edit-to-complete and per-edit RSS, one process:
target/release/markview --bench-latency FILE [--iterations N]

# The same aggregated over independent processes into a table:
python3 scripts/generate_stress_fixtures.py
python3 scripts/bench_latency.py target/release/markview \
  tests/fixtures/ordinary-10k.md tests/fixtures/text-cjk-100k.md \
  artifacts/perf-analysis/unique-100k.md \
  artifacts/perf-analysis/unique-400k.md \
  artifacts/perf-analysis/many-code-100k.md \
  artifacts/perf-analysis/text-cjk-1000k.md

# Native end-to-end edit latency, including the debounce and event loop:
python3 scripts/smoke_watch.py target/release/markview \
  --fixture artifacts/perf-analysis/unique-100k.md --edit top --skip-stream
```

`--bench-latency` drives the real `Worker`, progressive prefix publication,
block cache, image scheduler and GPU renderer; only the presentation target is
offscreen, so window creation, chrome shaping and compositor presentation are
excluded. `--smoke-test` reports the true process-entry first frame since this
analysis (`src/app/painting.rs`), and `scripts/smoke_watch.py` now accepts
`--fixture`/`--edit` for large-document end-to-end edit timing.

## Metric 1: first readable frame

Native `--smoke-test`, process entry to the first readable GPU frame:

| Fixture | Bytes | Blocks | First frame (ms) |
| --- | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 59 | 101 |
| unique-100k | 100 KiB | 438 | 104 |
| unique-400k | 400 KiB | 1747 | 95 |
| text-cjk-1000k | 1 MiB | 5871 | 189 |

Offscreen, with the stage split:

| Fixture | Bytes | Init (ms) | Parse (ms) | First-prefix layout (ms) | First frame (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 23.7 | 0.2 | 17.7 (whole document) | 75.5 |
| text-cjk-100k | 100 KiB | 28.4 | 2.7 | 9.5 | 72.4 |
| math-cjk-100k | 100 KiB | 28.1 | 2.6 | 11.1 | 74.1 |
| unique-100k | 100 KiB | 24.1 | 0.6 | 6.0 | 60.9 |
| many-blocks-100k | 287 KiB | 24.9 | 4.9 | 5.6 | 61.2 |
| unique-400k | 400 KiB | 22.5 | 1.9 | 6.0 | 57.4 |
| text-cjk-1000k | 1 MiB | 22.6 | 103.9 | 11.2 | 170.6 |

The document-dependent part of the first frame is small and bounded: the worker
publishes a prefix covering the viewport (plus half a viewport) after laying out
~11–16 blocks in 5–11 ms, and rendering is viewport-culled
(`crates/markview-render/src/frame.rs:34-46`). The target is missed because of
the fixed prologue, and the "size-independent upper bound" only breaks once the
whole-file parse adds up, at roughly 100 ms per MiB.

### Where the ~100 ms goes

| Stage | Evidence | Cost | Size-dependent |
| --- | --- | ---: | --- |
| Process entry → app entry, dominated by **UI `TextShaper::new()` = full fontconfig scan** | `src/app.rs:96`, `crates/markview-core/src/shaping.rs:181` | ~25 ms (22.5 ms scan) | no |
| Worker `LayoutEngine::new()` = **second full font scan**, before it accepts the first request | `src/worker.rs:128` → `crates/markview-core/src/layout.rs:177` | ~20–30 ms | no |
| `Renderer::new`: instance, adapter, device, shader module, **two** pipelines, 4 MiB mask atlas, buffers | `src/app/painting.rs:126`, `crates/markview-render/src/gpu.rs:61`, `pipeline.rs:2`, `raster.rs:83` | ~30 ms | no |
| Window creation, chrome overlay shaping, acquire/present, DPR-2 compositor handoff | `src/app/lifecycle.rs:33`, `src/app/painting.rs:25`, `frame.rs:186` | ~20–30 ms | no |
| Whole-file parse before the first prefix | `src/worker.rs:187`, `crates/markview-core/src/document/parse.rs:348` | 0.2 ms / 2.7 ms / 104 ms (10 KiB / 100 KiB / 1 MiB) | **yes** |
| Layout to coverage | `crates/markview-core/src/layout.rs:266` | 5–11 ms | viewport-bounded |

The two font scans are redundant and run once each: `FontContext::new()` builds
a fontique collection with system fonts, which enumerates every installed family
(~1287 here) and runs several generic-family sorts. The UI copy is needed only
when the first overlay is drawn; the worker copy is needed before block 0.
`Renderer.fallback` (`crates/markview-render/src/lib.rs:80`) stays `None` on a
normal launch, so there is no third scan.

Three whole-document passes precede the first laid-out block: `document::parse`,
`Images::prepare` (`src/worker.rs:213`) and `highlights.prepare`
(`crates/markview-core/src/layout.rs:235`, which clones every code block's
language and text into a job list). Only the block-layout walk is coverage-bound.

## Metric 2: edit latency

Native end-to-end write → GPU-completed frame of the refreshed layout, including
the file-watch debounce, event loop and render:

| Fixture | Bytes | P50 (ms) | P95 (ms) |
| --- | ---: | ---: | ---: |
| small synthetic | 40 B | 41.4 | 60.8 |
| unique-100k, top edit | 100 KiB | 43.2 | 43.7 |
| unique-400k, top edit | 400 KiB | 42.8 | 44.2 |
| text-cjk-1000k, top edit | 1 MiB | 143.8 | 167.5 |

Offscreen, from the write to the first refreshed frame (no debounce) and to the
complete re-layout:

| Fixture | Visible frame P50 (ms) | Complete P50 (ms) | Blocks reused at complete |
| --- | ---: | ---: | ---: |
| ordinary-10k | 0.85 | 0.85 | 58 / 59 |
| text-cjk-100k (repetitive) | 2.95 | 3.0 | 587 / 588 |
| math-cjk-100k (repetitive) | 2.85 | 2.8 | 711 / 712 |
| unique-100k | 2.00 | 38.7 | 255 / 438 |
| unique-400k | 3.61 | 308.2 | 255 / 1747 |
| many-blocks-100k | 5.70 | 252.5 | 255 / 4000 |
| many-code-100k (300 fenced blocks) | 7.81 | 70.5 | **0 / 599** |
| text-cjk-1000k | 103.5 | 132.5 | 4582 / 5871 |

The visible frame is small because the worker only re-lays out the blocks needed
to cover the viewport, and the changed block plus its neighbours are usually
retained. The **complete** re-layout is where the size dependence lives.

### The block cache cap is the central problem

`LayoutEngine` keeps `HashMap<CacheKey, Arc<BlockLayout>>` but, on each pass,
takes the old map and re-inserts at most the first 256 entries in document order,
and only while cumulative draws stay under 100 000:

```rust
// crates/markview-core/src/layout.rs:251, 323-326
let previous = std::mem::take(&mut self.cache);
...
cached_draws += layout.draws.len();
if cached_draws < 100_000 && self.cache.len() < 256 {
    self.cache.insert(key, layout);
}
```

`CacheKey.content` is the semantic `content_key`, so the cap only bites when
blocks have distinct content. The repository fixtures (`text-cjk-100k.md` and
`math-cjk-100k.md`) repeat 3–5 paragraph bodies
(`scripts/generate_large_fixture.py`), so ~4 cache entries cover all 588 blocks
and the cap is invisible. With `unique-100k.md`, 183 of 438 blocks are recomputed
on every edit; with `unique-400k.md`, 1492 of 1747. That is the 39 ms → 308 ms
scaling. `scripts/generate_stress_fixtures.py` writes the unique-content
fixtures so this stays measurable.

Three more defects sit on the same code:

- **A single oversized block is never cached.** `cached_draws` is tested after
  adding the current block's draws, so a block with ≥ 100 000 draws is never
  inserted. A 100 KiB document that is one list or one fence re-lays out 100 % of
  itself on every edit.
- **Cancellation truncates the cache.** On supersede, `layout_progressive`
  returns before the loop finishes (`layout.rs:270-272`), and the old map was
  already taken, so only the pre-cancellation entries are re-inserted. Under
  rapid edits the cache shrinks toward the per-pass block count, which is exactly
  the interactive workload.
- **Position-biased retention.** Because only the first 256 keys in document
  order survive, an edit deep in the document gets no benefit from the cap even
  when the surrounding blocks were laid out.

### The highlight cache cliff

`Highlights::poll` clears its whole cache when it reaches 256 entries
(`crates/markview-core/src/layout/highlights.rs:157-159`), and any arriving
highlight result bumps a global generation, which makes `poll_highlights` clear
the entire block cache (`layout.rs:346-352`). With ≥ 257 code blocks this is a
permanent loop: results arrive → cache clears → `prepare` re-enqueues every code
block → results arrive. Measured with 300 fenced blocks: `reused = 0` on every
edit and a 70 ms full re-layout every time, forever. The block cache never
survives a single pass.

### Whole-file re-parse sets the 1 MiB floor

`Event::Changed` bumps `content_version`, which invalidates the worker's cached
document, so every edit does `read_document` + `document::parse` over the whole
file (`src/worker.rs:175-201`). `parse` runs comrak, builds a line index and a
footnote map, then computes a source-range hash and a semantic hash for every
block (`crates/markview-core/src/document/parse.rs:332-336`). Measured parse:
0.2 ms / 10 KiB, 2.7 ms / 100 KiB, 104 ms / 1 MiB. That 104 ms is why the 1 MiB
edit takes 144 ms end to end and why the first frame grows with size.

### The debounce is a fixed floor

`QUIET = 30 ms` (`src/watch.rs:17-18`) means no edit can be shown sooner than
~30 ms after the write, before any worker or render cost. The native numbers
above show it: a 40-byte document and a 400 KiB document both land at ~42 ms
because the debounce dominates and the document work is 1–5 ms. Any attempt to
meet a sub-50 ms edit target must either reduce the debounce or overlap it.

### Other O(document) work on the edit path

- **UI-thread counts.** On a complete update, `ReaderSession::accept` runs
  `extract_text` + `TextCounts::of` (ICU word segmentation over the whole
  document) on the winit event loop (`src/state.rs:458-475`,
  `crates/markview-core/src/text.rs:82-95,188-229`): ~3.8 ms / 100 KiB,
  ~11.4 ms / 300 KiB, on the thread that then has to render.
- **Bottom-follow disables partial publication.** Scrolled to the bottom,
  `App::request` sets `coverage = f32::INFINITY` (`src/app/document.rs:10-20`).
  `PrefixPublication::publish` then needs `height >= INFINITY`, which never
  holds for a ≥ 32 KiB document, so no prefix is ever published and the reader
  waits for the complete re-layout (`src/worker.rs:78-89`). Watching a log at
  the bottom pays the full O(document) cost before the edit appears.
- **Per-block and per-prefix overhead.** Every pass builds a `CacheKey` per
  block including `codeblock_theme.clone()` — one `String` per block per pass
  (`layout.rs:287`) — and walks each block's subtree through `image_key`
  (`layout.rs:49-66`). Published prefixes clone the `LayoutSnapshot` block vector
  and the `ImageSnapshot` entry map (`src/worker.rs:242-256`). Each is small
  alone but all are O(blocks) on a localized edit.

## Metric 3: memory

RSS after scrolling through the document (offscreen; the native window adds a
few MiB for the surface and chrome):

| Fixture | Bytes | Blocks | RSS (MiB) | Peak (MiB) | Text index (MiB) |
| --- | ---: | ---: | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 59 | 43.9 | 43.7 | 0.7 |
| unique-100k | 100 KiB | 438 | 59.1 | 59.1 | 6.2 |
| text-cjk-100k | 100 KiB | 588 | 64.0 | 63.7 | 7.4 |
| math-cjk-100k | 100 KiB | 712 | 64.3 | 64.1 | 6.4 |
| unique-400k | 400 KiB | 1747 | 160.2 | 160.2 | 24.9 |
| many-blocks-100k | 287 KiB | 4000 | 182.0 | 182.0 | 29.0 |
| text-cjk-1000k | 1 MiB | 5871 | 255.3 | 258.3 | 74.5 |

Against the stated targets, 10 KiB is 43.9 < 80 MiB and 100 KiB is 59–64 <
100 MiB, both with headroom. Growth is close to linear in glyph count: the
marginal cost is ~350 bytes per source character of prose (unique-100k to
unique-400k adds 101 MiB for 307 KiB), and `VmHWM == VmRSS` in nearly every run
(the process never returns memory to the OS).

**No leak was found.** RSS plateaus immediately and then stays flat across
repeated reloads: unique-400k held 160.4 MiB over 19 edits, many-blocks
177.7 → 181.9 MiB over the first three edits then flat over 27, and the 1 MiB
fixture oscillated between 248 and 258 MiB with no trend. A separate churn
through 13 distinct documents also plateaued (78.5 MiB then flat).

What dominates the bytes:

- **Per-glyph layout representation.** `Draw` is 96 bytes and there is one
  `Draw::Glyph` per glyph, plus a 48-byte `TextCluster` per cluster, plus each
  node's reading-text `String` and grapheme boundaries
  (`crates/markview-core/src/scene.rs:75-106,160-184`,
  `crates/markview-core/src/text.rs:137-184`). For unique prose that is the
  ~350 B/char slope. This is the dominant per-document cost.
- **Allocator high-water from re-layout.** Re-laying out an uncached block builds
  a fresh `BlockLayout` while the previous snapshot still holds the old one, so
  peak retention is roughly twice the steady state, and glibc keeps the arenas.
  Many-blocks peaks at 182 MiB for a 287 KiB file almost entirely this way. There
  is no `malloc_trim` anywhere, so the benchmark's RSS reflects the high-water,
  not the live set.
- **Fixed process floor plus tracked GPU.** Offscreen GPU resources are 7.9 MiB
  (4 MiB mask atlas + 3.84 MiB target + 256 KiB geometry), and wgpu/Vulkan
  driver, font blobs, syntect and ICU data make up the ~25–35 MiB baseline. The
  color atlas (1 MiB) is lazy.

Bounds worth knowing, with their cap:

| Cache | Cap | Eviction |
| --- | --- | --- |
| `LayoutEngine.cache` | 256 keys ∧ 100 k draws | per pass, position-biased (`layout.rs:324`) |
| `Highlights.highlight_cache` | 256 entries | clear-all, no byte cap (`highlights.rs:157`) |
| `MathEngine.cache` | 256 entries | clear-all (`math.rs:41`) |
| `TextShaper.faces` / `font_sets` | appearance-driven | cleared every pass (`shaping.rs:195-201`) |
| GPU image textures | 256 MiB | only on the next insert (`render/images.rs:97-108`) |
| CPU decoded pixels | 256 MiB | only on the next insert (`src/images/cache.rs`) |
| Raster atlases | 4 MiB mask + 1 MiB lazy color | reset, not freed (`raster.rs`) |
| Geometry vertices | viewport-bounded | cleared per frame (`frame.rs:34-46`) |
| Tabs | count unbounded; inactive released after 20 s | `src/app/tabs.rs:11,176` |

Two retention points are worth fixing regardless of the targets: `Worker::cancel`
does not clear `cached`/`last`, so closing the last tab leaves the parsed
document and its options alive (`src/worker.rs:299-303`), and the highlight and
math caches survive a document change because they are only cleared by count.

## Optimization points, ranked

Impact is an estimate of the change against the metric it names; risk is the
chance of an output or behavior change.

### First frame

1. **One font context per process** — drop the second full scan (UI or worker).
   Make `App.ui` lazy (it is first used when the first overlay is drawn) or hand
   the worker's ready `TextShaper` back through the event proxy. ~20–30 ms,
   low risk. (`src/app.rs:96`, `src/worker.rs:128`, `layout.rs:177`.)
2. **Partial font discovery** — build the collection with system fonts disabled
   and register the families the stylesheet actually names, instead of
   enumerating all ~1287 families. ~15–20 ms, medium risk (unlisted scripts lose
   system fallback; verify against `fontdefs`). (`shaping.rs:220-337`.)
3. **Overlap or shrink `Renderer::new`** — create instance/adapter/device on a
   background thread while the font scan runs, and defer the image pipeline and
   the 4 MiB atlas until first use. ~15–25 ms, medium risk.
   (`pipeline.rs:21`, `raster.rs:83`.)
4. **Prefix-aware parse** for the size-independent upper bound — stop
   materializing blocks once the viewport is covered, or reuse block identity
   across parses. ~100 ms/MiB at the first frame, high implementation risk
   (headings, footnotes and anchors need the whole document).
5. **Skip the empty "Opening document…" frame** (`src/app/document.rs:25`) to
   avoid one overlay build and submit.

### Edit

6. **Replace the 256/100 k cap with a persistent, byte-budgeted, viewport-aware
   cache.** Keep the map across passes and cancellations, and evict by distance
   from the viewport rather than document position. This is the single largest
   win: it removes the O(document) term (308 ms → ~one block at 400 KiB) and
   makes rapid edits stop shrinking the cache. Medium risk.
   (`layout.rs:251,323-326`.)
7. **Fix the highlight cliff.** Evict highlight entries by LRU instead of
   `clear()`, and key each block on its own highlight version instead of bumping
   a global generation that clears the block cache. Removes the permanent
   re-layout for >256 code blocks. Low risk. (`highlights.rs:157-159`,
   `layout.rs:346-352`.)
8. **Reduce the debounce.** 30 ms is a third of the visible budget and is paid by
   every save; a shorter quiet window with the existing 100 ms ceiling, or
   coalescing that starts layout on the first event, would recover most of it.
   Low risk. (`src/watch.rs:17-18`.)
9. **Skip read + parse when bytes are unchanged**, and memoize `semantic_key`
   across parses. Helps the metadata-only reload case and the 1 MiB parse floor.
   Medium risk. (`src/worker.rs:175-201`, `parse.rs:332-336`.)
10. **Move `TextCounts::of` off the UI thread or memoize it by `content_id`.**
    ~4 ms / 100 KiB and ~40 ms / MiB of event-loop stall. Low risk.
    (`src/state.rs:458-475`.)
11. **Fix bottom-follow.** Use a finite sentinel and publish a prefix that covers
    the tail instead of `f32::INFINITY`. Medium risk. (`src/app/document.rs:10-20`,
    `src/worker.rs:78-89`.)
12. **Cache a single oversized block, and reuse sub-block geometry** for large
    lists and code blocks. Medium risk. (`layout.rs:324`.)

### Layout throughput

13. **Stop clearing the font caches on an unchanged stylesheet.**
    `set_stylesheet` runs every pass and clears `faces`/`font_sets`; an
    `Arc::ptr_eq`/`layout_key` guard keeps them. Low risk.
    (`shaping.rs:195-201`, `layout.rs:220`.)
14. **Shape each paragraph once.** The paragraph is shaped whole for measurement
    and then again per line, plus a `"-"` shape and a re-shape and full tail
    re-solve per backed-up line; shaping is ~60 % of layout. Reuse whole-paragraph
    clusters for lines that are not adjusted, and re-solve the tail lazily.
    Medium–high risk. (`inline.rs:171-177,342-345`, `paragraph.rs:140-148,157-225`.)
15. **Remove the per-cluster O(spans) scans.** `shape` calls
    `spans.iter().position(...)` for every grapheme part, and `paragraph.rs`
    scans spans and the mapping per drawn cluster. The spans and mapping are
    ordered; a cursor or `partition_point` makes them linear. Low risk.
    (`shaping.rs:421-438`, `paragraph.rs:298,330-337,379-383`, `mapping.rs:22-47`.)
16. **Intern the per-block cache inputs.** `codeblock_theme.clone()` allocates a
    `String` per block per pass, and `image_key` walks and hashes each block's
    subtree even on a hit. Precompute a per-block image fingerprint and an
    interned theme id. Low risk. (`layout.rs:49-66,273-290`.)
17. **Share `ImageSnapshot.entries` behind an `Arc`** instead of deep-cloning it
    once per pass and once per published prefix. Low risk. (`layout.rs:224`,
    `src/worker.rs:242-256`.)

### Memory

18. **Compact the glyph representation.** `Draw` at 96 bytes plus a 48-byte
    `TextCluster` per glyph is the memory slope; a structure-of-arrays or a
    smaller glyph record, and `shrink_to_fit` on the final snapshot, would cut
    the 350 B/char materially. Medium risk. (`scene.rs:75-106`, `paragraph.rs:294-461`.)
19. **Return freed memory after a document change** — `malloc_trim(0)` on Linux
    after publishing, or a decaying allocator — to remove the ~2× peak retention
    that currently shows up as steady-state RSS. Low risk.
20. **Clear the worker's cached document on cancel**, cap the tab count, and
    byte-bound the highlight and math caches. Low risk.
    (`src/worker.rs:299-303`, `src/app/tabs.rs:11`, `highlights.rs:23`, `math.rs:16`.)
21. **Evict GPU and CPU images by visibility** rather than only on the next
    insert, so a document switch or tab close releases up to 256 MiB each.
    Medium risk. (`render/images.rs:97-108`, `src/images/cache.rs`.)

The first five items are where the 50 ms first-frame target lives; items 6–8 are
where the edit targets live. Items 18–19 are what keeps a growing document inside
the memory targets.

## Measurement caveats

- The native numbers include window creation and compositor handoff at DPR 2;
  the offscreen numbers exclude them, which is why the same fixture can read
  61 ms offscreen and 101 ms natively.
- The `balanced` power profile was left in place. On this host `power-saver` has
  historically run 1.6–2× slower in every stage, so re-run acceptance with the
  `performance` profile.
- `--bench`'s `reading_text_index_bytes` samples the cached-refresh snapshot.
  Because the block cache is content-keyed and the shipped fixtures repeat, it
  reports 63–246 KB for a 100 KiB file where a fresh engine retains 7–9 MiB.
  Do not use it as the layout-retention metric; `--bench-latency` reports the
  cold snapshot's index instead.
- Each offscreen run is a fresh process with a cold document and font cache, but
  the OS file cache and GPU driver state persist across runs, so the first run of
  a session reads higher than the reported median. The worker's font scan is on
  the critical path and is included in the first-frame number.
- Raw reports for every table are under `artifacts/perf-analysis/` (ignored by
  Git), together with the preserved release binary of the analyzed revision.
