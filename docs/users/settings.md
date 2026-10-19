# Settings

[User documentation](README.md) · [Documentation](../README.md)

Open **Settings** with `Ctrl+,` (Command on macOS). Reading preferences belong
in `settings.toml`; visual themes belong in [stylesheets](stylesheets.md).

## Settings file

Desktop settings live at:

| Platform | File |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/settings.toml` or `~/.config/markview/settings.toml` |
| macOS | `~/Library/Application Support/markview/settings.toml` |
| Windows | `%APPDATA%/markview/settings.toml` |

Android stores settings in its private app files directory; see
[Android reading](android.md).

## Reading and interface preferences

- **Sidebar widths are independent.** Desktop and tablet Settings offer separate
  folder and outline widths; phones offer one shared sidebar width for Tabs and
  Folders, plus an independent outline width (200–600 logical pixels). Desktop users
  can also drag each sidebar’s inner edge. Widths persist as `folder-width`,
  `outline-width` and `tabs-width` in `settings.toml` (defaults: 260, 300 and 320).
  `tabs-width` controls the combined phone sidebar. Narrow windows limit the
  displayed width without changing the saved preference.

- **Tab style.** General settings offer Underline (default) and Connected tab styles, applied
  immediately. `tab-style` in `settings.toml` accepts `"underline"` or `"connected"`.

- **Window layout follows your preference.** General settings offer a dropdown
  with System, macOS, Windows and Linux layouts, applied next launch.
  The `window-layout` setting in `settings.toml` accepts `"system"`, `"macos"`,
  `"windows"` or `"linux"`. System uses native
  traffic lights on macOS, square window controls on Windows and chevrons on Linux.
  Right-side window controls share the toolbar's button size and spacing. A reserved strip beside
  the toolbar drags the window; controls keep the host system's behavior.
  Escape exits fullscreen after closing any open panel or confirmation.

- **Justification has limits.** A word space may shrink to two thirds or grow to
  one and a half of its own width, and letterfit may move by a hundredth of an
  em. Change them under `[justification]` in `settings.toml`, or set both
  tracking bounds to `0.0` to turn character-level justification off. Hyphenation
  is on by default.

- **Paragraph indent is off by default.** Choose it under **Settings**, or set
  `paragraph_indent` in `settings.toml`: prose indents its opening line while
  lists indent as a whole, and table cells and footnotes stay flush.

- **CJK is first-class.** The `cjk-type` setting (`SC`, `TC`, `JP` or `none`)
  picks the face and the punctuation convention together: a comma-like mark
  gives back its blank half at a line end on the mainland and in Japan, and is
  centred in Taiwan.

- **Scroll speed follows the desktop as far as it can.** Windows reports the
  system's lines and characters per notch, each applied to its own axis, and
  macOS scales its own deltas, so both are honored; a Linux detent carries no
  value and counts as three lines, and **Scroll speed** in **Settings**
  (`scroll-speed` in `settings.toml`, 0.5× to 2×) multiplies every wheel notch
  and arrow step.

- **Single instance is optional.** Enable **Single instance** in **Settings**, or set
  `single-instance = true` in `settings.toml`, to open files from subsequent
  launches in tabs of the existing window. It is off by default; files already
  open select their existing tab. Existing windows stay open when you enable it.

- **The interface follows the system language.** **Interface language** in
  **Settings** pins it to English, Simplified or Traditional Chinese, or Japanese instead.


## Markdown dialect

**Settings → Generic → Language & code** chooses how Markview reads Markdown.
The info control next to the dialect preset opens this page.

- **GFM (GitHub Flavored Markdown), the default.** Soft newlines stay spaces.
  Under this preset you can independently toggle single-newline hard breaks
  and `==highlight==` marks. See the
  [GFM specification](https://github.github.com/gfm/).
- **OFM lite (Obsidian-like, limited subset).** A restricted reading of
  [Obsidian Flavored Markdown](https://help.obsidian.md/obsidian-flavored-markdown):
  highlight marks are forced on and soft newlines stay spaces, so the two
  toggles stay visible but are disabled.

### What OFM lite reads

| Supported | Not supported |
| --- | --- |
| Callouts `> [!type] Title` with the built-in types and aliases, nesting, and foldable `+`/`-` bodies | Wikilinks `[[note]]`, embeds `![[note]]` and the vault model |
| `==highlight==` marks (forced on) | `#tags` and block references `^id` |
| Inline HTML `sub` and `mark` styling, and `u`, `ins`, `small` read as plain text, besides the usual `sup`, `b`, `i`, `code`, … | `%%` comments and other OFM-only syntax |

Callout titles are plain text, and an escaped, coded or styled `[!type]`
marker leaves the quote an ordinary quote.

```markdown
> [!warning]- Folded by default
> Body with ==mark== and <sub>sub</sub> text.
> > [!note] Callouts nest.
```

Exports follow the reader: in-app PDF and PNG export parse with the selected
dialect and unfold callout bodies. Command-line `markview export` uses the
default GFM dialect and does not load the reader's dialect settings.

## Network and image cache

Network images (`http:` and `https:`) are cached on disk between runs. A body
the server marks cacheable is reused until it goes stale, then revalidated with
a conditional request rather than downloaded again; `--offline` serves a cached
body without touching the network. The cache lives beside `settings.toml` (on
Linux, `~/.config/markview/cache/images`), holds at most 128 MiB with the least
recently used entries dropped first, and is cleared by deleting that directory.

Web pages and remote images use a browser-style `User-Agent` ending in
`Markview/<version>.0` and an `Accept-Language` derived from the interface language.
Web page requests also send `Accept: text/html`. Override the browser headers with
optional top-level keys in `settings.toml` (no UI controls):

```toml
user-agent = "Mozilla/5.0 (...) MyReader/1.0"
accept-language = "zh-CN,zh;q=0.9,en;q=0.8"
```

Omit these keys to follow the defaults. Settings reloads and interface language
changes apply to subsequent requests. Font downloads always identify themselves
as `Markview/<version>`.

## Links and local resources

Remote images can contact their servers when you open a document. Use `--offline`
to prevent image network requests. Relative local image paths can reach outside
the document folder. Links open only after a click; unfamiliar local file types
require confirmation, whose default action opens the containing folder.
See the [security policy](../developers/security.md) for limits and accepted risks.
