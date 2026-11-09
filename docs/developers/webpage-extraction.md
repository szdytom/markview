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

| field | value |
| --- | --- |
| `max_input_bytes` | 32 MiB |
| `max_elements` | 100,000 |
| `max_dom_nodes` | 400,000 |
| `max_semantic_depth` | 256 |
| `max_text_bytes` | 32 MiB |
| `max_blocks` | 100,000 |
| `max_table_rows` | 16,384 |
| `max_table_columns` | 256 |
| `max_table_cells` | 131,072 |
| `preserved_classes` | `math`, `inline`, `display`, `katex`, `MathJax`, `mathjax` |

Readability's remaining settings are fixed in `extract_with` and are not part
of the profile: `candidate_select_mode` is `Readability`, `char_threshold` is
500, `n_top_candidates` is 5, `min_score_to_adjust` is 5.0, `disable_json_ld`
and `keep_classes` are false, `readable_min_score` is 20.0,
`readable_min_content_length` is 140 and `text_mode` is `Raw`. Only
`max_elements_to_parse` and `classes_to_preserve` come from the profile.

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

## Consumer migration

- Read navigation from `Inline::link` or `Inline::link_url()`. Compare
  `TextStyle` for appearance, and compare link occurrences separately when
  joining runs. `Link::new` accepts a `LinkOccurrenceId`; source-coordinate
  allocation belongs to the front end.
- Store `TextSelection` endpoints as `SemanticPosition`. Obtain them through
  hit testing, search selection or `LayoutSnapshot::semantic_at`; resolve them
  through the current snapshot for immediate geometry work. Presentation
  counters never replace `SemanticPosition::revision`.
- `SemanticPosition::projection` preserves generated reading text relative to
  a semantic field. Source navigation uses `offset`; it does not invent source
  ranges for diagnostics, list markers or placeholders.
- A persisted reading continues to use session schema 4: input fingerprint,
  structural path and semantic field offset. A process-local `BlockId`, a
  borrowed geometry adapter and a reading projection are not persisted.

## Benchmark

`examples/webpage_bench.rs` replays the retired Readability-to-Markdown round
trip beside direct lowering, using the current model for both. Its
`replay.json` isolates extraction changes. `scripts/webpage_baseline.rs` and
`scripts/webpage_baseline_rss.rs` measure the actual extraction, model and layout
at `4328b6246ac6deed0243bff3869fef1dcbc74d2b`. They are measurement adapters,
not production dependencies. Create a detached worktree and copy only the
adapters, shared probe helpers and fixtures into it:

```sh
git worktree add --detach /tmp/markview-wp-baseline 4328b6246ac6deed0243bff3869fef1dcbc74d2b
mkdir -p /tmp/markview-wp-baseline/examples/webpage_bench
cp scripts/webpage_baseline{,_rss}.rs /tmp/markview-wp-baseline/examples/
cp examples/webpage_bench/{fonts,rss}.rs /tmp/markview-wp-baseline/examples/webpage_bench/
cp tests/fixtures/web-*.html /tmp/markview-wp-baseline/tests/fixtures/
cargo build --manifest-path /tmp/markview-wp-baseline/Cargo.toml --release --locked --example webpage_baseline --example webpage_baseline_rss
cargo build --release --locked --example webpage_bench --example webpage_rss --example webpage_replay_rss
python3 scripts/webpage_refactor_evidence.py
python3 scripts/benchmark_webpage_refactor.py new-run --baseline-checkout /tmp/markview-wp-baseline --cpu 2
python3 scripts/compare_webpage_bench.py artifacts/webpage-refactor/baseline.json artifacts/webpage-refactor/after.json
```

Both versions use a fixed 640×800 viewport, three warmups and twenty samples.
The first viewport metric stops when published height covers the viewport or
the whole document is complete. It measures CPU layout readiness. The short
title regression ensures the harness continues into the body. Previous
iteration results are dropped before extraction; lifecycle assertions reject
overlapping results. The independent minimal RSS executables load one input,
run three warmups and one measured pass, and report Linux `VmHWM`, including
warmup and allocator retention. Payload estimates remain separate from RSS.

The runner copies all five executables onto the same filesystem and pins both
implementations and their children to the selected CPU. This avoids tmpfs versus
Btrfs executable mapping differences and reduces scheduling variance. It checks
that the old checkout's model, layout and manifests are unchanged, records
commands, environment and exit codes, and retains each run. The comparison
checks fixture hashes and normalized body semantics before applying thresholds;
complex tables are a separate capability comparison. Fonts, input digests,
per-iteration samples and the source fingerprint are included in the evidence.

## dom_smoothie patch

The MIT-licensed [`dom_smoothie`](https://github.com/niklak/dom_smoothie/tree/0fdbefd324d6ba49b4bbdadc85b2a25cc4367b76)
is pinned to upstream commit `0fdbefd324d6ba49b4bbdadc85b2a25cc4367b76` through
`[patch.crates-io]` in the root `Cargo.toml`. It is based on upstream 0.19.0 and
adds `Readability::extract` (upstream PR #246), which returns the processed DOM,
content root and metadata without serializing the article, plus a `dom_query`
re-export. The legacy `parse` entry points keep their behavior on the same
extraction core.

The parity tests in `crates/markview-webpage/tests/extract.rs` compare the new
and legacy entry points' HTML, raw text and metadata, and check the legacy
policy entry points. Run `cargo test -p markview-webpage --locked --test extract`
to verify the adapter against the pinned commit. Equivalent compatibility tests
also live upstream. Remove the patch once a locked upstream release provides an
equivalent non-serializing entry point, keeping Markview's parity tests.

When upgrading `dom_smoothie`, lock the new version, inspect the adapter types
and the complete `Config`, run the `markview-webpage` tests, compare the
extracted block/text metadata on the fixtures, and measure extraction plus
conversion before accepting the change.
