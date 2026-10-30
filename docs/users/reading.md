# Reading documents

[User documentation](README.md) · [Documentation](../README.md)

| Keys | Action |
|:--|:--|
| `Ctrl+O` | Open a file |
| `Ctrl+Shift+O` | Open a folder |
| `Ctrl+Alt+R` | Show the active document's folder in the file manager |
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
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Switch tabs in recently-used order |
| `Ctrl+A` / `Ctrl+C` | Select the document, or copy the selection |
| Wheel, arrows, `Page Up`/`Page Down`, `Space`, `Home`/`End` | Scroll |

macOS uses Command in place of Ctrl, except for `Ctrl+Tab` and `Ctrl+Shift+Tab`,
which stay on Control so they do not collide with the system app switcher. Each
tab keeps its own search bar, query, match options and input focus; the session
restores them on startup. The reading column defaults to 760 logical pixels and
the type to 18.

`markview`, `markview FILE`, and `markview web URL` open the reader in the
background and return control to the shell immediately. Use `--foreground`
to keep the reader attached to the terminal and see its logs, for example
`markview --foreground notes.md` or `markview web URL --foreground`.
Export, stylesheet, font and diagnostic commands run synchronously.
If the background reader fails to start, a native error dialog shows the cause,
including stylesheet, window and GPU initialization failures. `--foreground`
reports these errors in the terminal instead.

- **Opening is flexible.** Launch with no file for an empty window, drop a
  Markdown file onto it, or paste Markdown from the clipboard; the file is read
  as UTF-8, a BOM included.

- **Tabs behave.** Drag a tab to reorder it, close one with its × button or
  the middle mouse button, and scroll an overflowing strip with the wheel.
  Closing the active tab returns to the one you used before it. Hold Ctrl and
  press Tab repeatedly to walk the recent order without committing until Ctrl
  is released; Ctrl+Shift+Tab walks the other way. Choose the tab appearance in
  [settings](settings.md).

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
`markview pdf`. Links address headings by their GitHub slug and HTML elements by their `id`
(or legacy `<a name="…">`). Empty anchors take no space; jumps into collapsed
`<details>` expand the enclosing sections. Duplicate targets resolve to the
first occurrence.

## Folders and Obsidian vaults

Use **Ctrl+Shift+O** or click the top-left sidebar button before loading a folder
to choose one; you can also pass a directory to `markview`. The toolbar's **Reveal**
button opens the current document's folder in the system file manager.
Markview lists `.md`, `.markdown` and
`.mdown` files in a left sidebar, preserving subfolders. Click a directory to
expand or collapse it and a file to read it in a tab. Subfolders start
collapsed; refreshing preserves the folders you expanded. Existing tabs are
reused. With the sidebar open, picking a tab expands the folders around its
document and scrolls that row into view. The sidebar scrolls independently; Tab
focuses its controls, arrow keys move between file rows, and Enter opens the
selected row. Its header shows the open folder's name and can collapse all
folders or refresh the tree after files are added, moved or deleted.
**Collapse all** also clears nested expansion and returns the tree to the open
folder's top-level entries, which stay visible; use **Ctrl+Shift+O** to choose
another folder.
Hidden entries such as `.obsidian` and `.git`, and symbolic links, are omitted.
Inaccessible subfolders are skipped; an unreadable root reports an error.

The file sidebar spans the full left edge on desktop and tablets; tabs and
the main toolbar start to its right, while the outline keeps its right-side
position. Wide windows reserve space for the sidebar. In narrow windows it overlays the
page and closes after choosing a document. Phones put the file tree and tab
list in one sidebar with a Tabs/Folders switch. On desktop and tablets, the
top-left sidebar button expands or collapses the file tree and stays to the
left of all tabs when collapsed. Session restoration retains the folder as
well as open documents
and reading positions.
An empty folder stays open so it can be refreshed after documents are added.
When both drawers are open, narrowing the window keeps the outline and closes
the file sidebar.

Select an Obsidian vault's root folder to browse its notes. Basic internal
links support `[[Note]]`, `[[Note|Label]]`, `[[Note#Heading]]` and
`[[#Heading]]`, along with Markdown links with or without `.md`. Folder paths
are resolved from the current document's directory before falling back to the
vault root. A bare note name is looked up in the current directory, at the root,
then throughout the tree when its suffix identifies a single note. Use a folder
path for duplicate names.
Markview remains read-only: it does not change notes or Obsidian configuration.
Image embeds such as `![[image.png]]` read as images from the open folder's
root. A `|` label becomes alt text; without one the image keeps an empty alt
text and no caption. Obsidian block references, note transclusion
(`![[Note]]`), plugins and graph views are not supported.

## Resource permissions

Local files you open directly use Trusted resource permissions: supported relative images and ordinary intranet, VPN, or localhost images load automatically. Clipboard text and web articles use Untrusted permissions. Public images load in either mode; click a blocked image to approve its specific file or network destination. A relative image in pasted text asks you to choose the actual image file, because pasted text has no local directory.

Opening a local Markdown link from Untrusted content asks for that document and keeps its Untrusted permissions. Authorizing a resource never changes the document's mode. Grants end when its content changes or its tab closes and are not saved in restored sessions. Load all lifts the image count limit and does not grant resource access.

Opening a local web URL authorizes that origin for the page and its images; other local origins need separate approval. HTTP requests use direct connections and ignore system proxies. Both modes retain the same processing limits and OS-handler confirmations.

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
