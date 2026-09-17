# Markview

Markview is a native, read-only Markdown reader for people who want a calm reading surface instead of a browser tab. It renders Markdown, math, code, tables, links, and images in a desktop window without a browser, WebView, JavaScript, or an external TeX process.

Markview is a fast, native Markdown reader with multi-threaded processing, GPU-accelerated rendering, and low memory usage, bringing publication-quality typography to your documents.

## Install

Download the latest build from [Releases](https://github.com/szdytom/markview/releases):
a `.deb`, an AppImage, or a `.tar.gz` archive on Linux; an `.msi` or `.zip` on
Windows; a zipped `.app` bundle on macOS. On Linux and macOS the install script
does the same thing:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/szdytom/markview/releases/latest/download/markview-installer.sh | sh
```

Linux builds need glibc 2.35 or newer, `libfontconfig1`, a working Vulkan
driver, and a desktop portal for file dialogs. The macOS bundle is unsigned, so
clear the quarantine flag once after downloading it:

```sh
xattr -d com.apple.quarantine /Applications/Markview.app
```

Per-platform details and the exact artifact list are in
[the packaging guide](docs/packaging.md).

## Try it

Building from source requires Rust 1.88 or newer, system fonts, and a working
Vulkan, OpenGL, Metal, or Direct3D 12 driver.

```sh
cargo run --release -- examples/welcome.md
cargo run --release -- /path/to/document.md
```

Launching without a file opens an empty window. You can also drop a Markdown file onto the window or use **Open**. Markview reads UTF-8 Markdown (including UTF-8 BOM) and watches the file for changes, which makes it useful beside an editor.

On Debian or Ubuntu, the native build commonly needs:

```sh
sudo apt-get install libfontconfig1-dev libxkbcommon-dev libwayland-dev fonts-noto-core fonts-noto-cjk
```

## Reading

- `Ctrl+O` opens a file; `Ctrl+T` chooses styles; `Ctrl+,` opens settings.
- `Ctrl+V` opens clipboard text that looks like Markdown in a new tab. The tab title comes from its first heading or sentence.
- `Ctrl++` / `Ctrl+-` changes the type size. `Ctrl+[` / `Ctrl+]` changes the reading column.
- Paragraph indent is off by default. Choose it under **Settings**, or set `paragraph_indent` in `settings.toml`; prose paragraphs indent their opening line, while lists indent as a whole, markers included. Table cells and footnotes stay flush.
- Justification starts from Typst's limits: a word space may shrink to two thirds and grow to one and a half of its own width, and letter spacing may move by a hundredth of an em. Change them under `[justification]` in `settings.toml`, where `spacing_min` and `spacing_max` are fractions of a space and `tracking_min` and `tracking_max` are in em. Setting both tracking bounds to `0.0` turns character-level justification off.
- The `cjk-type` setting (`SC`, `TC`, `JP`, or `none`) also picks the CJK punctuation convention: a comma-like mark gives back its blank half at a line end on the mainland and in Japan, and is centered in Taiwan.
- A hard break (two spaces at the end of a line) leaves its line at its natural width. An explicit `<br>` asks for the line it ends to be set flush like any other.
- Scroll with the wheel, arrow keys, Page Up/Down, Space, Home, End, or the scrollbar.
- Drag to select and use `Ctrl+C` to copy. `Ctrl+A` selects the document.
- Click a link to open web, mail, and local file links with the operating system's default handler. Links to other `.md` files open in a new tab; middle-click opens them in the background without switching away. Repeated middle-clicks reuse the existing tab. A `#heading` fragment moves to that heading, in the current document or in the `.md` file it names.
- Drag a tab horizontally to reorder it. Tabs shrink to keep at least the first two characters visible; when they overflow, scroll over the tab bar with the mouse wheel or trackpad. Dragging near either edge scrolls the strip automatically. Close a tab with `Ctrl+W`, its × button, or the middle mouse button.
- Hover over a wide code block, table, or formula to scroll it horizontally. Turn on **Code block wrapping** under **Settings**, or set `codeblock-wrap` in `settings.toml`, to hard-wrap code lines at the reading column instead.

macOS uses Command in place of Ctrl. The default reading column is 760 logical pixels and the default text size is 18 logical pixels.

## Supported content

Markview supports CommonMark headings, paragraphs, quotes, lists, emphasis (including CJK-friendly emphasis that closes next to CJK text), code blocks, GFM tables and task lists, footnotes, GitHub-style alerts, links, raw HTML equivalents, inline and display math, and local or remote images. Images can be PNG, JPEG, GIF, WebP, BMP, ICO, or SVG; animated images show their first frame.

The reader is intentionally read-only. It does not edit or save Markdown, provide a table of contents or search, print, or provide a multi-document workspace beyond tabs opened from Markdown links. Links address headings by their GitHub slug; raw HTML `id` attributes are not interpreted, so an explicit anchor is not a link target. See the [documentation map](docs/README.md) for behavior and implementation boundaries.

## Customize

Use the built-in light and dark styles, or install a `.mvss.toml` stylesheet:

```sh
markview ss install paper.mvss.toml
markview document.md --style paper
```

The [stylesheet guide](docs/stylesheets.md) explains the format and its supported conditions.

## Development

The project is a Rust workspace. Start with the [development guide](docs/development.md); the [architecture](docs/architecture.md) explains the boundaries that changes should preserve.

Markview is MIT-licensed. Third-party notices are in [THIRD_PARTY.md](THIRD_PARTY.md).
