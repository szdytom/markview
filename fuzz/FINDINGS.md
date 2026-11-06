# Findings from the webpage fuzz targets

Defects the `webpage` and `webpage_seam` targets reached, each recorded before
it was fixed. Every fix carries a regression test in
`crates/markview-webpage/tests/extract.rs`, and the fuzz oracle that found it
is tightened in the same commit.

Each finding starts from a minimal page; the command below prints the source,
the expectation and the lowered blocks, so the divergence is visible in one
place.

```sh
cd fuzz
cargo run -p mvfuzz --bin webdump -- <file>            # a generated unit
cargo run -p mvfuzz --bin webdump -- <file> --html     # a raw HTML page
```

## W1 — an explicit heading id was declared twice

`markview-webpage` (`lower.rs`, heading arm) put an element's `id` into the
block's anchor list and also into `BlockKind::Heading::anchor`, so one heading
declared the same fragment twice:

```html
<article><h1>Title</h1><h2 id="marker">Section</h2>
<p>Prose long enough for the extractor to keep this article alive and to
reach the heading below.</p></article>
```

```text
Block { anchors: ["marker"], kind: Heading { level: 2, anchor: "marker" } }
```

The document index registers the first occurrence and ignores the second, so
navigation worked, but the tree broke the one-declaration-per-anchor contract
the anchor oracle and any walk over anchors rely on.

Status: fixed. A heading's explicit id is now only its heading anchor, and a
non-heading id still lands on the block it names. Regression test:
`an_explicit_heading_id_is_declared_once`; the anchor oracle no longer
collapses a block's own declarations, so a duplicate is a finding again.

## W2 — a colliding span was shrunk instead of retried

`markview-webpage` (`lower.rs::fit_span`) turned any rowspan/colspan that
collided at the cursor column into a plain 1×1 cell, even when the span fit
one slot later, so a valid span disappeared and the row could end up with an
interior hole:

```html
<article><h1>Span shrink</h1>
<p>Filler prose keeps this page readable enough for the extractor to keep
the whole article including the table below.</p>
<table><tbody>
<tr><td rowspan="2">A</td><td colspan="2">B</td></tr>
<tr><td colspan="2">C</td></tr>
</tbody></table></article>
```

```text
A  rs=2 cs=1      B  rs=1 cs=2
A  (rowspan)      C  rs=1 cs=1   <- declared colspan 2
                     column 2 uncovered
```

Status: fixed. The cell now starts at the first free slot and its span is
clipped to the free rectangle there, which is where an HTML table places it:
`C` keeps both columns, the table stays three columns wide and no row has an
interior hole. Regression tests:
`a_colliding_span_is_retried_in_the_next_free_column` and
`a_span_is_clipped_to_the_free_columns_at_the_first_free_slot` (the latter
for a span that genuinely cannot fit under the profile's column cap). The
seam oracle requires the declared span again, and the table oracle checks
the interior-hole property.

## W3 — block content inside a heading was flattened

A malformed page can leave a heading open over the rest of the article, and
the extractor keeps that heading with block children. `lower.rs` lowered a
heading with `inlines()`, which walked those children as inline text, so the
body collapsed into one heading with no list or paragraph:

```html
<article><p>Lead paragraph with enough prose for the extractor to keep this
article and its malformed heading.</p>
<h2>Section<div><p>Nested prose inside the heading.</p>
<ul><li>item one</li><li>item two</li></ul></div></h2>
<p>Trailing prose keeps the candidate score high enough.</p></article>
```

```text
pre-fix:  Heading2 "SectionNested prose inside the heading.item oneitem two"
post-fix: Heading2 "Section"
          Paragraph "Nested prose inside the heading."
          List [item one, item two]
```

Status: fixed. A heading now lowers its inline content as the heading text and
its block children as blocks after it. A mixed inline wrapper keeps its text
and style in the heading while its blocks go below; the deferred blocks
inherit the wrapper's link, formatting and code context (so `<code>` content
stays literal and nested math markup stays text), and a semantic wrapper
(math, script, media) keeps its own lowering. Regression tests:
`block_content_inside_a_heading_stays_a_block`,
`heading_text_inside_a_mixed_inline_wrapper_keeps_its_style`,
`a_linked_heading_wrapper_links_its_deferred_blocks`,
`a_math_wrapper_with_a_block_still_lowers_as_a_formula` and
`a_code_heading_wrapper_keeps_its_content_literal`.

## Upstream behaviours the harness works around

These are `dom_smoothie` (the pinned fork) and are not Markview defects; the
targets avoid or accommodate them so they do not drown real findings.

- A single-cell table is unwrapped (`fix_single_cell_tables`), so the
  generator never asks for a one-cell grid.
- Candidate selection can drop or hoist siblings of a short, link-heavy body;
  the generator keeps the `<article>` the top candidate with prose paragraphs
  before and after the generated blocks. A page that still loses content is
  not asserted against the generator's record.
- With a page-level `<base>`, the extractor rewrites even fragment links to
  absolute URLs. The seam target leaves `<base>` to the `webpage` target's
  mutator, whose oracle accepts the resolved address.

## Campaign record

All runs are `cargo +nightly fuzz build` builds (ASAN) on one 16-core Linux
machine, with `ASAN_OPTIONS=detect_leaks=0`, the seed corpora under
`fuzz/corpus/` and `-artifact_prefix` outside the tree:

```sh
# Seeds: prepare_seeds.sh, plus the seven committed web fixtures.
./target/x86_64-unknown-linux-gnu/release/webpage corpus/webpage \
    -max_total_time=900 -max_len=65536 -print_final_stats=1
./target/x86_64-unknown-linux-gnu/release/webpage_seam corpus/webpage_seam \
    -max_total_time=900 -max_len=8192 -print_final_stats=1
```

- `webpage_seam`: 43,417 runs in 901 s, 4,750 units of coverage growth, no
  finding; an earlier 600 s block added 49,584 runs clean, and the final
  verification block on the committed oracles ran 14,063 inputs in 301 s
  clean.
- `webpage`: 28–73 executions/s (each accepted page is converted three times),
  ~9,500–21,000 runs per block, up to 2,900 new units per block; the final
  301 s verification block ran 9,454 inputs clean, and a further 168 s block
  on the finished oracles (image addresses included) ran 5,219 inputs clean.
  The three findings above
  were reached from the seed corpus and from the seam generator's tables, not
  only from long campaigns.
- The oracles themselves were the first thing the campaigns broke: the
  differential hashed the process-wide document revision, link occurrences
  were required to be contiguous and adjacent inside one field, the table
  oracle counted a table's cell fields twice, and the seam expected the
  table caption before its cells. All four were fixed in `fuzzlib`; a false
  positive here is as expensive as a missed bug.

# Findings from the phase D fuzz campaigns

## D1 — an SVG text run with an extreme font size panics tiny-skia

An SVG with ordinary text and a very large `font-size` reaches a tiny-skia
scanline assertion while rendering the glyph outline:

```text
thread panicked at tiny-skia-0.12.0/src/scan/path.rs:221:
assertion failed: edges[curr_idx].last_y >= curr_y as i32
```

Minimal unit (the `IDC1` header leaves the target size at the intrinsic 40×20):

```text
IDC1\0\0\0\0\0\0\0\0\0\0\0\0<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><text x="1" y="15" font-family="sans-serif" font-size="188888888888">Ab</text><rect width="40" height="20" fill="none"/></svg>
```

Reproducer: `fuzz/corpus/image_decode/svg-huge-font`.

Status: open/upstream. `markview-image::decode` calls `resvg`, whose
`tiny-skia` rasterizer panics instead of returning an error. The reader's
background image job catches the panic and turns it into an image failure, so
this does not terminate the process, but a valid SVG cannot be rendered.
A regression should either cap/validate text sizes before rasterization or
recover the panic at the decoder boundary; the finding belongs to the
`tiny-skia`/`resvg` interaction, not to the harness.

## D2 — a prefix cut before a reference definition after a fence loses the link

`parse_prefix` appends document definitions that the bare prefix lacks, but an
open fence at the cut swallows the appended definition and the fallback parses
the bare prefix alone. The full document resolves the link; the prefix shows
literal text, so the reader can show one paragraph and then settle on another.

Minimal source (no fuzz control bytes):

```markdown
[baz]
```

```
[baz]: /u
```

At cut 1 (inside the first line), the full parse links `[baz]` to `/u`; the
prefix parse returns a paragraph whose content key differs and whose inline is
literal `[baz]`. The fuzz reproducer is
`fuzz/artifacts/crashes6/prefix/crash-9976153b3992fac63bdb2149a3dc385b1d1ceb3d`.

Status: open. The parser should either prepend the missing definitions before
the prefix body or refuse the prefix parse when an open fence would swallow
them; a prefix that cannot match the full parse must not be returned as one.

## D3 — an empty table cell's source range escapes the table

The empty second cell in a table row gets a source range that includes the row's
trailing newline, while the table block's range stops before it. The containment
oracle catches the child range `22..23` against the table range `0..22`.

```markdown
|-|---:|
-:|---:|
 z  
```

The visible cells map inside the table; only the empty body cell's empty
paragraph takes the newline. The fuzz reproducer is
`fuzz/artifacts/crashes6/sourcepos_content/crash-401f4301d6614983d6983b9164044da8b060e5dc`.

Status: open. Either the table's range should include its terminating newline,
or the empty cell block should not claim it. `SourceMap::validate` accepts the
inconsistent pair, so the source-position oracle is the only current detector.

## D4 — upstream wiki-link resolution is outside the reference oracle

`![[o]]` parses as a link to `o`; the independent reference-definition scan does
not model Obsidian wikilinks and reported a definition the source did not
declare. This is a harness correction, not a Markview finding: `[[...]]` is the
pinned Comrak wikilink extension.



## Harness corrections made while triaging phase D

These looked like product failures but were harness/oracle mistakes; they are
recorded so the tightened oracles are not mistaken for silent product fixes.

- `reading_pipeline` compared the generated field list before filtering empty
  table-cell fields, and assumed the generated body always starts at source
  byte zero. Empty source lines made both assumptions false; the oracle now
  filters both sides and locates the first non-empty generated field.
- `reading_pipeline` required an old selection's copy to survive after a
  disclosure change even when the selection was no longer visible. It now
  checks `selection_rects` before asserting copy stability.
- `fonts` asserted that every non-empty text run produces a glyph; `SHP1` with
  an empty body legitimately shapes nothing.
- `highlight_schedule` performed A→B→A with a rejecting executor still in
  control and then called `wait_highlights`, blocking on tasks the input had
  withheld. The branch now enables the executor before draining.
- `image_decode`, `math`, `layout`, `pdf` and the integration targets use stage
  budgets that account for 16 MP RGBA decoding, configured math expansion,
  ASAN overhead and multiple real layouts. The old single-layout windows
  reported budget panics on legal work.
- The PDF text oracle counted image-placeholder labels that the exporter clips
  to the image slot; those labels are now excluded.
- The PDF link oracle compared a link rectangle inside a horizontally
  overflowing block without the viewport clip. Links outside the visible
  overflow window are now skipped, matching what the exporter can paint.
