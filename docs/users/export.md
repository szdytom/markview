# Export PDF or PNG

Press Ctrl+E or use the toolbar's export button. Choose PDF or a single PNG of
the whole document, select a destination, then open the result in its system
viewer. **Export and Watch** updates that same destination when the source changes.

Export has its own paper, margins, font size, paragraph indent, PNG scale and
stylesheet selection, saved under `[export]`. Changing these does not alter the
reading view. Both formats use paper styles; see the [theme gallery](themes.md).

## Command line

```sh
markview pdf document.md --output document.pdf
markview pdf document.md -o paper.pdf --paper letter --margin 20,25
markview pdf document.md -o paper.pdf --footer "{title} — {page}/{pages}"
markview pdf document.md -o document.pdf --watch
```

The default paper style uses A4 and a centered page number. Use `--paper`,
`--landscape` and `--margin` to override geometry, and repeat `--style` with the
highest-priority style first. Header/footer templates accept `{page}`, `{pages}`,
`{title}` and `{path}`. Run `markview pdf --help` for all options and units;
command options follow the subcommand, while `--offline` is global.

`--watch` remains running and observes the document and local images. Ctrl+C
ends it. Disclosures expand for export; links become PDF annotations. Oversized
content can be scaled or reported on stderr. Inspect the output when changing
paper size or exporting unusually wide tables.

## Resource permissions

Reader exports preserve the document's mode and current grants. PDF fails when
an image cannot be read and preserves an existing output; PNG can retain an image
placeholder. Direct CLI file inputs default to Trusted. For an Untrusted file:

```sh
markview pdf article.md -o article.pdf --document-trust untrusted
markview pdf article.md -o article.pdf --document-trust untrusted --allow-local-image ./diagram.png
markview pdf article.md -o article.pdf --document-trust untrusted --allow-network loopback=http://localhost:8080
```

Grant options repeat. Network grants name an origin and address class (`private`,
`loopback` or `link-local`), not all destinations. Offline mode makes no network
connections and does not bypass cache authority. A content change revokes grants;
an Untrusted watch job needing them must restart with explicit authorization.

## Exporting an editor buffer

For unsaved editor content, JSON progress, cancellation, font assets and explicit
Markdown dialects, use the [native integration protocol](../library/native-export.md).
Buffer commands do not load personal reader settings; in-app exports use the
reader's selected dialect.
