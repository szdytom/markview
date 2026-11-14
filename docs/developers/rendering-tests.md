# Rendering regression tests

Run the committed Linux visual suite with no physical GPU:

```sh
bash scripts/test_render_goldens.sh
```

It needs Lavapipe and `pdftoppm` (`poppler-utils`). The normal Linux workspace
tests skip `rendering_baselines`; the independent `golden.yml` workflow runs
the comparisons in CI. The suite uses the production parser,
layout, highlighting, image scheduler, Vulkan renderer and PDF writer.
Windows and macOS continue to run their existing rendering tests.

Use `--update` only after reviewing an intended change. Normal runs never
create or replace baselines. Every RGBA channel of every pixel must be within
two levels of its baseline, and dimensions must match. This accommodates the
observed Mesa rounding difference without allowing a percentage of bad pixels.

| Suite | Baselines | Failure artifacts |
| --- | --- | --- |
| Markdown, MVSS and PDF pages | `crates/markview-render/tests/goldens/` | `artifacts/render-goldens/` |
| Native settings and button states | `tests/goldens/ui/` | `artifacts/ui-goldens/` |
| Connected tabs: hover symmetry and active-tab drag overlap at 100%, 125% and 200% DPI | `tests/goldens/tabs/` | `artifacts/tab-goldens/` |
| Footnote hover previews: short/long content, selection, images, scrolling, light/dark and fractional DPI | `tests/goldens/hover-preview/` | `artifacts/hover-preview-goldens/` |
| Mermaid and inline SVG | `tests/goldens/diagrams/` | `artifacts/diagram-goldens/` |
| Search and text selection | `tests/goldens/search/` | `artifacts/search/` |

Failures retain actual, expected and magenta difference PNGs. PDF cases also
retain the exported PDFs. CI uploads the artifacts only when tests fail and
retains them for three days.
Unused baselines fail the Markdown, UI, hover preview and diagram suites, so removing a case
cannot silently leave an untested PNG. Every Markdown fixture must be registered.

## Add a visual case

Use a small fixture that isolates the visible behavior, then a composition case
when containers or inline objects interact. Register it in the owning suite;
unused baselines and unregistered Markdown fixtures must fail checks.

```sh
python3 scripts/check_render_coverage.py
```

The coverage checker reads the stylesheet schema and requires visual declarations
for drawing fields. The suite registry, not a second hand-maintained table here,
owns the current fixture list. Include narrow/wide measures, relevant themes,
fractional scale and regional typography when those affect the change.

The theme sampler covers reader/PDF registries and desktop/phone media. Paper
cases export real PDFs and rasterize them. Downloadable theme fonts are not
fetched by tests; visual checks use pinned fixtures. Metadata and download-source
validation need parser/service tests rather than pixel assertions.

## Fonts and reproducibility

Tests hide system fonts and use committed subsets. Regenerate with
`scripts/generate_test_fonts.py` when coverage changes; it needs source Noto fonts
and Python fontTools. Font provenance lives with [the fixtures](../../tests/fixtures/fonts/README.md).
Missing glyphs and unintended clipping should fail, not silently become a new baseline.

Use real regional faces for SC/TC/JP and test emoji versus text presentation,
combining marks, ligatures, punctuation, formulas and code when changing shaping.
Pinned fonts make comparisons useful; they do not promise identical output with
arbitrary system/downloaded fonts. Driver and Poppler pins belong in CI workflows.

## Reviewing a baseline change

Inspect the source fixture and every changed actual image, then compare its
expected/difference artifacts. Check that text, images and equations are present;
that only the intended layout/color changed; and that another theme, width,
regional convention, page or state did not change unexpectedly. Re-run the
normal suite after updating, followed by workspace tests and Clippy.

Keep the two-level tolerance fixed. Driver or Poppler changes need a controlled
comparison before updating images. Pinning the test fonts prevents host font
installation from changing the baseline; it does not assert that every user's
downloaded/system font produces identical pixels.

## Measure hover previews

The ignored `measure_hover_preview` test measures a loaded document through the
real 30 ms preparation deadline, 100 ms display gate, asynchronous subtree layout,
chrome and offscreen GPU completion. It records
the first hover and 30 reopenings. It also records background build time and the
longest UI tick; build time overlaps the display delay and is not added to it. Run each case in a fresh process, with the
Linux software-rendering environment above:

```sh
MARKVIEW_HOVER_CASE=long MARKVIEW_HOVER_OUTPUT=/tmp/hover-latency.json \
  cargo test --release --locked --lib measure_hover_preview -- \
  --ignored --nocapture --test-threads=1
```

Cases are `short`, `long`, `image`, `tall-image` and a generated approximately 25 KiB `stress`
note. Repeat with `--features diagnostics` for process-wide Rust allocation
counts, cumulative requested bytes, peak extra live bytes and retained bytes
across scheduling, background construction, display gating and the first frame. Allocation counting is test-only and
changes timing; use the uninstrumented run for latency. Requested bytes include
the full new size of reallocations and exclude native driver allocations.

RSS and process high-water RSS come from `/proc/self/status`; the high-water
mark includes document loading and renderer initialization. Images are already
decoded. GPU completion excludes window events, compositor presentation, display
refresh and screenshot readback. These are diagnostic measurements, not timing
assertions in CI.
