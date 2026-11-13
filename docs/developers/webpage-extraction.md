# Web page extraction

`markview-webpage` converts an HTTP response into the same semantic document
used by the Markdown reader. It owns the DOM traversal, article metadata and
extraction limits; it does not depend on CommonMark, layout, the application,
`reqwest` or `tokio`.

## Entry points

```rust
let article = markview_webpage::extract(html, final_url)?;
let article = markview_webpage::extract_with(html, final_url, &profile)?;
```

`extract` uses `PROFILE_V1`, a `Profile` value. `WebArticle` contains a
`markview_document::Document` and `ArticleMetadata` (title, byline, site name,
language, dates, excerpt, lead image, canonical URL and the final URL). The
title is never empty: a page that declares none falls back to the final URL.
A leading body heading marked as the title stands in for it; a later heading,
even an `h1`, is content and the metadata title is still added.
The returned document has no `SourceMap`: reading a web page never pretends to
have original Markdown bytes.

## Profile and `PROFILE_V1`

`Profile` carries the extraction limits and the preserved classes. `extract`
uses `PROFILE_V1`; `Profile` has no defaults, so a new field must be set there.
Change the profile, its tests and the fixture expectations together.

The exact limits and preserved classes live in `PROFILE_V1` in
[the extraction entry point](../../crates/markview-webpage/src/lib.rs).
The profile bounds input, DOM growth, semantic depth, text, blocks and table
occupancy. Tests must check failures before excessive allocation. Changing a
limit or preservation rule requires updating fixtures and the front-end version
when persisted semantics change.

## Code block languages

Language declarations are resolved before Readability removes containers. The
nearest declaration wins; `code` overrides `pre`, which overrides its ancestors.
On the same element, `nohighlight`/`no-highlight` wins, followed by
`data-language`, `data-lang`, and language classes. Empty declarations are ignored.

Representative source formats covered by extraction tests:

| Source | Language metadata |
| --- | --- |
| [Prism](https://prismjs.com/#language-inheritance) | `language-*` / `lang-*`, including ancestor inheritance and `language-none` |
| [highlight.js](https://highlightjs.readthedocs.io/en/latest/readme.html) | `language-*`, common bare language names on `pre` / `code`, `nohighlight` |
| [Jekyll/Rouge](https://github.com/jekyll/jekyll/blob/master/lib/jekyll/tags/highlight.rb) | `code.language-*` and `data-lang` (including `c++`) |
| [GitHub](https://github.com/rouge-ruby/rouge) | `highlight-source-*` on the enclosing container |
| [Prettify](https://github.com/googlearchive/code-prettify) | `pre.prettyprint.lang-*` |
| [rehype-pretty-code](https://github.com/rehype-pretty/rehype-pretty-code/blob/master/packages/core/src/index.ts) | `data-language` on `pre` and `code` |

Common language aliases are normalized to native syntax tokens. Explicit plain
text and unsupported declarations stop inheritance and content detection. Without
a declaration, only recognized shebang interpreters supply a language; ambiguous
snippets and Shiki output containing only inline colors remain unclassified.
Token spans contribute their text, while Markview supplies its own syntax colors.
Line breaks written as `br` elements inside `pre` are converted to literal
newlines before Readability runs, preserving code indentation and blank lines.

## Semantic preservation

Before Readability runs, the adapter copies language classes into
`data-mv-language`, cell alignment into `data-mv-align`, checklist state into
`data-mv-checked`, records math classes and TeX annotations in `data-mv-*`
attributes, and turns `script[type^="math/tex"]` into a marked `span`.
Readability removes classes, presentational attributes, inputs and scripts; the
marked attributes are the private contract the lowering reads afterwards. One
walk over the checkboxes marks every owning list item, so nested lists cost
their depth instead of a scan per item. The element limit is checked before
these passes, so an oversized page fails before any of them scans it.

Formula copies that exist only for display are skipped, so a page yields one
formula instead of the formula plus its visual text. `aria-hidden="true"` always
hides a subtree, including a caption, summary or definition term root.
`role="presentation"` only removes ARIA semantics, so it hides
text inside formula markup and nowhere else: a presentational paragraph or table
cell keeps its text. A MathML formula uses its TeX annotation when one exists,
then its `alttext`; the rendered glyph text is never read as TeX and stays plain
text with a diagnostic when neither is present. TeX delimiters are stripped from
every formula source while they still decide display mode.

The lowering walks the processed DOM directly into `DocumentBuilder`. It does
not serialize the body to HTML, Markdown or plain text. Unknown containers are
transparent; script/style and other non-reading content is ignored. Code blocks,
lists, block quotes, tables, definition lists, figures, disclosures, images,
links and formulas become semantic nodes. An anchor that wraps whole blocks
applies to every field below it, including figure captions, disclosure summaries
and callout titles. Definition-list wrappers are walked depth-first, so nested
wrappers keep document order and count against the semantic depth limit.

Images become `ImageSpec` values whose source is
`ImageSource::Resource(ResourceRef::document(address))`; the address is the
absolute URL resolved against the page's base or final URL, and an inline SVG
becomes a `data:image/svg+xml` resource. A `code.language-mermaid` block becomes
`ImageSource::Diagram(Diagram { language: "mermaid", source })`, not an image
URL. Links live on `Inline::link`, separate from `TextStyle`, with an occurrence id from the lowering's own counter;
they are not compared by `Eq`, and a block-level anchor shares one occurrence
across the runs it covers. The document model performs no fetching, resource
authorization or cache-key construction, which stay in the application.

`DocumentBuilder::finish` seals the tree and builds the document index once.
The body check, title and source line share the same builder before sealing.
The index records structural paths, outline, disclosure defaults and anchor-to-enclosing-
disclosures. Later lowering and reading code query that index through
`Document`, so a web document gets the same identities as a Markdown one even
though it has no `SourceMap`.

HTML whitespace uses the HTML rule: ASCII whitespace collapses, non-breaking
spaces remain. Entities are decoded by the HTML parser. `$`, backslashes and
pipes are plain text and are never reinterpreted as Markdown.

Tables build an occupancy grid: `thead`/`tbody`/`tfoot` become row groups, and
`rowspan`/`colspan` become positive spans. Groups are emitted as the head, then
every body, then the foot, which is the order HTML renders them in. `rowspan=0`
means the remaining rows of the current group. Invalid spans become one, spans
are clipped to the group and column count, and exceeding any table budget
returns a clear error rather than allocating an unbounded grid. A cell whose
declared span collides with an earlier cell is placed as a plain cell in the
next free slot instead of being dropped. A cell's `align` becomes its own
alignment and, when every cell of a column agrees, the column's alignment too.

## Empty bodies

A `ReadabilityError::GrabFailed` is an extraction failure. After extraction
succeeds, the body is considered readable when it contains a non-blank text,
code or formula field, or an image. Anchors, empty containers, whitespace and
rules alone do not count. The artificial title and source line are added only
after this check.

## dom_smoothie patch

Cargo configuration owns the pinned extractor revision. The adapter requires a
processed-DOM entry point so lowering can avoid HTML/Markdown serialization.
Remove a patch only when the locked upstream release provides equivalent behavior.

```sh
cargo test -p markview-webpage --locked --test extract
```

The parity tests compare processed output and metadata against legacy extraction.
The [historical benchmark](history/webpage-extraction.md) records the direct-lowering
migration; use [paired measurements](performance.md) for new performance changes.

When upgrading `dom_smoothie`, lock the new version, inspect the adapter types
and the complete `Config`, run the `markview-webpage` tests, compare the
extracted block/text metadata on the fixtures, and measure extraction plus
conversion before accepting the change.
