# Read documents

Open a UTF-8 Markdown file with **Open file**, drag it into the window, or run:

```sh
markview notes.md
markview path/to/notes
```

Markview is read-only. Keep editing in your own editor; saved changes refresh the
reader while preserving your position. Paste Markdown with `Ctrl+V` to read it
in a new tab. See [Android](android.md) for shared files and folders on mobile.

## Navigate and copy

Use Contents (`Ctrl+B`) for headings and Find (`Ctrl+F`) for text. `/` starts an
empty query; Ctrl+F uses selected text or restores the previous query. Search is
literal, case-insensitive by default, with case and whole-word options. Typing
keeps your position; navigating a result opens enclosing collapsed sections.

Select and copy reading text with Ctrl+C. Code-block copy buttons copy the whole
block, including offscreen text and original whitespace. Wide code blocks and
tables scroll horizontally within the page.

Links to Markdown open reader tabs; middle-click opens them in the background.
Heading links use GitHub-style anchors; HTML `id` and legacy named anchors also
work. Other supported links go to the operating system. Unfamiliar local file
types ask for confirmation and offer to open the containing folder.

Drag tabs to reorder, use × or middle-click to close, and Ctrl+Tab to switch in
recent-use order. Each tab retains its search state. Session restoration saves
search visibility, query and options, but does not restore search input focus.

## Keyboard shortcuts

| Keys | Action |
|:--|:--|
| `Ctrl+O` | Open a file |
| `Ctrl+P` | List the open tabs, or search workspace and open-tab files once you type; use ↑/↓ to select, Enter to open, Esc to close |
| `Ctrl+Shift+O` | Open a folder |
| `Ctrl+Alt+R` | Show the active document's folder in the file manager |
| `/` | Start an empty document search |
| `Ctrl+F` | Find the selection or reopen the previous query |
| `Enter` / `Shift+Enter`, `F3` / `Shift+F3` | Next or previous search result |
| `Ctrl+B` | Open the table of contents |
| `Ctrl+T` | Open a blank page |
| `Ctrl+N` | Open a new window |
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
which stay on Control so they do not collide with the system app switcher.

## Folders and Obsidian vaults

Open a folder with **Open folder**, Ctrl+Shift+O, or a directory argument. The
sidebar lists `.md`, `.markdown` and `.mdown` files. The start page reopens recent
workspaces; Quick access and Ctrl+P find files by name or relative path.

Click folders to expand them and files to open tabs. External changes update the
tree; **Refresh** requests an immediate check. **Hide excluded entries** hides
hidden names and common build/dependency directories. Turning it off reveals
those entries, but does not follow symbolic links or read `.gitignore` rules.
If the root moved, open its new location. In narrow windows the sidebar overlays
the page and closes when you choose a document.

For an Obsidian vault, open its root. Supported links include `[[Note]]`,
`[[Note|Label]]`, `[[Note#Heading]]` and `[[#Heading]]`. Relative folder paths
resolve from the current document before the vault root. Bare names search the
current directory, root, then an unambiguous suffix in the tree; use a folder path
when names collide. `![[image.png]]` resolves from the open folder root, and a
pipe label is alt text, not an image width.

Note transclusion, block references, plugins and graph views are unsupported.
Markview does not modify notes or vault configuration. See
[Markdown dialect](settings.md#markdown-dialect) for parsing choices.

## Resource permissions

Directly opened local files use Trusted resource permissions: relative images
and ordinary intranet/VPN/localhost images can load automatically. Clipboard
and web content use Untrusted permissions. Public images can load in either mode.
Click a blocked image to approve its particular file or network target; pasted
relative images require selecting the actual file because pasted text has no folder.

An Untrusted link to local Markdown asks for that file and keeps it Untrusted.
Grants end when content changes or the tab closes and are not restored later.
**Load all** lifts the image-count cap, not resource permissions.
Use `--offline` to prevent network requests. See [network settings](settings.md#network-and-image-cache).

## Supported content

Markview reads tables, highlighted code, footnotes, alerts, math, images and a
restricted HTML subset. YAML front matter starts collapsed. Mermaid fences render
natively without a browser or external process. Animated images show a static
frame. Unsupported browser behavior, scripts and arbitrary HTML/CSS are outside
the reader's scope. The [examples](../../examples) show representative content.

## Web articles

```sh
markview web https://example.org/article
```

You can also paste an HTTP(S) URL. Web reading extracts static articles into
session-only tabs; links and images resolve against the final page URL. It does
not support JavaScript-dependent pages, authenticated browsing or offline page
loading. Failures appear in the tab. Opening a local web origin authorizes that
origin; unrelated local origins still need permission.

## Terminal behavior

Window launches detach by default. Add `--foreground` to keep the reader attached
and see startup errors/logs. Export, font and stylesheet commands run synchronously.
For output files, use [Export](export.md); for appearance, use
[settings](settings.md), [themes](appearance.md) and [fonts](fonts.md).
