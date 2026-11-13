# Develop Markview

This guide takes a contributor from a checkout to a validated change. Read
[CONTRIBUTING](../../CONTRIBUTING.md) for contribution policy and
[AGENTS.md](../../AGENTS.md) for repository conventions.

## Build and verify

Use a Rust toolchain satisfying `workspace.package.rust-version` in
[Cargo.toml](../../Cargo.toml). On Debian/Ubuntu, install native build dependencies:

```sh
sudo apt-get install libfontconfig1-dev libxkbcommon-dev libwayland-dev mesa-vulkan-drivers vulkan-tools
cargo build --locked
cargo run --locked -- --foreground examples/welcome.md
```

The reader needs fonts covering the document's scripts. Tests use committed font
subsets. For Web-only or Android development, use the separate
[Web](web.md) or [Android](android.md) setup instead.

Before submitting a change:

```sh
cargo fmt --all --check
python3 scripts/check_local.py --compile
cargo test --workspace --all-targets --locked
cargo run -p xtask --locked -- complexity
```

`check_local.py` requires `actionlint`; it runs repository checks, formatting and,
with `--compile`, Cargo check and Clippy. `--list` lists the selected checks.
Use `--icons` for icon assets, `--dist` for cargo-dist configuration and `--full`
for workspace/PDF tests. See [testing](testing.md) for GPU setup and specialized
suites. A missing GPU adapter is a test setup failure, not a skipped test.

## Choose the layer

Use the [architecture map](architecture.md#document-processing-and-rendering).
Put a change at the owner of its behavior: source interpretation in a front end,
reading geometry in layout, drawing in a backend, and user policy in the host.
Native services belong in `markview-platform`.

Start with a focused regression that fails for the reported behavior. Test the
observable result rather than the implementation's own intermediate calculation.
Then run the relevant integration suite and the repository checks above.

## Add a document node

1. Define the construct's meaning, reading order and copy/search behavior.
2. Add its semantic representation in `markview-document` and conversion in the
   appropriate front end. Preserve source mapping where source text exists.
3. Produce reading text and geometry together in `markview-layout`. Decide how
   the construct behaves in selection, disclosures, overflow and pagination.
4. Extend paint instructions only when the existing vocabulary cannot express
   it. Renderers must not reinterpret source syntax.
5. Test parsing, identity, source ranges, copy/search, reflow and export. Add
   visual fixtures for visible changes and GPU tests for GPU-specific behavior.

Do not use source offsets as persistent identities, glyphs as the copy model,
or a separate layout path for a special inline object.

## Change asynchronous behavior safely

Test replacement while work is pending, stale completion, cancellation and failed
reads. Acceptance must check the current session/content/request even when the
worker cooperates with cancellation. Preserve the last usable snapshot on a
recoverable read failure. Reuse parsing for layout-only changes.

For settings and panels, follow [Native UI](ui.md). Reader and export preferences
have separate effects: changing paper settings must not reflow the reader.
For resource work, preserve the [security boundary](security.md).

## Add a UI icon

1. Add a square geometry-only SVG under `assets/ui`, with consistent dimensions
   and viewBox. Set `fill` explicitly on filled figures; theme colors paint it.
2. Register it with `markview_icon::icon!` in `src/app/chrome/icons.rs`.
3. Assign it to the button and retain the button label for identity/tests.

For generated application and callout assets, run `cargo run -p xtask -- icons`
and check the result with `cargo run -p xtask -- icons --check`.

## Bundled themes and visual verification

Follow the [MVSS authoring workflow](../users/stylesheets.md#authoring-workflow).
Register bundled themes in the reader/PDF registry in
`crates/markview-layout/src/style.rs`, according to their output destination.
A theme normally needs no parser or renderer changes. New MVSS fields need parser
and rendering coverage plus a format-reference update.

Run the [visual suite](rendering-tests.md), inspect the images, and test the theme
with both pinned test fonts and its intended fonts. Pinned-font coverage does not
establish how an optional downloaded face looks.

## Debug a running reader

Window launches normally detach. Keep logs in the terminal with:

```sh
RUST_LOG=debug target/debug/markview --foreground examples/welcome.md
```

For timing or native screenshots, build with `--features diagnostics` and follow
[performance measurement](performance.md). For reproducible README captures, use
`scripts/capture_screenshots.sh` on its supported KDE/Wayland desktop; source
fixtures live under `docs/screenshots/source/`.

## Keep documentation maintainable

Follow the [documentation rules](documentation.md). Update the page that owns a
changed user behavior or public contract. Internal refactoring alone should not
require rewriting guides. Run `python3 scripts/test_documentation.py` after edits.
