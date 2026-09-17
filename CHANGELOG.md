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

### Added

- Footnote references are clickable: a reference moves to its note, and the
  note's number moves back to the citation it was opened from.
- MVSS adds a `footnote_ref` condition styling footnote references and the
  note's number.
- Math accepts LaTeX `\(...\)` and `\[...\]` delimiters alongside dollar signs.

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
