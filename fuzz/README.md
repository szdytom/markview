# Fuzzing harness for the Markview libraries

A `cargo-fuzz` + libFuzzer crate (nightly) covering the library crates —
`markview-commonmark`, `markview-document`, `markview-layout`,
`markview-pdf`, `markview-render`, `markview-image`,
`markview-font-archive` and `markview-webpage`. This directory is
its own workspace and is deliberately **not** a member of the root workspace,
so `cargo test`/`cargo dist` at the root never see it.

The web front end arrives through the same `dom_smoothie` fork the reader
pins (`[patch.crates-io]` in `fuzz/Cargo.toml`); if the pin drifts, the
harness would fuzz a different extractor than the app ships, so the two
manifests must move together.

## What the targets look for

A target is only worth a slot if it can fail for a reason other than a
segfault, so every one below carries an **oracle**: a property a correct
implementation must have, checked against the tree or the output. Tiers follow
the plan: T1 runs the property in an ordinary build, T2 adds ASAN, T3 compares
two readings — a fast path against a full one, a cache against a fresh engine,
a tree against an independent scan of the same bytes.

### G1 — parsing and document structure

- **`parse`** — looks for a panic, an overflow, a budget overrun, or a source
  range that is unordered, out of bounds or off a character boundary. How: one
  input goes through the full parse, `assert_source_ranges`, the outline /
  `details_enclosing` / image walks, four prefix cuts, `parse_incremental` and
  `reparse`, all under one `InputGuard`.
- **`reparse`** — looks for the incremental fast path disagreeing with a full
  parse after a deterministic edit. How: both documents are fingerprinted
  *recursively* — every nested block's range and every inline's kind, style and
  range — so a shifted nested range is caught even when the rendered text is
  unchanged. A declined fast path is equal by construction, so the interesting
  inputs are the ones it accepts.
- **`prefix`** — looks for `parse_prefix` not being the opening of the full
  parse. How: six fixed and derived cut points per input; a prefix may truncate
  its last block, but no earlier block may differ and the last must stay the
  same construct.
- **`refdef`** — looks for a reference link resolving differently from what the
  source declares. How: an independent column-zero scan of the same bytes
  derives the definition table, and only two directions are asserted — a
  resolved link must have a definition carrying that destination, and an
  unresolved one must have no definition at all — so label *spelling* cannot
  fire it. A second differential appends a definition nothing references and
  demands every block before it stay identical.
- **`sourcepos_content`** — looks for a range that is ordered and in bounds yet
  still not a real place for the node that stores it. How: a containment walk
  (nested block inside its parent, inline inside its block, cell inline inside
  the table) through the separate `SourceMap`; reading blocks must have a
  mapping, including legitimate zero-width mappings.
- **`details_structure`** — looks for the `<details>` tree disagreeing with the
  tags the source wrote. How: the disclosure chain is rebuilt from the tree and
  compared with `Document::details_enclosing`, and every element's declared
  `open` is checked against its own opener rather than a sibling's.
- **`anchors`** — looks for a fragment jump that is unreachable or lands on the
  wrong block. How: `seam::fragment_targets` rebuilds the registry of every
  target the tree declares — a raw-HTML `id`/`name` on a block or inline, a
  heading slug, typed footnote definitions and distinct reference occurrences —
  with the disclosure chain framing each one. The target then demands that `details_enclosing` names that
  chain, that expanding it makes the name resolvable, that the first block a
  jump reaches is the block declaring the name first (with the reader's own
  toggles, the first declaration the source left open), that the layout invents
  no other anchor, that an anchor sits inside its block's box in y order, and
  that pagination keeps up. Two readings of one input, no second
  implementation.

### Integrated reading and webpage pipelines

- **`reading_pipeline`** — real Markdown parsing, pinned-font shaping, search
  navigation through disclosures, copy, selection rectangles with horizontal
  offsets, reflow, and source-map range validity. Literal search uses an
  independent non-overlapping scan; ASCII folding is restricted to ASCII input.
  Ordinary text copies must equal the matched substring; atomic projections
  only exercise legal selection and geometry. Raw input uses parsed fields as
  its model; generated input independently records the expected field order and
  checks that its first plain line maps to source byte zero and back. Input is
  at most 32 KiB. `RDP1` plus eight control bytes separates mode, query
  field/word, case, step count and widths from the body. Odd mode generates
  plain multilingual paragraphs, a disclosure and a table. There are 1–8 reflows
  and at most four consumed hits per step. Missing generated selections fail;
  raw inputs may legitimately lose geometry to layout limits. `RDB1` is a
  directed 64 KiB mode: padding keeps the document near the limit while a plain
  marker paragraph remains searchable, copyable and source-round-trippable,
  with one real layout.
- **`webpage_layout`** — first byte modulo four selects a generated `webgen`
  article, a direct legal span grid with a nested table and figure, a plain
  three-column grid, or raw HTML. Extraction, generation and downstream
  consumption share the input budget. Generated HTML is limited to 128 KiB,
  input to 8 KiB, and direct grids to 51 cells. Four width/fold states compare
  cached and fresh scenes, reading text, pagination and anchors. Fully expanded
  direct grids check ordered unique cell markers; generated articles check
  marker conservation while allowing captions' reading order. The plain grid
  checks that short LTR cell text stays in its own column. Pagination fragments
  must cover each block's bands once, contiguously and in page order, with
  anchors and targets inside the page list. Raw HTML has no full-preservation
  assertion. Narrow table overflow is legal.

Prepare local seeds without downloading upstream corpora. This writes the
integration, highlight, seam and project-dialect seeds, and copies a shared
Markdown pool (committed fixtures plus dialect/reference cases) into every
target whose unit is a document:

```sh
python3 fuzz/scripts/prepare_pipeline_seeds.py
```

`prepare_seeds.sh` adds the spec examples fetched from shallow clones to the
same pool after the fetch.

The integration targets report semantic path hits (`web/span-layout`,
`web/multipage`, `reading/selection`, `history/state`, and so on) every thousand
inputs and at the end of a `pipeline_replay` run, so a campaign's effective
coverage is visible without per-input logging.

### G2–G5 — stylesheets, layout, text

- **`mvss`** — looks for a panic in stylesheet parsing or validation. How:
  `format_version`, the rule / font / page / svg / mermaid tables and the
  numeric, URL and digest checks, under budgets; a malformed sheet must return
  `Err`, never panic.
- **`layout`** — looks for a panic or non-finite, implausible geometry. How: the
  whole pipeline (line breaking, paragraphs, microtype, tables, scene) with
  every `Limits` budget driven toward its floor, and every produced geometry
  value checked finite and bounded.
- **`layout_diff`** — looks for the engine answering differently the second
  time, or a progressive layout disagreeing with a direct one. How: the same
  document is laid out twice by one engine and once by a fresh engine — all
  three snapshots must fingerprint equal, so a cache that changes the result is
  itself a finding. Syntax highlighting runs synchronously for these comparisons
  because the fingerprints include paint; background colors arriving between
  passes are an expected change. The document is then laid out progressively,
  with a prefix snapshot checked block by block (id, position, geometry) against
  the direct layout. A versioned `LDH1` branch additionally accepts a count
  (0–16), that many two-byte `(opcode, value)` operations, and up to 32 KiB of
  Markdown. Opcodes modulo ten change width, size, merge valid MVSS, switch
  documents, force disclosures, replace image versions/pixels, cancel, clear
  caches, release, or reparse with a new revision. Every checkpoint reconstructs
  a cold synchronous engine from the complete current state. Text fields,
  links and block geometry have explicit diagnostics alongside the scene hash.
  Remove operation pairs and adjust the count to shrink a history. Legacy raw
  Markdown inputs retain the original cache/progressive checks.
- **`paginate_pipeline`** — looks for page fragments losing, repeating or
  reordering a real laid-out block. How: Markdown up to 16 KiB is laid out with
  pinned fonts and then paginated with a page height derived from the snapshot
  and two to four desired pages. Fragments must cover each block's bands once,
  in page order; visible blocks must produce a fragment; page items must be
  finite, y-ordered and carry correct first/last flags; anchors and targets must
  point inside the page list.
- **`layout_parallel`** — looks for serial and native parallel block layout
  disagreeing. Two synchronous-highlight engines use pinned fonts and 2–4
  workers across cold, cached, resized/theme/image, original-source and
  cancelled/edited passes. A three-paragraph probe prefix guarantees worker
  execution even for empty inputs; the unprefixed source is also checked to
  preserve front matter coverage. A cancellation callback verifies that work
  really ran on another thread. Sampled prefixes, visible and retained local
  scenes, expanded disclosures, reading text, search bindings and source ranges
  must agree; font blob identities and cache bookkeeping may legally vary.
  Inputs are limited to 32 KiB after UTF-8 replacement, with the Markdown
  structure mutator. The paired-pass wall budget is five times `layout` and
  fixed allocation headroom twice `layout`; environment overrides still apply.
  CI runs its smoke alongside the existing targets and tests the shared oracles.
- **`math`** — looks for an uncaught panic, an overflow, or non-finite box
  geometry on the ratex path. How: `MathEngine::layout` under budgets, with a
  LaTeX-aware mutator keeping the corpus inside macro territory, where the
  expander's work accounting lives. `MTH1` instead composes valid fragments —
  macros, groups, scripts, delimiters and matrices — so deep layout and the
  resource limits are reached by well-formed formulas too.
- **`highlight`** — looks for a panic in the syntax pass, reached the way the
  reader reaches it (a fenced code block inside a layout pass). How: the unit is
  `language \0 code`; the byte selects a language the shipped syntax set knows,
  and the fence is longer than any backtick run in the code, so the block really
  carries the input. A `raw` language keeps the old fixed fence for integration
  behavior. The uncolored fallback is legal, and the detached worker gets its
  own allocation headroom.
- **`highlight_schedule`** — looks for stale background work changing the
  current document or a settled pass disagreeing with a direct engine. How:
  `HLP1` plus eight control bytes drives a queue that accepts, rejects, runs
  one task, polls, switches document or theme, clears, releases, and performs an
  A→B→A sequence. Every input drains its queue before the budget closes, so a
  withheld task can never block the target. The final document must fingerprint
  equal to a synchronous `Direct` engine, and no task may remain queued.
- **`shaping`** — looks for non-finite advances or offsets. How: parley/swash
  shaping over arbitrary text and sizes, plus the stylesheet validation the
  shaper runs on install.
- **`fonts`** — looks for a panic in font validation or face parsing. How:
  arbitrary bytes go through the byte-level checks and directory scan; a
  malformed font must degrade to "not a font". `FNT1` selects a committed face,
  optionally mutates it, then runs the shaper over it, so valid faces reach
  face registration and real glyph shaping instead of only the validator.
- **`shaping`** — also has an `SHP1` structured mode that composes multilingual,
  combining-mark, emoji and bidi text and checks finite advances, glyph
  presence, and deterministic repetition. Pure arbitrary text and raw size bit
  patterns remain.

### G6–G7 — export, images, interaction and archives

- **`interaction_sequence`** — looks for real document geometry disagreeing
  with a gesture or scroll result. How: `INT1` plus controls drives a real
  settled snapshot through hit testing, hover, click grains, selection drags,
  disclosure/reflow changes, horizontal overflow, wheel scroll, pan phases and
  release inertia. Offsets and deltas must stay finite and within their stored
  bounds; selections must extract from the current snapshot.
- **`font_archive`** — looks for an extraction that escapes its directory,
  keeps a member it handed to the caller, or panics on a damaged container.
  How: `ARC1` synthesizes valid zip, tar, gzip-tar and zstd-tar packages with
  filtered members, or feeds the payload as a damaged archive; matching,
  ownership transfer and cleanup run through the shared
  `markview-font-archive` core.
- **`image_decode`** — looks for a decoded image violating its size, alpha or
  consumer contract. How: `IDC1` plus twelve control bytes (target size,
  initial width, PDF consumer, explicit-target bit) feeds real bytes to the
  production decoder now shared through `markview-image`. SVG text receives the
  committed fixed font database. Successful results must have non-zero sides,
  at most 16 million pixels, exact RGBA length and a valid intrinsic size;
  bitmap rasters stay within an 8192 side; a PDF bit sends the pixels through
  `PreparedImage`. Invalid input is a legal `Err`.

- **`pdf`** — looks for two exports of one input differing, or the exported
  bytes disagreeing structurally with the document they came from. How: export
  twice and compare bytes, then re-parse with `lopdf` and check page count,
  well-formedness, page geometry, per-page content, text round-trip and link
  destinations against the layout, pagination and geometry the export was given.
  Documents containing images use small non-uniform, partly transparent RGBA
  images; an image input is also exported through the precompressed
  `PreparedImage` path and checked against the same structural oracles.
- **`geometry`** — looks for a renderer primitive returning a non-finite result
  from finite inputs (I7). How: the unit is an `arbitrary`-derived struct of raw
  `f32` bit patterns, and the checks cover intersection, corner/edge fitting and
  viewport transforms.

### G8 — web pages

- **`webpage`** — looks for a conversion that breaks the document contract or
  answers differently on a second reading. How: arbitrary bytes are an HTML
  page and a byte picks the base URL; every accepted article must have unique
  anchors (each filed under the disclosures that frame it), one address per
  link occurrence, link addresses that parse (or keep their scheme after a
  failed join) and image resources that parse as absolute or `data:` URLs,
  whitespace-normalised text with no raw newline or tab, a table whose cells
  tile the grid without overlap or an interior hole and whose column model
  covers the widest row,
  non-empty collapsed metadata with absolute canonical/image URLs, the
  title/source preamble in its fixed position, every block reachable through
  the sealed index, and a diagnostic for a duplicate id only when one element
  kept it. The conversion must also fingerprint equal on a second run, match
  between `extract` and `extract_with`, and never change a success into a
  failure when the profile is scaled down to the same limits the reader
  applies.
- **`webpage_seam`** — looks for the lowering disagreeing with the page the
  generator wrote. How: a byte stream becomes a well-formed article with
  unique markers (headings with and without explicit ids, styled and linked
  runs, inline formulas, nested lists with checklists, start values and
  reversed order, quotes, code blocks, definition lists, details, figures,
  inline images, tables with row and column spans) and a recorded
  expectation. The extracted document must carry every marker exactly once
  in order and turn each generated element into the block the generator
  described: text and style, resolved link address, formula source and
  display flag, list direction, tightness and item structure, table group
  order, cell text, kind and span, disclosure state, definition terms, figure
  caption and image alt. Markers make the comparison unique, and the
  expectation comes from the generator rather than from the lowering, so a
  lost or misplaced element cannot agree with itself. Defects it reached,
  with their regression tests, and the upstream behaviours it works around
  are recorded in [`FINDINGS.md`](FINDINGS.md).

## How the oracles are built (`fuzzlib`)

- `allocator.rs` — a counting global allocator. **Declared once, here**
  (`#[global_allocator] pub static GLOBAL`); the [`budget::InputGuard`]
  reads the same instance, so the allocation gate is live. The guard
  meters a *per-input window peak*: `open_window` snapshots live bytes
  and resets the window high-water mark, so each input is judged on its
  own transient peak, never on the process-global peak (which an earlier
  larger input would otherwise mask). A zero allowance must panic — that
  is pinned by reverse unit tests (`cargo test -p mvfuzz`), including one
  where the input's peak stays below the historical global peak.
- `budget.rs` — per-input wall-clock and peak-allocation budgets. The
  allocation figure is the *window* peak of live bytes since the guard
  opened (see `allocator.rs`), so a budget-blowing input is caught even
  after a larger earlier input raised the global peak. The per-stage
  defaults (`Budget::parse/layout/pdf`) are calibrated from representative
  measurements — high percentile times a safety factor, plus a linear term
  when the peak grows with input length; `cargo run -p mvfuzz --bin calibrate`
  re-derives them. Campaigns override them via `MARKVIEW_FUZZ_TIME_MS`,
  `MARKVIEW_FUZZ_ALLOC_BASE_KB`, `MARKVIEW_FUZZ_ALLOC_KB_PER_KB` (B1) — no
  rebuild. Wall budgets carry a factor of two over the calibrated maximum so a
  descheduled small input in a parallel campaign does not read as a runaway; the
  layout allocation figure carries 2.5, because the detached highlight worker's
  tail can still allocate while the next input's guard is open. The wall budget
  detects *slow* inputs; a hung one is libFuzzer's `-timeout`'s job, since a
  post-hoc `finish` check cannot observe a hang.
- `pipeline.rs` — shared parse/layout plumbing. `warmup()` runs once per
  process before any layout-family input's guard starts: a fenced code
  block pays fontconfig's cold caches and the syntax-set build, and
  `prewarm_highlight` (a `fuzz`-only markview-layout API) pays each
  syntax's one-time parse-state build, so none of that warm-up cost
  lands on an input's account. `export_pdf` supplies small non-uniform,
  partly transparent images for every source the layout drew, so an input
  containing an image reaches the exporter instead of stopping at its "cannot
  embed image" precondition; an image input can also take the precompressed
  `PreparedImage` path.
- `mutators.rs` — structure-aware markdown and MVSS mutators for the text
  targets (line-level edits of headings, lists, fences, tables, math
  delimiters), with a 1/4 fallback to libFuzzer's own mutation. `math()` is
  the LaTeX-shaped one, wired into the `math` target through
  `LLVMFuzzerCustomMutator`: it splices macro fragments
  (`MATH_MACRO_FRAGMENTS`), wraps and duplicates spans, and edits at TeX's own
  granularity, because the line-oriented markdown edits read as literal text
  to the expander. The integration targets commit their versioned headers and
  control bytes (`RDP1`, `RDB1`, `INT1`, `HLP1`, `ARC1`, `FNT1`, `SHP1`,
  `MTH1`, `RPS1`, `IDC1`, `LDH1`) and mutate only the payload, so a campaign
  keeps reaching the structured paths instead of spending its time repairing a
  magic byte. `MTH1` and `RPS1` keep their control words too, so the structured
  math and reparse sequences stay valid across mutations. The `geometry` target
  instead takes an `arbitrary`-derived struct (F2): every field is an
  independent `f32` bit pattern.
- `oracle.rs` — finiteness/boundedness assertions, the layout snapshot
  fingerprint (three `DefaultHasher` passes over every geometry field;
  f32s hashed by bit pattern), and the deterministic input-hash helpers
  the targets share. `document` fingerprints the whole block tree —
  nested blocks' source ranges and every inline's kind, style and range —
  so a reparse that shifts a nested range is caught even when the reading
  text is unchanged. `assert_source_ranges` demands the contract
  `SourceMap::validate` states — every range ordered, in bounds and on a
  character boundary, with every semantic offset inside its field — which
  holds for a re-parsed `<details>` body too, because a snippet's offsets are
  remapped onto the document's. `inline_source` wraps the inline lookup so a
  run with reading text that lost its range cannot fingerprint as an empty
  `0..0`. `assert_prefix_consistent` is the `prefix` target's differential (a
  prefix may truncate its last block, but no earlier one may differ, and the
  last must stay the same construct). Layout options **pin the committed test
  fonts** (`crates/markview-layout/tests/fonts`), never the host's font set,
  so snapshots compare equal across machines.
  Font content fingerprints are memoized by blob identity within a comparison;
  only the content fingerprint enters the digest, keeping worker identities
  irrelevant without rehashing a whole font for every glyph.
- `seam.rs` — the structural differentials the `G1` Tier-3 targets share:
  source ranges against the text they address (`assert_rich_text_covered`
  and the containment walks), reference resolution against an independent
  scan of the same bytes (`assert_reference_resolution`), the `<details>`
  tags the source wrote against the elements the tree built
  (`details_elements`, `declared_open`), and the fragment targets the tree
  declares with the disclosure chain framing each one (`fragment_targets`).
  They need no second implementation, only an input whose two readings
  disagree.
- `webpage.rs` — the `G8` invariants over one `markview_webpage` article,
  the structural fingerprint the differential compares, the budget-profile
  monotonicity check, and the reading-text projection the seam target uses.
- `webgen.rs` — the `webpage_seam` generator: it writes a page and records
  what it wrote, so the comparison has an independent reading. It only emits
  shapes the pinned extractor preserves: it does not generate a single-cell
  table, keeps prose around a short body so candidate selection keeps the
  article, and leaves a page-level `<base>` to the `webpage` target (see
  `FINDINGS.md` for those upstream behaviours).
- `pdf_oracle.rs` — the `pdf` target's structural readback: the exported
  bytes are re-parsed and checked against the layout, pagination and geometry
  the export was given (page count, well-formedness, page geometry, per-page
  content, text round-trip, link destinations). The text check is a multiset
  over the code points the pinned faces can draw, and it excludes the
  character after an undrawable glyph, because `lopdf`'s `/ToUnicode` decoder
  matches greedily without consulting `codespacerange` — an extractor limit,
  not an export defect.
- `probe.rs` — `measure`/`Measured`, so a one-off probe reports the same
  window-peak allocation `budget::InputGuard` meters; used by
  `bin/mathprobe` and `bin/warmcheck`.

## Building and running

```sh
cd fuzz
cargo +nightly fuzz build                       # ASAN (T2), every target
cargo +nightly fuzz build parse                 # one target
cargo +nightly fuzz run parse -- -runs=5000     # brief smoke
cargo +nightly fuzz run layout_parallel -- -runs=5000 -max_len=32768
cargo +nightly fuzz run webpage -- -runs=5000 -max_len=65536
cargo +nightly fuzz run webpage_seam -- -runs=5000 -max_len=8192
```

CI runs library feature checks and differential-oracle tests in one job,
parallel with four ASAN smoke shards (`syntax`, `layout`, `web`, `output`).
Each shard builds and runs only its assigned targets, with the same seed
preparation and `-runs=100` budget. The shards together cover every harness
in `Cargo.toml`; the CI infrastructure test checks that none are skipped or
repeated. Each shard caches the standalone fuzz workspace separately.

Formatting: `cargo +nightly fmt -p markview-fuzz -p mvfuzz` inside
`fuzz/`. A bare `cargo fmt --all` here climbs to the *root* workspace and
reformats the app with the wrong toolchain, so keep the root on stable
(`cargo fmt --all` at the root) and the harness on nightly, and never
cross-format.

Seeds (C1–C4): `fuzz/scripts/prepare_seeds.sh` shallow-clones the CommonMark
and GFM repositories and KaTeX, takes the spec/test files, and extracts the
CommonMark spec examples (652), the GFM spec examples (648 of 672; the missing
ones nest fenced code inside the example block), KaTeX test-suite LaTeX (764)
and the built-in MVSS stylesheets. It also writes the committed
`tests/fixtures/*.md` documents and hand-written dialect/reference seeds into a
shared Markdown pool, then distributes the spec and project examples to
`parse`, `prefix`, `refdef`, `sourcepos_content`, `details_structure`,
`anchors`, `layout`, `layout_parallel` and `pdf` (each under its own size cap;
the layout family and the other slower targets get an evenly sampled subset
rather than all 1300 spec examples, `layout_parallel` skips the 10 KiB
fixtures because one input costs tens of seconds, and `pdf` skips the
colour-emoji fixture until the text oracle can account for the fallback). The local `prepare_pipeline_seeds.py` additionally
seeds the reading, web, history, queue, image, archive, font, shaping, math,
highlight and webpage-seam targets, and copies `tests/fixtures/web-*.html` into
`fuzz/corpus/webpage/` plus raw mode 3 variants into `webpage_layout`. The
script needs network access for the cloned corpora (the fixtures are local);
corpora are gitignored and rebuilt on demand.

## Adding an oracle

1. Name the property a correct implementation must have, and the reading it
   can be falsified against. Prefer a second, independent reading (a scan of
   the source, a fresh engine, a re-parse of the output) over a restatement of
   the code under test; a differential that mirrors the implementation only
   tests itself.
2. Decide what may legally vary. Every oracle here needs a documented escape
   hatch — a declined fast path, an uncolored fallback, a snippet-relative
   range — or it will be red from the first corpus replay and bury anything
   new.
3. Register the target in `Cargo.toml`, describe it in the table above, and run
   `fuzz/scripts/guard.sh --full` before spending a campaign on it.

## Campaigns

A campaign is a run of one target long enough to stop finding new coverage. A
record is a requirement, not an option, and it holds:

- **command** (target, flags, `-runs`), **build** (commit plus how the binaries
  were produced), **wall clock** and **CPU time** (the libFuzzer exit
  summary), **ASAN on/off**, and the **seed-corpus state** (what
  `prepare_seeds.sh` produced).
- Known environment: fontconfig's C library keeps charset caches until process
  exit, so campaigns run with `ASAN_OPTIONS=detect_leaks=0`, and a
  `LeakSanitizer` report on that cache is not a regression.
- Corpus and campaign records are generated artifacts and stay outside version
  control. libFuzzer also drops `crash-*`/`leak-*` files in the crate root when
  a run dies; move them out of the tree before the next run.
- A found bug's path: minimize it, seed the minimized input into the corpus
  (and, for a library bug, add a regression test in the crate), and attribute it
  (markview vs comrak vs another upstream). An input that only trips an oracle
  needs the same attribution before it is worth a record.

`fuzz/scripts/` carries the slot allocator and the drivers a long campaign runs
through:

- `slot.sh` — eight flock-protected slots, one per P-core logical CPU,
  `taskset`-pinned and RSS-capped through `rsscap.sh`. `--list`, `--wait`
  (block for a slot), `--try` (run only if one is free); `FUZZ_SLOT_DIR` moves
  the lock directory.
- `rsscap.sh` — an RSS watchdog for the command it wraps: a process that grows
  past the limit is killed, so a runaway fuzzer dies instead of taking the
  machine down. A virtual-address cap (`ulimit -v`) is **not** usable here:
  ASAN reserves terabytes of shadow address space, so a sanitized binary would
  abort before `main`.
- `campaign.sh <target> <seconds>` — one bounded run: slot, pin, RSS ceiling,
  libFuzzer flags, and a self-contained record (command line, binary mtime,
  corpus state, exit summary, peak RSS) written next to the run's log. The
  record root is `out` at the top of the script. Fuzzing runs in fork mode with
  `-keep_seed`, so a crash libFuzzer reports mid-block does not end it and the
  block still honours `-max_total_time` on an accumulated corpus.
- `campaign-supervisor.sh <hours>` — a fill loop that keeps every slot busy
  from a priority-ordered rotation for the given window, then drains.
- `guard.sh` — `cargo check -p mvfuzz` before a campaign starts (`--full` adds
  every target), so a broken shared library cannot waste a slot.
