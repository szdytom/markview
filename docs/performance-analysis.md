# Latency and memory analysis

This page is a diagnostic analysis, not a guarantee or a change log. It answers
three questions for the reader's own targets, locates the code responsible, and
ranks the optimization points. The measured facts are recorded here; the
repeatable commands and the environment they were taken in are below.

| Metric | Target | Current |
| --- | --- | --- |
| First readable frame | 50 ms for 10 KiB, 70 ms for 100 KiB excluding process startup, size-independent upper bound | **~41–57 ms from 10 KiB to 1 MiB** (offscreen); ~72–86 ms natively through process startup, and flat to 4 MiB. A fresh file is parsed only as far as its opening viewport needs, with definitions that follow still resolved. |
| Edit → updated frame | ≤ 30 ms for 100 KiB, < 50 ms for 1 MiB, size-independent | **1.3 ms / 100 KiB and 6.4 ms / 1 MiB** visible frame offscreen; ~18 ms and ~23 ms end to end natively. A small edit to a plain-text document re-parses only the block it changed. |
| Edit → complete re-layout | bounded, no size dependence | **one changed block**: 1.7 ms / 100 KiB, 3.2 ms / 400 KiB, 5.4 ms / 287 KiB of 4000 blocks; the cache retains every block until the next document or option set |
| RSS | 10 KiB < 50 MiB, 100 KiB < 80 MiB, bounded for large documents | **42.2 MiB / 52.7 MiB**; a 1 MiB document is 65 MiB; growth is linear in glyphs and no leak was found (plateaus) |

Three facts explain most of the result:

1. **A fixed prologue runs before anything is on screen.** Font discovery (shared
   across every shaper and overlapped with renderer initialization), renderer
   initialization, window creation and compositor handoff are 50–80 ms
   together, depending on how much of the work overlaps. Document content is
   small for the first frame (layout to the viewport is 6–10 ms).
2. **A fresh document is parsed only as far as its opening viewport needs.**
   The worker parses a bounded prefix, lays out just enough of it to cover the
   viewport, and publishes that, so the first frame no longer waits for the
   whole-file parse. Reference definitions and footnotes that follow the cut
   are appended to the prefix parse, so links and notes in the first frame
   already resolve; the complete parse follows.
3. **The layout block cache persists across passes.** It keeps one entry per
   block, keyed on the block's content plus the images and syntax colors it
   uses, and drops every entry a complete pass did not touch. A localized edit
   therefore re-lays out only the changed block, and a syntax color arriving
   invalidates only the code blocks that gained it.

This page uses **first frame** for process entry to the first GPU-completed
frame whose geometry covers the viewport, and **edit** for a small on-disk
change to an already-loaded document. Edit timings are reported from the moment
the write returns unless the native end-to-end number is named; the reader's
file-watch debounce (`QUIET = 10 ms`, `MAX_WAIT = 40 ms`, `src/watch.rs`)
adds ~10 ms to the interactive path.

## How it was measured

Linux x86_64, Intel Core Ultra 5 125H with Intel Arc (MTL) through Vulkan, Rust
1.96.0-nightly, release profile (thin LTO, one codegen unit), AC power,
`balanced` power profile. The native window ran at DPR 2 (1200 × 800 logical,
2400 × 1600 physical); the offscreen harness used 1200 × 800 at scale 1, a
760 px column and 18 px text. Timings below are medians of 2–5 independent
processes; edit distributions are medians over 8–50 edits per process.

```sh
# First frame, edit-to-frame, edit-to-complete and per-edit RSS, one process:
target/release/markview latency FILE [--iterations N]

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

`latency` drives the real `Worker`, progressive prefix publication,
block cache, image scheduler and GPU renderer; only the presentation target is
offscreen, so window creation, chrome shaping and compositor presentation are
excluded. `smoke-test` reports the true process-entry first frame since this
analysis (`src/app/painting.rs`), and `scripts/smoke_watch.py` now accepts
`--fixture`/`--edit` for large-document end-to-end edit timing.

## Metric 1: first readable frame

Native `smoke-test`, DPR 2, alternating processes per fixture:

| Fixture | Bytes | Process entry (ms) | Document open (ms) |
| --- | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 74.3 | 42 |
| unique-100k | 100 KiB | 72.0 | 38 |
| text-cjk-1000k | 1 MiB | 77.1 | 42 |
| 4 × text-cjk-1000k with footnotes | 4 MiB | 83.5 | 42 |

`Process entry` is the whole cold start, which the 80 ms / 100 ms targets bound;
`Document open` is the same process opening the document, which is what the
50 ms / 70 ms targets name and what a `Ctrl+O` in a running reader costs. Both
are inside their targets, and neither grows with the file: the 4 MiB row costs
about what the 100 KiB one does. Eight alternating processes per row. The 10 KiB
cold start is window-bound and sits at the target — the spread across sessions
is 70–94 ms depending on host load.

Offscreen `latency`, median cold first frame:

| Fixture | Bytes | Init (ms) | Parse (ms) | First-prefix layout (ms) | First frame (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 29.5 | 0.32 | 12.6 (whole document) | 52.6 |
| text-cjk-100k | 100 KiB | 29.3 | 1.13 | 10.6 | 51.2 |
| unique-100k | 99 KiB | 32.6 | 0.44 | 5.9 | 46.2 |
| unique-400k | 399 KiB | 28.1 | 0.40 | 6.0 | 41.4 |
| many-code-100k | 105 KiB | 29.2 | 0.81 | 12.2 | 48.7 |
| many-blocks-100k | 280 KiB | 30.6 | 1.94 | 9.8 | 56.6 |
| text-cjk-1000k | 1000 KiB | 22.1 | 1.80 | 12.5 | 55.8 |

The document-dependent part of the first frame is small and **bounded**: a fresh
file is parsed only as far as `PREFIX_BYTES` (64 KiB), the worker lays out just
enough of that prefix to cover the viewport (6–25 blocks in 6–13 ms), and
rendering is viewport-culled (`crates/markview-render/src/frame.rs:34-46`). The
parse column is that prefix parse, so it no longer grows with the file. When the
prefix names a reference or note, the worker scans the file for the definitions
and appends them, which costs ~0.5 ms/MiB and still leaves a 4 MiB file with
footnotes at ~2 ms of parse and a ~70 ms first frame. This session ran warmer
than the earlier ones — `Init` is
5–12 ms higher — which is why the small fixtures read ~45 ms here.

At 10 KiB the native cold start is **window-bound, not document-bound**. A phase
trace puts the worker's font scan and first publication at ~40 ms and
`Renderer::new` at ~30 ms, while the event loop does not start until ~50 ms and
the first redraw is not serviced until ~60 ms; the frame lands at ~64–67 ms.
Submitting the open before GPU initialization, so the two overlap, changed
nothing measurable for the same reason. The remaining levers are the ~25 ms
`Gpu::new` (adapter, device, surface) and the compositor's mapping and redraw
latency, neither of which moves for document work.

The tables below are the earlier analysis that introduced the shared font
collection and progressive publication; they are kept for the reasoning, not as
current numbers.

Native `smoke-test`, process entry, before and after sharing the font
collection:

| Fixture | Bytes | Blocks | Before (ms) | After font sharing (ms) |
| --- | ---: | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 59 | 94.2 | **82.3** |
| unique-100k | 100 KiB | 438 | 93.7 | **79.3** |

Sharing the font collection removed about 12–14 ms, because the main thread no
longer scans the system fonts before the window is created and the worker no
longer scans a second time. The remaining prologue is the renderer, window and
compositor cost.

Offscreen, before the sharing change, with the stage split:

| Fixture | Bytes | Init (ms) | Parse (ms) | First-prefix layout (ms) | First frame (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 23.7 | 0.2 | 17.7 (whole document) | 75.5 |
| text-cjk-100k | 100 KiB | 28.4 | 2.7 | 9.5 | 72.4 |
| math-cjk-100k | 100 KiB | 28.1 | 2.6 | 11.1 | 74.1 |
| unique-100k | 100 KiB | 24.1 | 0.6 | 6.0 | 60.9 |
| many-blocks-100k | 287 KiB | 24.9 | 4.9 | 5.6 | 61.2 |
| unique-400k | 400 KiB | 22.5 | 1.9 | 6.0 | 57.4 |
| text-cjk-1000k | 1 MiB | 22.6 | 103.9 | 11.2 | 170.6 |

The offscreen harness spawns the worker after the renderer, so it used to pay
the worker's scan serially. With the shared collection and the worker started
first, as the window does, its first frame fell to 51.0 ms (ordinary-10k),
38.6 ms (unique-100k) and 156.2 ms (text-cjk-1000k).

### Where the fixed prologue goes

| Stage | Evidence | Cost | Size-dependent |
| --- | --- | ---: | --- |
| System font discovery, shared and started on the worker thread so it overlaps the renderer | `crates/markview-core/src/shaping.rs` `system_fonts`, `src/worker.rs` `warm_system_fonts` | ~20 ms, off the main thread | no |
| `Renderer::new`: instance, adapter, device, shader module, **two** pipelines, 4 MiB mask atlas, buffers | `src/app/painting.rs:126`, `crates/markview-render/src/gpu.rs:61`, `pipeline.rs:2`, `raster.rs:83` | ~30 ms | no |
| Window creation, chrome overlay shaping, acquire/present, DPR-2 compositor handoff | `src/app/lifecycle.rs:33`, `src/app/painting.rs:25`, `frame.rs:186` | ~20–30 ms | no |
| Bounded prefix parse before the first prefix | `src/worker.rs`, `crates/markview-core/src/document/incremental.rs` | 0.2 ms / 1.1 ms / 1.9 ms (10 KiB / 100 KiB / 1 MiB) | no (64 KiB cap) |
| Layout to coverage | `crates/markview-core/src/layout.rs` | 6–11 ms | viewport-bounded |

`FontContext::new()` builds a fontique collection with system fonts, which
enumerates every installed family (~1287 here). It used to run twice, once in
`App::new` on the main thread and once in the worker; now it runs once on the
worker and every other shaper clones the `Arc`-backed collection. The UI shaper
is created on the first overlay, which is already past the window, and
`Renderer.fallback` (`crates/markview-render/src/lib.rs:80`) stays `None` on a
normal launch, so no third scan is possible.

Three whole-document passes precede the first laid-out block: `document::parse`,
`Images::prepare` (`src/worker.rs`) and `highlights.prepare`
(`crates/markview-core/src/layout/highlights.rs`, which walks the block tree for
code keys and clones the language and text of the jobs it still has to run).
Only the block-layout walk is coverage-bound.

## Metric 2: edit latency

Native end-to-end write → GPU-completed frame of the refreshed layout, including
the file-watch debounce, event loop and render:

| Fixture | Bytes | P50 (ms) | P95 (ms) |
| --- | ---: | ---: | ---: |
| small synthetic | 40 B | 18.0 | 19.6 |
| unique-100k, top edit | 100 KiB | 18.1 | 22.8 |
| text-cjk-1000k, top edit | 1 MiB | 22.6 | 25.0 |

Offscreen, from the write to the first refreshed frame (no debounce) and to the
complete re-layout:

| Fixture | Visible frame P50 (ms) | Complete P50 (ms) | Complete geometry (ms) | Blocks reused at complete |
| --- | ---: | ---: | ---: | ---: |
| ordinary-10k | 0.91 | 0.92 | 0.9 | 58 / 59 |
| text-cjk-100k (repetitive) | 2.40 | 2.40 | 2.4 | 587 / 588 |
| unique-100k | 1.45 | 2.79 | 0.45 | 437 / 438 |
| unique-400k | 2.01 | 9.45 | 0.84 | 1746 / 1747 |
| many-blocks-100k | 2.34 | 8.58 | 1.51 | 3999 / 4000 |
| many-code-100k (300 fenced blocks) | 2.08 | 3.73 | 0.96 | 597 / 600 |
| text-cjk-1000k | 6.17 | 28.26 | 1.53 | 5870 / 5871 |

The visible frame and the complete geometry now differ: the worker publishes the
prefix as soon as it covers the viewport, then finishes the layout. The
"complete" column also carries the reading counts for the footer, which cost
~0.2 ms / 10 KiB and ~21 ms / 1 MiB; they run on the worker, after the prefix,
so they no longer delay the frame.

### The block cache is retained per block

`LayoutEngine` keeps `HashMap<CacheKey, CacheEntry>`, where each entry holds the
`Arc<BlockLayout>` shared with the snapshot that published it and the number of
the pass that last used it. A pass never takes the map out, so a superseded pass
cannot lose entries, and at the end of a complete pass every entry it did not
touch is dropped:

```rust
// crates/markview-core/src/layout.rs
self.cache.retain(|_, entry| entry.pass == pass);
```

That leaves the cache at exactly one document's worth of geometry, which is the
same set of `Arc`s the snapshot already holds, so retaining it costs pointers
rather than copies. `CacheKey` is the block's semantic `content_key`, its layout
options, the stylesheet identity, the resolved syntax theme, and an `external`
fingerprint of the images it uses and which of its code blocks already carry
syntax colors.

The earlier cap was 256 entries and 100 000 cumulative draws, re-inserted in
document order on every pass. It made edit cost O(document) as soon as blocks
had distinct content: with `unique-100k.md`, 183 of 438 blocks were recomputed
on every edit, and with `unique-400k.md`, 1492 of 1747. It also never cached a
block with ≥ 100 000 draws, and cancellation truncated the retained prefix.
`scripts/generate_stress_fixtures.py` writes the unique-content fixtures that
keep this measurable.

### Syntax colors invalidate only their own blocks

A highlight result arrives asynchronously and used to bump a global generation,
which made every code block's cache key stale and cleared the whole layout
cache. `Highlights` also cleared its cache at 256 entries
(`crates/markview-core/src/layout/highlights.rs`), so a document with more code
blocks than that re-enqueued and re-laid out everything on every pass: measured
with 300 fenced blocks, `reused = 0` forever and a 70 ms full re-layout per
edit.

The `external` fingerprint now carries, per code block, the highlight key and
whether its result is present. A result that arrives flips that bit for the
blocks that use it and changes their key, so only those blocks are laid out
again; the rest of the cache is untouched. The highlight cache no longer clears
itself by count: `prepare` retains it on the code keys of the current document,
and the per-pass `highlight_bytes` budget bounds what is ever enqueued. The
same 300-block fixture now reuses 597 of 600 blocks on an edit.

### Bounded parses: one block on edit, one prefix on open

`Event::Changed` bumps `content_version`, which invalidates the worker's cached
document, so once every edit read the file and ran `document::parse` over the
whole of it: comrak, a line index, a footnote map, then a walk of the block tree
(`crates/markview-core/src/document/parse.rs`). That parse cost ~19 ms / MiB and
was ~90 % of a 1 MiB edit.

`document::parse_incremental` (`crates/markview-core/src/document/incremental.rs`)
now reuses the previous parse. It takes the common byte prefix and suffix of the
old and new source, finds the blank-line-delimited group the change falls in,
re-parses only that group with comrak, and keeps every block before it as-is and
every block after it with its source ranges shifted by the edit's length. Heading
anchors are re-assigned across the spliced list, because an added, renamed or
removed heading changes the suffix later headings with the same slug take.

The fast path only accepts documents whose top-level blocks are leaf blocks
separated by blank lines. Lists, quotes, footnotes, fenced, indented and HTML
code, tables and reference definitions can span a blank line or carry meaning
outside the block that holds them, so a document containing any of them falls
back to a full parse. A randomized 500-edit test compares the result against a
full parse block for block. The parse fell from ~19 ms to ~3.4 ms for the 1 MiB
fixture; the remaining cost is the block clone and the shifted source ranges, so
it still grows with the document, but with a ~5× smaller constant.

The anchor walk underneath was quadratic when headings repeat: `Anchors::unique`
restarted its suffix scan at one for every heading, so the thousandth `## Same`
cost a thousand lookups. The 1 MiB fixture repeats one heading 1460 times and
spent ~84 ms there. `Anchors` now remembers the next suffix per base slug
(`crates/markview-core/src/document/heading.rs`), which took the full parse from
~105 ms to ~28 ms: 0.2 ms / 10 KiB, 1.7 ms / 100 KiB, 28 ms / 1 MiB.

### The debounce floor

`QUIET = 10 ms` and `MAX_WAIT = 40 ms` (`src/watch.rs`) mean no edit is shown
sooner than ~10 ms after the write, and a burst of writes is still coalesced.
The ceiling matters for editors that rewrite a file in several events: the old
100 ms ceiling was a fifth of the old 1 MiB number, while the 10 ms quiet window
is now small next to the document work.

### Other O(document) work on the edit path

- **Reading counts — implemented.** `extract_text` + `TextCounts::of` (ICU word
  segmentation over the whole document) used to run on the winit event loop
  while a complete update was accepted, ~3.8 ms / 100 KiB and ~21 ms / 1 MiB,
  on the thread that then had to render. The worker computes them after the
  prefix is published, skips the computation when a newer edit is already
  waiting, and caches the result by content identity so every complete update
  for that content carries it (`src/worker.rs`). They never delay a frame, and
  a second document with identical content still fills its footer.
- **Bottom-follow disables partial publication.** Scrolled to the bottom,
  `App::request` sets `coverage = f32::INFINITY` (`src/app/document.rs:10-20`).
  `PrefixPublication::publish` then needs `height >= INFINITY`, which never
  holds for a ≥ 32 KiB document, so no prefix is ever published and the reader
  waits for the complete re-layout (`src/worker.rs:78-89`). Watching a log at
  the bottom pays the full O(document) cost before the edit appears.
- **Per-block and per-prefix overhead.** Every pass computes an `external` key
  per block by walking the block's subtree once for images and once for code
  (`layout.rs`), and `Highlights::prepare` walks the document for code keys. All
  are O(blocks) on each pass. Published prefixes clone the `LayoutSnapshot`
  block vector and the `ImageSnapshot` entry map (`src/worker.rs:242-256`).
  Each is small alone but none is free.

## Metric 3: memory

RSS after scrolling through the document (offscreen; the native window adds a
few MiB for the surface and chrome):

| Fixture | Bytes | Blocks | RSS (MiB) | Peak (MiB) |
| --- | ---: | ---: | ---: | ---: |
| ordinary-10k | 10 KiB | 59 | 42.2 | 42.0 |
| unique-100k | 100 KiB | 438 | 53.0 | 53.0 |
| text-cjk-100k | 100 KiB | 588 | 49.7 | 49.5 |
| math-cjk-100k | 100 KiB | 712 | 51.1 | 51.1 |
| unique-400k | 400 KiB | 1747 | 107.3 | 107.3 |
| many-code-100k | 108 KiB | 600 | 92.9 | 92.9 |
| many-blocks-100k | 287 KiB | 4000 | 115.7 | 115.7 |
| text-cjk-1000k | 1 MiB | 5871 | 82.6 | 82.5 |

Against the stated targets, 10 KiB is 42.2 < 50 MiB and 100 KiB is 50–53 <
80 MiB, both with headroom. Growth is close to linear in glyph count: the
marginal cost is ~220 bytes per source character of prose (unique-100k to
unique-400k adds 54 MiB for 307 KiB). The repetitive 1 MiB fixture stays at
82.6 MiB because its blocks share very few content keys, so the cache and the
snapshot hold many references to a little geometry.

**No leak was found.** RSS plateaus and then stays flat across repeated edits:
`rss_slope_bytes_per_iteration` is within a few kilobytes per edit on every
fixture, and a churn through 13 distinct documents also plateaued. Because a
complete pass drops every cache entry it did not touch, the retained geometry
never grows past one document plus the blocks a superseded pass left behind.

What dominates the bytes:

- **Per-glyph layout representation.** `Draw` is 96 bytes and there is one
  `Draw::Glyph` per glyph, plus a 48-byte `TextCluster` per cluster, plus each
  node's reading-text `String` and grapheme boundaries
  (`crates/markview-core/src/scene.rs:75-106,160-184`,
  `crates/markview-core/src/text.rs:137-184`). For unique prose that is the
  ~220 B/char slope. This is the dominant per-document cost.
- **Allocator high-water from re-layout.** Re-laying out a block builds a fresh
  `BlockLayout` while the previous snapshot still holds the old one, and glibc
  keeps the arenas. Retaining every block removed the repeated re-layout that
  used to make this worse: many-blocks fell from 182 to 116 MiB and the 1 MiB
  fixture from 255 to 83 MiB. There is no `malloc_trim` anywhere, so the
  benchmark's RSS reflects the high-water, not the live set.
- **Fixed process floor plus tracked GPU.** Offscreen GPU resources are 7.9 MiB
  (4 MiB mask atlas + 3.84 MiB target + 256 KiB geometry), and wgpu/Vulkan
  driver, font blobs, syntect and ICU data make up the ~25–35 MiB baseline. The
  color atlas (1 MiB) is lazy.

Bounds worth knowing, with their cap:

| Cache | Cap | Eviction |
| --- | --- | --- |
| `LayoutEngine.cache` | one pass's blocks | entries not used by the last complete pass (`layout.rs`) |
| `Highlights.highlight_cache` | the document's code keys | dropped when a key leaves the document; per-pass `highlight_bytes` budget |
| `MathEngine.cache` | 256 entries | clear-all (`math.rs:41`) |
| `TextShaper.faces` / `font_sets` | appearance-driven | cleared every pass (`shaping.rs:195-201`) |
| GPU image textures | 256 MiB | only on the next insert (`render/images.rs:97-108`) |
| CPU decoded pixels | 256 MiB | only on the next insert (`src/images/cache.rs`) |
| Raster atlases | 4 MiB mask + 1 MiB lazy color | reset, not freed (`raster.rs`) |
| Geometry vertices | viewport-bounded | cleared per frame (`frame.rs:34-46`) |
| Tabs | count unbounded; inactive released after 20 s | `src/app/tabs.rs:11,176` |

Closing the last tab now calls `Worker::release`, which drops the retained
document, the engine's geometry and syntax colors, and the decoded image
entries, so an idle reader holds nothing from the document it closed
(`src/worker.rs`, `src/images.rs`).

## Optimization points, ranked

Impact is an estimate of the change against the metric it names; risk is the
chance of an output or behavior change.

### First frame

1. **One font context per process — implemented.** `TextShaper` builds the
   process-wide system font collection on first use, every other shaper clones
   it, and the worker starts the scan while the window and renderer initialize.
   Native first frame fell 94.2 → 82.3 ms (10 KiB) and 93.7 → 79.3 ms (100 KiB);
   the offscreen harness fell 71.6 → 51.0 ms. (`shaping.rs` `system_fonts`,
   `TextShaper::warm_system_fonts`, `src/worker.rs`.)
2. **Partial font discovery** — build the collection with system fonts disabled
   and register the families the stylesheet actually names, instead of
   enumerating all ~1287 families. ~15–20 ms, medium risk (unlisted scripts lose
   system fallback; verify against `fontdefs`). (`shaping.rs:220-337`.)
3. **Overlap or shrink `Renderer::new`** — create instance/adapter/device on a
   background thread while the font scan runs, and defer the image pipeline and
   the 4 MiB atlas until first use. ~15–25 ms, medium risk.
   (`pipeline.rs:21`, `raster.rs:83`.)
4. **Prefix-aware parse — implemented.** `document::parse_prefix` parses at most
   64 KiB of a fresh file, appends the reference definitions and footnotes the
   prefix needs (rescanning for them only when the prefix names a reference),
   and the worker lays out only enough of it to cover the viewport and publishes
   that before the complete parse. The first frame no longer grows with the file
   (4 MiB paints in ~70 ms, was ~172 ms, and a 4 MiB file with footnotes behaves
   the same). (`document/incremental.rs`, `src/worker.rs`.)
5. **Skip the empty "Opening document…" frame** (`src/app/document.rs:25`) to
   avoid one overlay build and submit.

### Edit

6. **Replace the 256/100 k cap with a persistent cache — implemented.** The map
   survives passes and cancellations, every complete pass drops the entries it
   did not touch, and `CacheKey.external` folds in the images and syntax colors
   a block uses. Removes the O(document) term (308 ms → 3 ms at 400 KiB).
   (`layout.rs`.)
7. **Fix the highlight cliff — implemented.** Each block's key names its own
   highlight result, so an arriving result invalidates only the blocks that use
   it, and the highlight cache is retained per document instead of cleared at
   256 entries. The 300-block fixture reuses 597 of 600 blocks per edit.
   (`highlights.rs`, `layout.rs`.)
8. **Reduce the debounce — implemented.** The quiet window is 10 ms with a 40 ms
   ceiling. (`src/watch.rs`.)
9. **Incremental parse — implemented.** A change confined to one block of a
   leaf-only document re-parses just that block and reuses the rest
   (`document/incremental.rs`). This removed ~16 ms of the 1 MiB edit's parse.
   What remains is the O(document) clone and source-range shift, and a full
   parse for documents with containers or reference definitions.
10. **Move `TextCounts::of` off the event loop — implemented.** The worker
    computes the counts after publishing the prefix and sends them with the
    update (`src/worker.rs`, `src/state.rs`), removing a ~21 ms / 1 MiB stall
    from the frame path.
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
16. **Intern the per-block cache inputs.** The resolved syntax theme is now a
    fingerprint, not a `String` per block per pass, but `external_key` still
    walks and hashes each block's subtree twice per pass, image specs and code
    keys. A single walk, or a per-block fingerprint cached across passes, would
    remove that. Low risk. (`layout.rs`.)
17. **Share `ImageSnapshot.entries` behind an `Arc`** instead of deep-cloning it
    once per pass and once per published prefix. Low risk. (`layout.rs:224`,
    `src/worker.rs:242-256`.)

### Memory

18. **Compact the glyph representation.** `Draw` at 96 bytes plus a 48-byte
    `TextCluster` per glyph is the memory slope; a structure-of-arrays or a
    smaller glyph record, and `shrink_to_fit` on the final snapshot, would cut
    the ~220 B/char materially. Medium risk. (`scene.rs:75-106`, `paragraph.rs:294-461`.)
19. **Return freed memory after a document change** — `malloc_trim(0)` on Linux
    after publishing, or a decaying allocator — to give back the high-water that
    an option or document change leaves behind. Low risk.
20. **Clear the worker's cached document on cancel — implemented.** The last tab
    close releases the retained document, geometry, colors and decoded images.
    Capping the tab count and byte-bounding the math cache remain. Low risk.
    (`src/worker.rs`, `src/app/tabs.rs:11`, `math.rs:16`.)
21. **Evict GPU and CPU images by visibility** rather than only on the next
    insert, so a document switch or tab close releases up to 256 MiB each.
    Medium risk. (`render/images.rs:97-108`, `src/images/cache.rs`.)

The first-frame and edit items (1, 4, 6–10) are implemented. What is left on
those paths is a full parse for documents with containers or reference
definitions, and the O(document) clone the incremental parse still does. Items
2–3 are where the remaining cold-start time is — the ~25 ms `Gpu::new` and the
font scan behind it. Items 18–19 are what would give a large document a smaller
memory slope and a lower high-water.

## Measurement caveats

- The native numbers include window creation and compositor handoff at DPR 2;
  the offscreen numbers exclude them, which is why the same fixture can read
  50 ms offscreen and 80 ms natively.
- The machine's `intel_pstate` governor reads `powersave` with
  `balance_performance` energy preference, which is its normal state; the
  `balanced` platform profile was left in place. On this host `power-saver` has
  historically run 1.6–2× slower in every stage, so re-run acceptance with the
  `performance` profile.
- `bench`'s `reading_text_index_bytes` samples the cached-refresh snapshot and
  is not a retention metric. Do not use it to judge layout memory; read
  `latency`'s RSS instead.
- The offscreen first frame is noisy across processes: on this host three
  processes of the same fixture spread over 10–20 ms, and running all baseline
  processes before all candidate processes biases the comparison with thermal
  drift. Alternate the two binaries when accepting a change.
- Each offscreen run is a fresh process with a cold document and font cache, but
  the OS file cache and GPU driver state persist across runs, so the first run of
  a session reads higher than the reported median. The worker's font scan is on
  the critical path and is included in the first-frame number.
- Raw reports for every table are under `artifacts/perf-analysis/` (ignored by
  Git), together with the preserved release binary of the analyzed revision.
