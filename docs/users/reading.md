# Reading documents

[User documentation](README.md) · [Documentation](../README.md)

| Keys | Action |
|:--|:--|
| `Ctrl+O` | Open a file |
| `Ctrl+Shift+O` | Show the active document's folder in the file manager |
| `/` | Start an empty document search |
| `Ctrl+F` | Find the selection or reopen the previous query |
| `Enter` / `Shift+Enter`, `F3` / `Shift+F3` | Next or previous search result |
| `Ctrl+B` | Open the table of contents |
| `Ctrl+T` | Choose a stylesheet |
| `Ctrl+E` | Export the document |
| `Ctrl+,` | Open settings |
| `Ctrl++` / `Ctrl+-` | Larger or smaller type |
| `Ctrl+[` / `Ctrl+]` | Narrower or wider reading column |
| `Ctrl+V` | Read Markdown or an HTTP(S) web article from the clipboard in a new tab |
| `Ctrl+W` | Close the tab |
| `Ctrl+A` / `Ctrl+C` | Select the document, or copy the selection |
| Wheel, arrows, `Page Up`/`Page Down`, `Space`, `Home`/`End` | Scroll |

macOS uses Command in place of Ctrl. The reading column defaults to 760 logical
pixels and the type to 18.

- **Opening is flexible.** Launch with no file for an empty window, drop a
  Markdown file onto it, or paste Markdown from the clipboard; the file is read
  as UTF-8, a BOM included.

- **Tabs behave.** Drag a tab to reorder it, close one with its × button or
  the middle mouse button, and scroll an overflowing strip with the wheel.
  Choose the tab appearance in [settings](settings.md).

- **Links open where they should.** Web, mail and local files go to the
  operating system's default handler; links to other `.md` files open in a new
  tab, and middle-click opens them in the background. A `#heading` fragment
  moves to that heading, in this document or in the file it names.

- **The file is watched.** Edit it in your own editor and Markview repaints in
  place, keeping your position unless you were already at the end.

- **Scrolling is eased.** Page Up/Down, `Space`, `Home`/`End`, the arrow steps,
  the wheel, a click on the scrollbar track and a `#heading` jump ease over
  120–400 ms; a wheel turned against the motion still in flight takes over from
  where the page is rather than finishing it first. A thumb drag and every other
  scroll stay immediate.

- **A hard break stays hard.** Two trailing spaces leave the line at its natural
  width; an explicit `<br>` asks for the line it ends to be set flush.

Markview is deliberately read-only: it does not edit or save Markdown. Its
multi-document workspace consists of reader tabs. Use the table of contents and
search to navigate within a document. Printing uses the export panel or
`markview pdf`. Links address headings by their GitHub slug; raw HTML `id` attributes are not
interpreted, so an explicit anchor is not a link target.

## Supported content

Tables keep their alignment, fenced code is highlighted, footnotes are numbered
and clickable, GitHub alerts keep their meaning, and images — PNG, JPEG, GIF,
WebP, BMP, ICO or SVG, with an animated image showing its first frame — sit
inline or centred. `---` fenced YAML front matter is kept as metadata: it starts
collapsed under a `Frontmatter` label, and opening it shows the source as a
highlighted `yaml` block. A `mermaid` fenced block becomes a diagram: flowcharts,
sequence diagrams and the other supported types are laid out and rasterized in
Rust, so they need no browser, network or external process. Links to other
Markdown files open in new tabs, so a folder of documents behaves like one.
Everything can be selected and copied, and any block too wide for the column
scrolls on its own.

## Web articles

Experimental web reading is available with `markview web https://example.org/article`
(or paste an HTTP(S) URL with Ctrl+V). It downloads static UTF-8 HTML in the
background, extracts the article as Markdown, and opens
it in a temporary native reader tab created immediately with a loading message.
Loading errors stay in the tab’s reading area; background completions update
their original tabs without changing focus or reopening closed tabs.
Links and images resolve against the final
page URL. Reopening the same URL selects its existing tab; repeated requests
while it is loading share the download. These tabs last for the current session. JavaScript pages, authenticated content and offline page loading are
not supported. Web launches open their own window; other links still open in the
system browser. Existing network address restrictions and download limits apply.

For preferences and network behavior, see [settings](settings.md).
For output files, see [export](export.md).
