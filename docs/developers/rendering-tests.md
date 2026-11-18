# Rendering regression tests

Run the committed Linux visual suite with no physical GPU:

```sh
bash scripts/render-env.sh
```

The wrapper needs Podman or Docker and runs the Linux amd64 image pinned by
digest in [`ci/render/image`](../../ci/render/image). It automatically selects
Podman when installed; set `MARKVIEW_CONTAINER_ENGINE=docker` to select Docker.
The GHCR package is public, so local users and downstream forks can pull the
pinned image without registry credentials or package permissions in CI.
The image supplies Rust, Lavapipe, LLVM, Fontconfig, FreeType and Poppler;
host graphics drivers and libraries are not mounted. The normal Linux workspace
tests skip `rendering_baselines`; the independent `golden.yml` workflow calls
the same wrapper in CI. The suite uses the production parser,
layout, highlighting, image scheduler, Vulkan renderer and PDF writer.
Windows and macOS continue to run their existing rendering tests.

Use `bash scripts/render-env.sh --update` only after reviewing an intended change. Normal runs never
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
arbitrary system/downloaded fonts. Driver and Poppler pins belong in
[`ci/render/Dockerfile`](../../ci/render/Dockerfile), alongside the base image
digest, Ubuntu archive snapshot and Rust archive checksum.

The wrapper mounts the checkout at `/workspace`, with the caller's user and
group IDs so generated files remain editable. Cargo downloads and build outputs
live under `${XDG_CACHE_HOME:-$HOME/.cache}/markview/render/<image-digest>/`,
separate from native builds and other rendering images. Host networking lets
Cargo use local proxy settings; no GPU devices or host libraries are shared.
The image prints its CPU model, Rust, native-library and Vulkan driver versions
on each run.
Both local and CI baseline runs use amd64. LLVM's runtime code generation still
depends on the host CPU, so the two-level channel tolerance remains in force.

`scripts/test_render_goldens.sh` is the runner inside the image. Direct native
runs are useful for debugging, but committed baseline updates use the wrapper.

## Update the rendering environment

Change the Dockerfile's pinned inputs together, then build and inspect the image:

```sh
podman build --network=host --platform linux/amd64 \
  -t ghcr.io/szdytom/markview-render:reviewed ci/render
```

Publish with a GitHub token that has `write:packages`, without including the
token in shell arguments:

```sh
gh auth token | podman login ghcr.io --username "$(gh api user --jq .login)" --password-stdin
podman push --digestfile /tmp/markview-render.digest \
  ghcr.io/szdytom/markview-render:reviewed
printf 'ghcr.io/szdytom/markview-render@%s\n' \
  "$(cat /tmp/markview-render.digest)" > ci/render/image
```

Keep the GHCR package public so downstream forks retain anonymous pull access.
Run `bash scripts/render-env.sh` against the published
digest, review differences, then update only the affected baselines. Re-run
without `--update`, including once with an empty cache. Keep environment and
baseline changes together for review. Ordinary test runs never rebuild the image
or resolve a mutable image tag.

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

For reviewed rounding differences between rendering environments, a shared
reference can preserve the two-level tolerance. Use actual frames from each
environment, rather than assuming the old baseline equals an actual frame:

```sh
python3 scripts/reconcile_render_golden.py \
  tests/goldens/hover-preview/image-fit-dark-125.png \
  artifacts/local-actual.png artifacts/ci-actual.png
```

The script requires Pillow and updates the baseline in place. For each RGBA
channel it intersects the actual values' tolerance intervals, preserves the
existing value when possible, and otherwise moves it to the nearest valid
value. It refuses mismatched dimensions or a channel spread greater than four,
without writing the baseline. Supply at least two frames; additional frames
constrain the same intersection. This is a manual maintenance step after visual
review, never an automatic response to a golden failure.

For `image-fit-dark-125`, the original local baseline (`ea2f7476`) matched the
local render exactly, and the CI-derived baseline (`4ebdd986`) came from the
actual frame in
[CI run 43613210551](https://github.com/szdytom/markview/actions/runs/43613210551).
Reconciling those frames changed 30 channels across 14 pixels by one level each;
the reference remains within two levels of both outputs. Re-run the golden
suite and compare against the saved CI actual after reconciliation. Test the
script with `python3 -m unittest discover -s scripts -p 'test_reconcile_render_golden.py'`.

## Measure hover previews

The ignored `measure_hover_preview` test measures a loaded document through the
30 ms preparation deadline, the default 250 ms display delay, asynchronous
subtree layout, chrome and offscreen GPU completion. It records the first hover
and 30 reopenings, including background build time and the longest UI tick.
Build time overlaps the display delay. Run each case in a fresh process with
the Linux software-rendering environment above:

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
