# Development guide

This page is for contributors changing Markview. It is procedural: the design rationale is in [architecture](architecture.md), and the MVSS format is in [the stylesheet guide](stylesheets.md).

## Build and verify

Use the locked workspace commands from the repository root:

```sh
cargo fmt --all --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
```

Release archives, installers, and the platform icons are maintained separately;
see the [packaging guide](packaging.md). After changing `assets/markview-icon-color.svg`:

```sh
cargo run -p xtask -- icons
```

For visual or timing changes, also use the real pipelines:

```sh
target/release/markview --render examples/welcome.md --output artifacts/welcome.png
target/release/markview --pdf examples/welcome.md --output artifacts/welcome.pdf
target/release/markview --smoke-test examples/welcome.md --output artifacts/window.png
target/release/markview --bench tests/fixtures/ordinary-10k.md --output artifacts/ordinary.json
target/release/markview --bench-latency tests/fixtures/ordinary-10k.md --output artifacts/latency.json
python3 scripts/smoke_watch.py target/release/markview
```

`--pdf` writes the paper edition: the document is laid out again at the page's
text measure, broken into pages, and written as vector text with subset fonts.
Check an export in a viewer (`pdftotext`, `pdfinfo`, `qpdf --qdf`) for page
count, page furniture, link annotations, text selection and embedded fonts.
A block that had to shrink to fit the page, or a band taller than the page, is
reported on stderr. The export never depends on the GPU, so it runs headless.

The GPU renderer and the PDF writer share the layout and the `print` sheet, so
the same document at the same measure must place the same content in the same
place. `scripts/compare_pdf_render.py` holds them to that: it exports one
fixture both ways, rasterizes the PDF page with Ghostscript, and reports each
content band's best alignment, profile overlap, ink ratio and a windowed SSIM,
then fails on a shift, a missing band, or a low overlap. It needs python3 with
numpy and Pillow, plus `gs`:

```sh
python3 scripts/compare_pdf_render.py examples/welcome.md --out /tmp/pdf-cmp
```

At the default two device pixels per layout pixel the two images overlap by
86–97% per band with no shift; the remaining difference is glyph rasterization
weight, because the GPU bakes subpixel coverage into bitmaps while Ghostscript
antialiases vector outlines. `--out` keeps a side-by-side image and a diff heat
map for eyeballing.

`--bench-latency` measures the two latency targets and the reload memory trend
through the real layout worker and prefix publication: process entry to the
first readable GPU frame, a small on-disk edit to the first refreshed frame and
to the complete re-layout, and per-edit RSS. Aggregate independent processes
with `python3 scripts/bench_latency.py`. The shipped large fixtures repeat
paragraph bodies, which the content-keyed block cache hides; generate
unique-content and cache-stressing fixtures with
`python3 scripts/generate_stress_fixtures.py` before drawing scaling
conclusions. The [latency and memory analysis](performance-analysis.md) records
the baseline those commands produced.

`scripts/generate_large_fixture.py` writes 100 KiB `math-cjk-100k.md` and
`text-cjk-100k.md` fixtures for large-document timing. Set `MARKVIEW_PROFILE=1`
when running `--bench` to add inclusive per-sub-stage layout timings to the
report under `profile_ms`; leave it unset for production-comparable numbers,
because the probes add roughly 5 % to layout.

`tests/fixtures/emoji-fallback.md` exercises bold check/cross Emoji, keycaps,
headings, emphasis and mixed scripts. Include it in cold-process comparisons:
warm reflows alone hide font fallback initialization costs. For a stage breakdown:

```sh
MARKVIEW_PROFILE=1 target/release/markview --bench tests/fixtures/emoji-fallback.md --iterations 3 --output artifacts/emoji-profile.json
```

`layout.font_resolve_ms` measures configured candidate resolution/loading;
`layout.font_choose_ms` measures cluster coverage selection and fallback warnings;
`layout.shape_build_ms` measures Parley's build, including its own font fallback
and shaping. These are inclusive diagnostic spans, not additive pipeline stages.
Render this fixture in both themes as well as measuring it. Default Emoji
candidates explicitly use weight 400 because many Emoji fonts have no bold face.

When no configured face covers a cluster/word, a `WARN` line on stderr reports its
Unicode codes, requested candidates and available exact faces before handing it to
Parley. Normal selection of a later configured candidate is silent. Warnings are
limited to one per candidate set (including requested weight), at most 64 per text
shaper, and survive reflow/stylesheet resets. Internal object placeholders are
excluded. To exercise warnings, use a temporary custom style with unavailable font
families or an Emoji candidate inheriting weight 700; verify repeated reflows stay
quiet.

The reader and the core crate log through the `log` facade, and the binary writes
`LEVEL message` lines to stderr. A plain window run defaults to `warn`, so the
lifecycle and timing lines stay quiet; `--render`, `--bench` and `--smoke-test`
default their own targets to `debug` so the diagnostics below are visible. Set
`RUST_LOG` to override either, for example `RUST_LOG=info` for the display metrics
or `RUST_LOG=debug` to include dependency logs.

The render and benchmark modes use the GPU offscreen and do not load personal settings. The watch smoke test writes only temporary documents and closes the window it starts.

Window layout publishes a readable prefix before completion. Native `--smoke-test`
logs `process entry→readable GPU frame` (true process entry) and
`process app entry→readable GPU frame` (after command-line and stylesheet setup)
for that first frame and `full layout complete` separately, then waits for the
complete snapshot to render before exiting. Its PNG captures the first readable
frame. Offscreen `--bench` and `--render` continue to use complete geometry;
their timings must not be reported as progressive window first-frame timings.

Ignored GPU tests are useful for settings, selection, and image-frame regressions:

```sh
cargo test --workspace --locked settings_and_selection_frame -- --ignored
cargo test --workspace --locked tab_strip_frames_clip_overflow_at_fractional_dpi -- --ignored
cargo test --workspace --locked gpu_frame_draws_decoded_images -- --ignored
cargo test --workspace --locked color_glyphs_preserve_rgb_and_share_paint_order -- --ignored
```

## Choose the layer

1. Put Markdown meaning, reading text, geometry, hit testing, and selection mapping in `crates/markview-core`.
2. Put GPU resources, clipping, rasterization, and frame assembly in `crates/markview-render`.
3. Put files, settings, watching, image I/O, platform effects, commands, and window interaction in the root crate.

Keep core free of window, GPU, clipboard, filesystem, and configuration dependencies. Prefer immutable snapshots and explicit version tags at asynchronous boundaries. Reuse the retained `Document` when only layout settings change.

## Add a document node

Use this sequence when adding a Markdown or HTML construct:

1. Identify the semantic input and its intended reading-text and copy behavior. Do not begin with a renderer primitive.
2. Extend `BlockKind`, `InlineKind`, or the relevant style data in `crates/markview-core/src/document.rs` (and `html.rs` for supported raw HTML).
3. Parse the construct into that semantic representation, preserving a source range and stable reading order. Keep unsupported syntax as literal text rather than silently dropping it.
4. Add layout behavior in `layout.rs`. Decide whether the node is text, a block, or an atomic inline box; produce text nodes and geometry together so hit testing and copying use the same mapping.
5. Add only the semantic paint instructions needed by the renderer. Do not make the renderer reinterpret Markdown.
6. Add a stylesheet condition only if the node has a visual role that no existing condition expresses. Validate it through the same MVSS parser as bundled and user styles.
7. Add focused unit tests for parsing, source ranges, reading text, selection/copying, layout, and cache identity. Add a renderer test only for GPU-specific behavior.
8. Add a fixture or example when the feature is difficult to understand visually, then run the full workspace checks.

Common mistakes are treating source offsets as persistent identity, putting display-only text into copied reading text, using glyphs as the selection model, and creating a second layout path for a special inline object. These produce subtle bugs in reflow, repeated blocks, CJK selection, and asynchronous reloads.

## Change asynchronous behavior safely

When a worker or loader changes, test stale-result rejection, replacement of pending requests, failed reads, and versioned image completion. Preserve the last valid snapshot on recoverable errors. When a change affects only colors or interaction overlays, avoid invalidating geometry; when it affects fonts, width, spacing, or intrinsic asset size, invalidate the relevant layout.

## Keep documentation maintainable

Document guarantees and reasons in architecture pages, procedures and examples in guides, and measured facts in the performance page. Remove a stale statement instead of adding a contradictory exception. Link to the owning page rather than copying the same rule into several documents.

## Refactor with a preserved baseline

Before changing a performance-sensitive path, build the current release and copy
its binary outside `target/release`. Keep its source revision with it. After the
change, build the candidate with the same lockfile, toolchain and release profile.
Run on an idle machine with hardware GPU access, on AC power with the
`performance` power profile: on `power-saver` this host ran 1.6–2× slower in
every stage, and the preserved binary reproduced the slowdown, so a power-limited
run looks exactly like a regression without being one.

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

Keep visual checks alongside timing checks. Compare deterministic offscreen
exports with the preserved binary, then run the ignored GPU tests and native
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
