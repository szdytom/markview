# Export

[User documentation](README.md) · [Documentation](../README.md)

Markview exports without a browser or a print dialog. In the reader, `Ctrl+E` or
the toolbar's export button opens an export panel: it writes the document to
PDF, or to one PNG of the whole document, and opens the result with the
operating system. **Export and Watch…**, beside it, keeps rewriting the same
file whenever the document is saved. The panel carries its own text size
(12 pt by default), first-line
indent, paper, orientation, margins, PNG scale and stylesheet sequence — the
bundled `print` sheet is layered with whatever the panel selects — all kept
under `[export]` in `settings.toml`. Changing them never reflows the reading
view.

The same exports are on the command line, for scripts and batch runs:

```sh
markview pdf document.md --output document.pdf
markview pdf document.md -o paper.pdf --paper letter --margin 20,25
markview pdf document.md -o paper.pdf --footer "{title} — {page}/{pages}"
markview pdf document.md -o document.pdf --watch
```

The bundled `print` stylesheet supplies the paper: A4 with 20 mm side margins,
black on white, and a centred page number. Body text is 12 pt unless
`--font-size` says otherwise. `--paper` takes `a3`, `a4`, `a5`, `a6`, `b5`,
`letter`, `legal`, `tabloid`, or `WIDTHxHEIGHT` in millimetres; `--margin` takes
one, two, or four millimetres; `--landscape` swaps the sides.
The six header and footer slots are set with `--header`, `--footer` and the
`-left`/`-right` variants, and their templates may use `{page}`, `{pages}`,
`{title}` and `{path}`. PDF commands use `--paper` and `--margin` for page geometry
and `--style` for colors; window dimensions, reading column and reader theme
flags do not apply. Put command options after the subcommand; only `--offline`
is global. The `render` command alone accepts `--scroll`, in logical pixels.

`--watch` keeps the command running after the first export and rebuilds the PDF
whenever the document, or a local image it references, changes; Ctrl+C ends the
session. Every rebuild reuses the unchanged parse, block layout and decoded
images, so an unchanged save is skipped and a small edit pays only for the part
that changed.

A paragraph keeps two lines on each side of a page break, a heading travels with
the block it introduces, code blocks wrap, and a table too wide for the page is
scaled down with a warning on stderr. Web and mail links become clickable
annotations, and a `#heading` link becomes an internal jump.

The PDF information dictionary takes `--title`, `--author` (repeat it for
several authors), `--subject`, `--keywords`, `--language` and `--creator`.
Nothing else is invented, and no creation or modification date is ever written,
which is what keeps two exports of one document byte for byte identical.

## Resource permissions

Exports from the reader preserve the document's resource permissions. Directly opened local files use Trusted defaults; clipboard and web exports keep their Untrusted mode and current grants. PDF export fails when an image cannot be read, leaving any previous output intact; PNG keeps a placeholder for the blocked image.

CLI PDF inputs default to Trusted. For an Untrusted local input, select its mode and authorize only the required resources:

```sh
markview pdf article.md -o article.pdf --document-trust untrusted
markview pdf article.md -o article.pdf --document-trust untrusted --allow-local-image ./diagram.png
markview pdf article.md -o article.pdf --document-trust untrusted --allow-network loopback=http://localhost:8080
```

Both grant options repeat. A network grant names an origin (scheme, host, effective port) and a class: `private`, `loopback`, or `link-local`. It does not authorize other origins or address classes. `--offline` opens no network connections and grants no additional cache access. Grants apply to the initial document content; a changed source in `--watch` revokes them, so an Untrusted job needing them must be restarted with explicit grants.
## Exporting an editor buffer

`markview export` accepts Markdown on stdin, so an editor can export unsaved
changes without creating a Markdown file:

```sh
markview export --stdin --format pdf --output snapshot.pdf --base-dir . < document.md
markview export --stdin --format png --output whole.png --scale 2 < document.md
```

Buffers default to Untrusted. The host must preserve the document's trust using
`--document-trust trusted|untrusted` and may supply the same `--allow-local-image`
and `--allow-network` grants described above. A resource directory does not grant
trust.

`--base-dir` resolves relative images (default: the current directory).
`--style` selects a bundled PDF template; `--style-file` selects a custom MVSS
file targeting `pdf`. Both output formats use its page geometry. `--font-size`
is in logical pixels (default: 16). Repeat `--fonts` for font directories or
`--font-file` for explicit font files. Explicit files replace system discovery.

Stdout contains one JSON object per line: `progress` events with a `phase` and
`fraction`, followed by a `done` event with PDF page/byte counts or PNG dimensions.
Errors go to stderr and return a nonzero exit status. A host can create the
path supplied by `--cancel-file` to request cancellation at export checkpoints.
Hosts that require cancellation to preserve the destination should export to
a temporary output, wait for successful completion, and then replace it.
