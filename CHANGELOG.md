# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!--
cargo-dist takes the H2 section whose version matches the tag as the GitHub
Release notes, and its heading as the release title. Keep one H2 per release,
at the same level, without `[brackets]`.
-->

## Unreleased

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
