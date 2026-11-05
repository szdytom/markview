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
