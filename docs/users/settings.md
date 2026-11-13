# Configure reading

Open **Settings** with Ctrl+, (Command on macOS). Use settings for personal reading
preferences, [themes](appearance.md) for visual design and [Fonts](fonts.md) to
choose or download families. Export has its [own settings](export.md).

## Settings file

| Platform | File |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/settings.toml` or `~/.config/markview/settings.toml` |
| macOS | `~/Library/Application Support/markview/settings.toml` |
| Windows | `%APPDATA%/markview/settings.toml` |

Android keeps settings in private app storage. Prefer the UI for ordinary changes.
Saved settings override defaults; explicit command-line options override the
session. Diagnostic and buffer-export commands do not inherit personal reader
preferences. Use the command's `--help` for its exact options and defaults.

## Reading and interface preferences

| To change… | Use… |
| --- | --- |
| Type size or reading measure | Settings, or Ctrl++/Ctrl+- and Ctrl+[/Ctrl+] |
| Justification, hyphenation or paragraph indent | Reading preferences; advanced bounds live under `[justification]` |
| Chinese/Japanese fonts and punctuation conventions | `cjk-type`: `SC`, `TC`, `JP` or `none` |
| Folder, outline or phone sidebar width | Sidebar controls; desktop sidebars also have draggable edges |
| Wheel/arrow speed | Scroll speed; touch scrolling is independent |
| Window controls | Window layout; takes effect on the next launch |
| Interface language | Follow the system or select English, Simplified/Traditional Chinese or Japanese |
| Reuse an existing desktop window | Enable Single instance; later opens select or add tabs |
| Code-block copy buttons | Generic → Language & code → Code block copy button |

Code-block copying preserves the entire block's whitespace, including offscreen
content. Hiding its button (`codeblock-copy = false`) does not affect selection
copying or export. Phone/tablet-specific options are described in [Android](android.md).

## Markdown dialect

**Settings → Generic → Language & code** chooses how Markview reads Markdown.

- **GFM (GitHub Flavored Markdown).** You can toggle `==highlight==` marks.
  See the
  [GFM specification](https://github.github.com/gfm/).
- **Obsidian (the default).** A restricted reading of
  [Obsidian Flavored Markdown](https://help.obsidian.md/obsidian-flavored-markdown):
  highlight marks are forced on, so the highlight toggle stays visible but is disabled.

**Single newline as hard break** is independent of the Markdown preset and is off
by default. Switching to Obsidian in Settings turns it off; you can enable it
again in either preset. A saved `hard-breaks` choice also applies when reopening
the app or exporting a document.

**Grid tables** is independent of the preset and on by default. It supports
`+---+` grid borders, spanning cells and block content inside cells. Toggle it
in the same settings section, or set `grid-tables = false` in `settings.toml`.
The choice also applies to exports.

Settings store this preset as `markdown-flavor = "obsidian"`; the former
`"ofm-lite"` value is still accepted. An explicit `"gfm"` choice is preserved.

### What Obsidian reads

| Supported | Not supported |
| --- | --- |
| Callouts `> [!type] Title` with the built-in types and aliases, nesting, and foldable `+`/`-` bodies | Obsidian note transclusion `![[note]]` and full vault semantics |
| Wikilink image embeds `![[file.png]]` from the open folder root, with a pipe label read as alt text | Obsidian image width syntax and other non-image embeds |
| `==highlight==` marks (forced on) | `#tags` and block references `^id` |
| Inline HTML `sub` and `mark` styling, and `u`, `ins`, `small` read as plain text, besides the usual `sup`, `b`, `i`, `code`, … | Other OFM-only syntax |
| Hidden `%%inline comments%%` and line-start `%%` block comments | Callout title formatting (titles stay plain text) |

Wikilinks such as `[[note|label]]` and heading targets are supported by the core
reader in both presets. An open folder also resolves relative note links.

Comments disappear from reading, selection/copy, search and reader exports.
Paired markers on one content line hide inline text; a marker at the start of a
content line (also inside lists/quotes) can hide across blank lines until the next
unescaped `%%`, or to EOF when unclosed. Unmatched midline markers stay literal.
For example, `word%%note%%word` reads as `wordword`. Code spans/blocks, atomic
math, supported HTML code/pre elements and front matter metadata stay protected;
`\%%` escapes a marker. GFM keeps comment markers literal.

Raw HTML blocks and `<summary>` contents keep `%%` markers literal, as in
Obsidian. Markdown paragraphs between HTML blocks, separated by blank lines,
still hide comments; inline HTML tags in a Markdown paragraph do not prevent
comment parsing.

Callout titles are plain text, and an escaped, coded or styled `[!type]`
marker leaves the quote an ordinary quote.

```markdown
> [!warning]- Folded by default
> Body with ==mark== and <sub>sub</sub> text.
> > [!note] Callouts nest.
```

Exports follow the reader: in-app PDF and PNG export parse with the selected
dialect and unfold callout bodies. Command-line `markview export` defaults to GFM and does not load the reader's
dialect settings; use `--markdown-flavor`, `--hard-breaks` and
`--highlight-marks` to select them explicitly. Grid tables are enabled by default;
use `--grid-tables false` to disable them.

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
