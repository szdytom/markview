# Changelog

All notable changes to this project are documented in this file.

For new entries:

- Use dense lists: each item stays on one line.
- No soft breaks.
- Entries are grouped under sections such as `Added`, `Changed`, and `Fixed`.

Historical entries keep their existing format.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!--
cargo-dist takes the H2 section whose version matches the tag as the GitHub
Release notes, and its heading as the release title. Keep one H2 per release,
at the same level, without `[brackets]`.
-->

## Unreleased

### Added

- Add compact, right-aligned stacked-square SVG buttons in the Contents header to expand or collapse all outline sections.
- Add the 8-bit reader theme with green CRT colors, Fusion Pixel typography throughout the document and UI.
- Collapse and expand nested table-of-contents sections with per-tab state and keyboard navigation that skips hidden headings.

- Add a compact About tab in Settings with a centered application icon, the standard project description, a clickable project link and one-click copying of diagnostic information: version, build date, OS, active WGPU backend, and commit.

- Read `---` fenced YAML front matter: a flat mapping renders as a two-column table with a bold key column, anything nested renders as a `yaml` code block, and `front_matter` joins the MVSS vocabulary as the frame around either shape — with `table`/`cell` or `code_block`/`label` composing into one rendering, and `show = false` hiding the block while its metadata stays parsed. A document that opens with an unclosed `---` is unchanged.

### Changed

- Prefer upright LXGW WenKai for Simplified Chinese emphasis in reader and PDF themes, offer it through GitHub, SourceForge and archlinuxcn (Europe/TUNA) font downloads, and fall back through system Kai fonts (`regularscript[cjk]`) to synthetic italics.

- Reuse segmented settings controls for the Fonts status filter, with shared borders and consistent selection and focus highlights.

- Rework the Styles settings with clearer priorities: numbered badges for enabled styles, and arrows hidden at the ends of the order.
- Rework the Fonts settings: drop the source filter, rename the states to Missing / Downloaded / In System, split the bulk action into Download Missing and Download All, and show sources, download progress and empty-state guidance.
- Consolidate panel navigation and font UI ownership, share bounded HTTP transport, and decouple PDF requests from CLI launch options.
- Clarify paired performance checks under any consistent power mode; refresh absolute README metrics only during releases.
- Compare readers and PDF pipelines against SuperGoodViewer 1.0.8, whose mathematics fixtures now render and whose new `sgv export` command joins the PDF table.
- Refresh the README size, memory and first-frame figures to the current build.

### Fixed

- Draw shared borders in segmented settings controls only once, preserving selection and keyboard focus highlights.

- Update font download counts as each file finishes, accumulate transferred bytes correctly, and show a continuous progress bar using each active transfer’s byte fraction with clearer details and an SVG cancel control.

- Draw the increase/decrease controls with SVG icons instead of font glyphs, avoiding minus-sign font fallback warnings.
- Keep the bulk font-download buttons hoverable, and silently do nothing when no downloads are needed.
- Align the Styles summary and separator with Generic settings while keeping the tabs clear on short panels, align style text, center the priority badges and row controls, and hide unavailable arrows.
- Build the downloadable-font catalogue when the Fonts or Styles page opens instead of during window startup, removing a system-font collection build from the first frame and about 8 MiB from the reader.
- Name the requested resource in shared HTTP transport errors, so a failed font download no longer reports an image error.
- Fold a ligature's continuation clusters into the cluster that draws the glyph, so its selection highlight, hit testing and copy range cover the whole ligature instead of leaving a gap over half of it.

## 0.1.5 - 2026-09-22

This release adds a downloadable font catalogue, a table-of-contents drawer, collapsible `<details>` blocks, and theme-colored Mermaid diagrams.

### Added

- Let a stylesheet draw Mermaid diagrams in its own colors with a `[mermaid]` table: a built-in preset plus any of the renderer's fields, overlaid field by field as `[page]` is. `font_family` names `fontdef` ids the way a rule's `font` does. Diagrams measure and draw with the reader's own faces — the theme's list, then the body's Han faces for a cluster the list cannot draw, downloads and `--fonts` included — so a Chinese label keeps the reader's regional face. The bundled dark reader themes use it now, so they no longer show diagrams on white paper.
- Add a persisted `scroll-speed` preference (0.5×–2×, in Settings) that scales every wheel notch and arrow step.
- Add a table-of-contents drawer (`Ctrl+B`, or the toolbar's outline button) that lists a document's headings, highlights the reading position, and jumps to a heading through the ordinary anchor path; it is an overlay, so the document keeps scrolling and selecting behind it, while an open panel or confirmation keeps input precedence over it. A heading near the end scrolls to the top too, using the blank tail the other scroll paths already reach.
- Declare downloadable font families with `[[font-family]]`, independent of `fontdef`: a family lists the names it may already have (any of them being installed skips the download), optional description, SPDX license and homepage, and one or more mirror sources. A source is either a set of direct files or an archive whose container is recognized from its own bytes and whose members are picked by `**`-style patterns, with an optional `sha256` and a 2 GiB archive cap; a failed mirror is removed before the next one runs. `fontdef.urls` is gone with it.
- Give the settings panel three tabs — Generic, Styles and Fonts — and put the whole font catalogue on the Fonts tab: every family the builtin recommendations and the catalogued stylesheets declare, with its name, description, license, size and owner, filters by source and state, one download and one cancel per family, and an **Open fonts folder** button. Styles no longer mention fonts at all.
- Add `markview fonts list|download|path|verify`, and move the diagnostic modes to subcommands (`render`, `pdf`, `bench`, `latency`, `smoke-test`) parsed with `clap`; `markview FILE` still opens the reader, and the old `--render`-style flags now name the subcommand that replaced them.
- Recommend Noto in the reader itself: the builtin stylesheet declares Noto Serif, Noto Sans, Noto Sans Mono, Noto Serif CJK SC and Noto Sans CJK SC, each mirroring the official GitHub release archive with jsDelivr, the canonical ctan.org redirector and the Tsinghua CTAN mirror, and downloading every static weight its mirrors publish—the nine Latin weights with their italics, seven per Chinese subset.
- Offer Noto Color Emoji as a downloadable family, mirrored from GitHub, jsDelivr, ctan.org and the Tsinghua CTAN mirror.
- Accept a color bitmap face as a font download: `CBDT`/`CBLC` or `sbix` strikes count beside the outline tables, since the rasterizer already draws them.
- Offer Fira Code as a downloadable family, from the upstream variable release and from Arch's `ttf-fira-code` package on archlinux.org and the Tsinghua mirror.
- Cache network images on disk beneath the configuration directory: honor `Cache-Control`/`Expires`, revalidate stale entries, hold at most 128 MiB with LRU eviction, and serve a cached body under `--offline`.
- Render a raw `<details>` block as a collapsible element: clicking its summary toggles a Markdown body (nesting and `open` supported), `details` and `summary` join the MVSS vocabulary, and both exports show every body expanded.
- Render a `mermaid` fenced block as a diagram: the library runs on the image workers, its SVG and pixels are cached per source, and the result appears in the window, `--render`, `--pdf` and `--smoke-test`, while a broken diagram keeps the image placeholder and `--offline` still renders local diagrams.
- Measure per-frame scroll pacing in `--bench` over a cold, warm and prewarmed pass: prepare and total percentiles, frames over the 120 Hz and 60 Hz budgets, glyphs rasterized in and out of the frame, and atlas pressure, including for a document with no geometry.
- Support independent box edges and corners, heading markers, letter spacing, position conditions and pagination hints in reader and PDF output.
- Add paper-edge rules under `page.header` and `page.footer`, with symmetric `rule_width`/`rule_color` decorations for PDF and PNG exports.

### Changed

- Redesign the Settings header with a persistent title and clickable Generic, Styles and Fonts tabs.
- Draw the Styles page's order buttons as borderless vector arrows, matching the rest of the chrome instead of text glyphs.
- Replace the Styles and Fonts pages' paging arrows with a scrolling list that shares the panel's wheel and scrollbar, clipping each row to the visible band.
- Prepare Mermaid fonts and SVGs on the image workers, allowing the first layout to publish before a cold font collection is ready.
- Load restricted SVG fonts only from configured Mermaid, SVG generic, and selected CJK `fontdef` candidates, avoiding full system-font materialization on first open.

- Keep a diagram's parsed source across a theme change, so a new `[mermaid]` table redraws the diagrams already on screen without parsing them again.
- Honor the system's per-axis lines- and characters-per-notch on Windows and count a Linux wheel detent as the usual three lines instead of one.
- Ease discrete scroll requests and wheel notches for 120–400 ms: Page Up/Down, `Space`, `Home`/`End`, the arrow steps, a click on the scrollbar track and a `#heading` jump, with a reversing wheel taking over from the displayed offset, while a thumb drag stays immediate and ends a running animation.
- Expand the `<details>` elements framing a heading a `#anchor` link or outline entry names, so a jump into a collapsed body reaches it instead of reporting it missing.
- Hold a wheel gesture to the axis its first few moments chose, separated by the boundary the platform reports or by a pause where it reports none and inheriting nothing from the gesture before it, so a diagonal trackpad gesture no longer stops the page or slips a wide formula away, and a sideways gesture over no wide block still scrolls by the vertical motion it carries.
- Prepare the screenful below in idle frames, leaving the visible frame's image demand alone, so no scroll frame rasterizes a glyph once a document is open: the worst such frame on a 100 KiB CJK document falls from 2.6 ms to 0.6 ms.
- Reuse the PDF writer's font and stylesheet caches across a watch session's rebuilds instead of rebuilding them on every export.
- Export PDFs about 40% faster: transparent fills are no longer written, an inline run's background is one rectangle instead of one per cluster, and a formula's glyphs leave as runs rather than one text object each.
- Compare readers against SuperGoodViewer in the README tables, adding its open time and a resident-memory table for all three readers.
- Measure each reader's resident memory and record readers that fail to render a fixture, instead of timing their compile-error window.

### Fixed

- Close the table of contents when clicking outside its drawer.
- Make platform and image-cache tests portable across macOS, Linux and Windows.
- Fix `<details>` parsing and interaction edge cases: tags that share a block and elements nested in the opening block still match, a summary stays with its own element, a quoted body is not quoted twice, references and footnotes defined outside the element resolve, identical elements toggle independently, and adjacent elements keep their content and nesting budget.
- Reject pathologically nested Mermaid labels and render the diagram on a stack sized for the worst case the source cap allows, so a fence that passes the size bounds can no longer abort the reader.
- Keep a Mermaid diagram's fence source out of the reading text, so selecting the figure no longer copies the source and a loading or failed diagram copies its placeholder message instead.
- Remove soft line breaks from Chinese Markdown prose to avoid inserting spaces.
- Name the download client with a `User-Agent`, so a mirror like Tsinghua's Arch repository answers instead of refusing an anonymous request.
- Name `Noto Serif SC` and `Noto Sans SC` in the bundled Chinese `fontdef` lists, so the subset faces a download installs are actually shaped instead of being skipped.

## 0.1.4 - 2026-09-19

This release adds stylesheet discovery and validation, expands theme and typography support, and improves reader previews.

### Fixed

- Avoid spurious font fallback warnings from UI headings requesting unavailable weight 600 faces.

### Added

- `markview ss list` lists bundled and installed stylesheets.

- Optional CJK Medium UI overlay and GPU weight comparisons, with documented exact-weight fallback behavior.

- Monochrome, Qi Baishi, Van Gogh and Mondrian PDF themes, plus `theme = "none"` for uncolored code.

- MVSS `targets` declares UI/PDF destinations, filters theme selectors and rejects incompatible use.

- Celadon, Blueprint and Rosewood reader themes, a theme preview fixture, and an MVSS authoring skill.

- Benchmark reports separate GPU preparation/submission from blocking completion
  to diagnose tail latency without changing the end-to-end timing scope.

- Settings offer an eye button that fades the panel for live document previews.
- `markview ss validate FILE.mvss.toml` parses a stylesheet in place and
  reports its version and rule count, so a draft can be checked before install.
- The `markview-icon` `icon!` macro parses an SVG at compile time into a
  unit-box vector buffer, so the reader keeps its UI icons as editable files
  with no SVG parser in the binary.
- The reader exports the open document to PDF or a whole-document PNG from a
  toolbar button or `Ctrl+E`, with its own text size, indent, paper, margins,
  stylesheet sequence and PNG scale under `[export]` in `settings.toml`,
  independent of the reading view.
- The export panel's `Export and Watch…` action keeps rewriting the same file
  whenever the document is saved.
- The README sets the same text through a browser engine and through Markview at
  one measure, and `scripts/render_typography_comparison.py` reproduces the
  figure.
- The README also times opening a document against MarkText, with
  `scripts/compare_readers.py`, and one document to one PDF against Typst,
  Chromium and XeLaTeX, with `scripts/compare_pdf_engines.py`.

### Changed

- Prefer Medium (500) CJK faces throughout bundled themes, with inherited-weight fallback; refresh README screenshots.

- Enlarge Print page headers, footers and page numbers to 0.75em.

- Refresh Light/Dark palettes and move shared style defaults into the hidden, lowest-priority `builtin` sheet.
- Reorganize the MVSS guide with theme recipes, cascade rules and a validation workflow.

- Stylesheet panels use a matching vector arrow for the Back action.
- Redesigned reader chrome with flat, square controls and coordinated light/dark themes;
  shared, grouped settings and export panels scroll without shrinking their controls.

- The toolbar's Open and Settings buttons, and every panel's close button, are
  vector icons drawn from those buffers instead of text labels.
- The reader's export opens the written file with the operating system, and the
  export icon points out of its tray instead of into it.
- The export panel refuses to open without a document, and its header names the
  document and the resulting measure instead of repeating the rows.
- PDF export is set at 12 pt body text by default, on the command line and in
  the reader's export panel.

### Fixed

- Keep all three toolbar buttons visible while a panel or confirmation is open.
- Buttons now show distinct hover and pressed fills, including selected choices;
  keyboard-only focus replaces the border without stacking extra outlines.

## 0.1.3 - 2026-09-18

Themeable list markers and code chips, a watching PDF export, and exports that
pin their own fonts.

### Added

- An MVSS font candidate takes `synthetic_italic = true`, which shears an
  upright face by 14° when the family has no italic of its own. The bundled
  styles use it for CJK emphasis, so Chinese and Japanese text now slants
  instead of falling back.
- `--pdf … --watch` re-exports whenever the document or a local image it
  references changes, until the session is stopped. Rebuilds reuse the previous
  parse, block layout and decoded images; an unchanged save is skipped.
- An MVSS `fontdef` takes `emoji = true`, which marks the family as the face for
  Emoji text; the bundled reader and print styles use it.
- MVSS takes `align` and `shape` on list markers: `align` places a bullet,
  number or checkbox left, centered or right in its column, and `shape` draws a
  bullet as a disc, square, triangle, diamond, plus or minus. A `shape` list is
  cycled by bullet nesting depth; ordered levels do not advance it.
- MVSS takes `numbering` and `align` on `enum`, so a theme can format ordered
  numbers (`a)`, `I.`, `一、`, `①`) with Typst's numbering patterns and place
  them independently of bullets. The marker column grows to the widest number
  a list renders.
- MVSS takes `border_width`, `radius`, and `accent` on `task_marker`, so a
  theme can thicken, round and fill a task checkbox; a completed box fills with
  `accent` and draws its check in `color`. The bundled styles do.
- `--fonts DIR` (repeatable) adds a directory of font files, and
  `--ignore-system-fonts` shapes with those directories alone, so an export no
  longer depends on the fonts the machine happens to have installed.
- MVSS takes `padding` on `["code"]`, which insets an inline code chip: the
  horizontal sides widen the run and push its neighbours, and the vertical
  sides make the chip taller. The bundled styles pad code, which used to touch
  the text around it.

### Changed

- Bullets and task checkboxes are drawn shapes rather than text, so they are no
  longer selectable or copied; the bundled styles center them in their column.
- Inline code breaks between any two characters inside a run, free at a word
  edge and at a small penalty inside a word, so a long identifier wraps instead
  of overflowing its block. The run's edges keep ordinary break rules, so a
  following comma or closing bracket never starts a line.

### Fixed

- A task checkbox is drawn as an antialiased box with its check centered
  inside, like a list marker. The interior used to resolve transparent and the
  check used to sit outside the box, so a pending task read as a solid square.
- Emoji style is uniform again: a text family that happens to hold an Emoji
  symbol—Noto Sans CJK covering `⚠️`, say—no longer beats the configured Emoji
  face, which used to mix monochrome and color Emoji in one document.
- A blockquote's bar is centered on the text it frames: the quote's box no
  longer absorbs the outer spacing of its first and last child, which left the
  bar hanging far below a quote.
- Syntax colors no longer leak: a code comment painted every line after it in
  the comment color, because syntect was handed lines without their terminator.
- PDF export drew the CJK and emoji inside a formula's `\text{…}` group with
  the document's fonts instead of dropping them.
- A PDF's text map names every character of a ligature: an `fi` ligature used
  to extract as `f`, losing the `i` from copied or searched text.
- A PDF names every glyph of an ordered-list number, which used to leave a
  replacement character after the number when the text was copied.
- `--pdf --watch` recognizes equivalent spellings of one path, including a
  `..` over a symlinked directory such as macOS's `/var`.

## 0.1.2 - 2026-09-18

Paper export, faster first frames, and a smaller idle footprint.

### Added

- `--pdf FILE --output out.pdf` exports the document to paper: vector text with
  subset fonts, page breaking with widow and orphan control, page furniture, and
  link annotations.
- `--paper`, `--landscape`, `--margin`, `--header*`, and `--footer*` configure
  the page; a bundled `print` stylesheet supplies the defaults, and `--style`
  layers on top of it.
- `--title`, `--author`, `--subject`, `--keywords`, `--language`, and
  `--creator` fill the PDF information dictionary; a title defaults to the
  document's first heading, and unset fields stay out.
- MVSS adds a `[page]` table for paper, margins, and the header and footer
  slots, plus the `page`, `page_header`, `page_footer`, and `page_number`
  conditions.
- `scripts/compare_pdf_render.py` holds the PDF export and the GPU render of
  one document to the same content bands, alignment and profile overlap.
- Footnote references are clickable: a reference moves to its note, and the
  note's number moves back to the citation it was opened from.
- Consecutive footnote references share one bracket pair, as in `[1,2]`, and
  only the numbers stay click targets.
- MVSS adds a `footnote_ref` condition styling footnote references and the
  note's number.
- Math accepts LaTeX `\(...\)` and `\[...\]` delimiters alongside dollar signs.
- `settings.toml` takes a `[justification]` table bounding word spacing and
  letter spacing, so character-level justification can be tuned or turned off.
- `settings.toml` takes a `codeblock-wrap` boolean, also exposed under
  **Settings**, that hard-wraps code block lines at the reading column. The
  `--render` and `--smoke-test` image exports enable it by default.
- A CJK curly quote may start or end a line, as the full-width brackets already
  could, so a quoted phrase no longer glues a CJK run together.
- The Windows MSI registers Markview for `.md`, `.markdown`, and `.mdown`, so
  the reader joins **Open with** and **Default apps**; Windows 10 and 11 still
  ask the user to confirm the handoff once.
- `--bench-latency` measures process-entry first-frame latency, edit-to-refresh
  latency after a small on-disk edit, and the RSS trend across reloads.
- The latency and memory analysis documents the three targets, the responsible
  code, and ranked optimization points; stress fixtures and aggregation scripts
  make its scaling results repeatable.

### Fixed

- PDF pagination preserves multiline headings, includes line gaps in widow control,
  and fits tall images together with their leading space.
- PDF links resolve percent-encoded anchors and keep their hitboxes inside the
  printed text area.
- Heading anchors count suffixes in constant time, so a document that repeats a
  heading no longer parses quadratically (a 1 MiB repeated-heading file parses
  about 3.5× faster).
- A small edit no longer drops part of a list or paragraph: the incremental
  parser recognizes empty list items and Markdown's own blank-line rules
  (Unicode spaces are content, not blank lines).
- Reading counts are cached per content identity and sent with every complete
  update, so a second document with identical content still fills its footer.
- Closing the last tab releases the worker's parsed document, decoded images and
  layout caches, so an idle reader keeps nothing from the document it closed.
- `--render` and `--pdf` wait for the syntax highlighting pass, so exported
  code keeps its colors instead of only the text.
- The window sets its Wayland application ID, so a desktop with
  `markview.desktop` installed shows the Markview icon and groups the window
  with it instead of falling back to a placeholder.
- Compressing a line now moves the glyph with the blank half it spends, so an
  opening CJK bracket no longer overlaps the character after it.
- A quote break keeps the neighbouring prohibition, so a closing quote no longer
  hands a full stop to the next line and an opening quote no longer strands an
  opening bracket on the last.
- A tab-indented fenced code block inside a list no longer gains a leading space.
- A footnote's number is set at the note body's size and baseline in a column
  shared by every note, instead of floating above the text as a superscript.
- The diagnostic renders and benchmarks set CJK text in the configured face
  again: they never selected a `[cjk]` variant, so every Han cluster was drawn
  in a system fallback face. `--cjk-type` now names one on the command line.
- The Windows reader is linked for the Windows subsystem, so opening it no
  longer puts a console window on screen. A run with a command line attaches to
  the console it was launched from, and output with nowhere to go is dropped
  instead of panicking.
- Installing the Windows MSI over an already installed copy of the same version
  replaces it instead of leaving both registered, which a rebuild of a released
  version used to produce.

### Changed

- The minimum supported Rust version is 1.92, which the PDF backend requires.
- Opening a large file paints its first viewport from a bounded prefix parse
  instead of waiting for the whole file: a 4 MiB document shows its first
  readable frame in ~75 ms instead of ~175 ms, and reference definitions or
  footnotes later in the file still resolve in that first frame.
- A small edit re-parses only the block it changed, when the document is plain
  text, and reuses every other block. The reader's reading counts are computed
  on the layout worker rather than the event loop; together these cut a 1 MiB
  edit's time to the refreshed frame by about a third.
- The block cache survives passes and invalidates per block: a localized edit
  re-lays out only the changed block instead of the whole document, and the
  256-entry/100k-draw cap is gone.
- Syntax colors invalidate only the code blocks that gained them, instead of
  clearing the whole layout cache, and the highlight cache no longer clears
  itself at 256 entries. Both remove a permanent re-layout for code-heavy
  documents.
- The file-watch quiet window is 10 ms (was 30 ms) with a 40 ms ceiling, so an
  edit reaches the screen sooner while a save burst is still coalesced.
- System fonts are discovered once per process, and the scan runs on the worker
  while the window and renderer initialize, cutting the native first readable
  frame by about 12–14 ms.
- Justification spends word spaces and letter spacing first, then shares the
  remaining slack evenly, so a CJK line closes to the full measure instead of
  stretching one gap. Word spacing now follows Typst's two-thirds to
  three-halves limits.
- CJK punctuation gives back the blank half of its em box at a line start or
  end, following the convention the `cjk-type` setting names, and Han text
  gains a quarter em against Latin.
- A closing mark hangs part of its advance into the end margin, a hyphen is
  cheaper in the middle of a word than near either edge, and a last line that
  slightly overflows is compressed instead of wrapped.
- A paragraph is reflowed to avoid stranding a single word on its last line.
- An explicit `<br>` justifies the line it ends, while a hard break of two
  trailing spaces does not.
- Packaging builds run only when a packaging input changes, in a `Packages`
  workflow that no longer gates merges.
- Diagnostics go through `log` and `env_logger` as `LEVEL message` lines. The
  window logs at `warn` and the diagnostic modes at `debug`; `RUST_LOG` overrides
  both.
- Layout tests shape with pinned Noto subsets instead of host fonts, so the
  suite no longer passes on macOS and fails on Linux or Windows.
- The `comrak` patch points at upstream again, which now carries the fenced
  block offset fix the personal fork had supplied.

## 0.1.1 - 2026-09-16

A security release. Untrusted documents can no longer abort the process, read
arbitrary local images, or reach the local network.

### Security

- A Markdown file of deeply nested emphasis no longer aborts the process: every
  recursion and work allowance now comes from one shared `Limits` value, and
  pathological code blocks, formulas, tables, and paragraphs degrade instead of
  hanging.
- Image sources are relative to the document only. Absolute paths and `file:`
  URLs are refused, while `../` continues to work.
- Local links follow one policy. Markdown opens in the app, a reviewed inert
  allowlist (text, images, fixed-layout documents, audio, video) and directories
  go to the operating system, and everything else — including `.html` and every
  executable, script, or installer type — asks for confirmation first, defaulting
  to revealing the file in the file manager.
- Remote images are capped at 128 distinct sources per document revision, with a
  notice strip offering Dismiss and Load all. Both answers apply to the tab and
  revision they were chosen in, so opening another document shows its own notice
  and starts capped again.
  Loopback, private, and link-local addresses are refused after resolution and
  before connecting, and the resolved address is pinned so a rebind cannot
  bypass the check.

### Fixed

- Pasting Markdown whose first sentence ends in a multi-byte terminator, such as
  the CJK `。`, no longer panics: the tab title is cut on a character boundary.
- A failed or pending image typesets its placeholder through the paragraph
  engine, so the message wraps, justifies, and hyphenates like body text and
  fills the image box before its last line is elided. Previously the whole
  message was shortened to one line, so a box of any size rarely showed it.

### Documentation

- `docs/security.md` is revision 3: the implemented decisions, the shared
  `Limits` defaults, and the risks that remain accepted are recorded, and the
  open verification work is separated from it.

## 0.1.0 - 2026-09-16

An early development release. Expect breaking changes.

### Added

#### Rendering

- CommonMark headings, paragraphs, block quotes, lists, and emphasis, including
  CJK-friendly emphasis that closes next to CJK text.
- GFM tables and task lists, footnotes, GitHub-style alerts, links, and the raw
  HTML equivalents that Markdown documents use.
- Inline and display math rendered natively, with parsing diagnostics shown in
  the document.
- Asynchronous syntax highlighting for code blocks.
- Images from local files, `file:`, `http(s):` and `data:` URIs, in PNG, JPEG,
  GIF, WebP, BMP, ICO and SVG. An image alone in its block is centered, animated
  images show their first frame, and `--offline` blocks the network.

#### Typography

- Publication-quality paragraph layout with hyphenation and justification, plus
  a greedy comparison mode (`--greedy`).
- Optional paragraph indent, from Settings or `settings.toml`: the opening line
  of a prose paragraph indents, lists indent as a whole, and table cells and
  footnotes stay flush.
- Locale-aware font selection, including CJK fallback.
- Progressive layout, so a usable first frame appears before the whole document
  is laid out.
- Word counting by dictionary instead of by whitespace.

#### Reading

- Multi-document tabs, with drag-to-reorder, labels that shrink to keep the first
  two characters visible, wheel scrolling over an overflowing tab bar, and
  auto-scroll when dragging near either edge.
- Close the active tab with `Ctrl+W`, its × button, or the middle mouse button.
- `Ctrl+V` opens clipboard text that looks like Markdown in a new tab, titled
  from its first heading or sentence.
- Select by drag, by word on double click, or by block on triple click; `Ctrl+A`
  selects the document and `Ctrl+C` copies.
- Draggable scrollbars sized from the stylesheet, and horizontal scrolling for
  wide code blocks, tables, and formulas on hover or with `Shift`+wheel.
- Links open with the operating system's default handler. Other `.md` files open
  in a new tab, middle click opens them in the background and reuses an existing
  tab, and `#heading` fragments move to that heading, in the current document or
  in the `.md` file they name.
- Documents are watched for changes and re-rendered while you read them.
- The end of a document can be scrolled two thirds of a page above the window
  bottom.

#### Stylesheets and settings

- Built-in light and dark styles.
- MVSS `.mvss.toml` stylesheets with composable conditions, installed with
  `markview ss install FILE.mvss.toml` and selected with `--style ID`.
- A settings panel (`Ctrl+,`) backed by a live-reloading `settings.toml`.
- Command-line control over theme, alignment, hyphenation, type size, reading
  column, paragraph indent, window size, and scroll offset.

#### Diagnostics

- `--render FILE --output preview.png` renders through the real GPU pipeline
  offscreen, `--bench FILE` reports layout metrics, and `--smoke-test FILE`
  captures a window.

#### Packaging

- Release archives and installers: a `.tar.gz` and a shell installer on Linux, a
  `.zip`, a PowerShell installer and an `.msi` on Windows, and a zipped `.app`
  bundle on macOS, alongside a `.deb` and an AppImage.
- Application icons generated from the source SVGs by
  `cargo run -p xtask -- icons`, then installed into the window and into the
  Windows executable resources.
- Archives ship `THIRD_PARTY.md` and `licenses/`, and the Debian package and the
  macOS bundle carry a generated third-party notice.
- Every package is built and unpacked on every push, so a broken installer fails
  before a release can publish.

#### Documentation

- Guides for the architecture, the performance model, the security and threat
  model, development, stylesheets, and packaging under `docs/`.
