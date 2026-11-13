# Choose and download fonts

Markview uses installed fonts and can download families declared by a stylesheet.
Reading a document or installing a theme never downloads fonts automatically.

## Choose the reading font

Open **Settings → Fonts → Set fonts**. Choose a family for serif, sans-serif or
monospace text; Han roles appear when a CJK convention is enabled. **Default**
restores the stylesheet's fallback chain. Changes reflow the document immediately.
Downloading a family only makes it available; it does not choose it for a role.

## Download missing families

The Fonts page shows availability, size, license and declaring stylesheets.
Filter to **Missing**, then download a family or use **Download Missing**.
**Download All** also stores copies of system-provided families. Individual
transfers can be cancelled. **Open fonts folder** reveals downloaded files on desktop.

```sh
markview fonts list
markview fonts download
markview fonts download noto-sans-cjk-sc
markview fonts path
markview fonts verify
```

Use `--style ID` or `--file SHEET.mvss.toml` to select declarations from a particular
style. `--dry-run` previews a download; `--force` fetches existing families again.
Use `markview fonts download --help` for transfer options. Offline mode refuses
transfers but permits listing and verification.

Downloads live in `fonts/` beside [settings.toml](settings.md#settings-file).
The reader and normal exports use them. For controlled output,
`--ignore-system-fonts` excludes both system and downloaded fonts; supply explicit
font files/directories instead. Diagnostic benchmarks isolate their font inputs.

## Missing characters or unexpected faces

Choose a family covering the script and the required weight. Han selectors filter
out Latin-only families. A full CJK collection can be much larger than a regional
subset; download only the coverage you need. A theme's fallback chain still
matters for symbols, emoji, italic and bold text.

Use the page's license/source information before downloading an unfamiliar
stylesheet's fonts. For theme authors, see [downloadable font declarations](stylesheets.md#downloadable-fonts).
For editor-managed caches and JSON progress, see [native integration](../library/native-export.md#editor-integrations).
