# Fuzzing harness for the Markview libraries

A `cargo-fuzz` + libFuzzer crate (nightly) covering the three library
crates — `markview-core`, `markview-pdf`, `markview-render` — per
[`artifacts/fuzzing-plan.md`](../artifacts/fuzzing-plan.md). This directory
is its own workspace and is deliberately **not** a member of the root
workspace, so `cargo test`/`cargo dist` at the root never see it.

## Targets

| Target | Plan group | Tier | Entry point | Oracle |
| --- | --- | --- | --- | --- |
| `parse` | G1 | T1 | `document::parse`, prefix parse, incremental reparse | no panic (O1); budgets (O2) |
| `reparse` | G1 | T3 | incremental vs full parse | block-for-block equality |
| `mvss` | G2 | T1+T2 | `Stylesheet::parse` + validation | no panic; bounded values |
| `layout` | G3 | T1+T2 | `LayoutEngine::layout` | snapshot fields finite/bounded |
| `layout_diff` | G3 | T3 | normal vs progressive layout | snapshot fingerprint equality |
| `math` | G4 | T1+T2 | `MathEngine::layout` | no panic; box geometry finite |
| `highlight` | G5 | T1 | tree-sitter highlight pass | no panic within budgets |
| `shaping` | G5 | T1+T2 | `TextShaper` metrics + stylesheet validation | advances finite |
| `fonts` | G5 | T1 | font config over the committed test faces | no panic |
| `pdf` | G6 | T1+T2 | `markview_pdf::export` | PDF bytes, no panic |
| `geometry` | G7 | T1+T3 | `markview_render::fuzz_api` primitives | results finite iff inputs are (I7) |

Tiers follow the plan: T1 = ordinary build, T2 = ASAN (the default
`cargo fuzz build`), T3 = differential oracles inside the same harness.

## Shared machinery (`fuzzlib`)

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
  defaults (`Budget::parse/layout/pdf`) are the calibrated numbers in
  [`artifacts/budget-calibration.md`](../artifacts/budget-calibration.md);
  re-run `cargo run -p mvfuzz --bin calibrate` after notable changes and
  update both files together. Campaigns override them via
  `MARKVIEW_FUZZ_TIME_MS`, `MARKVIEW_FUZZ_ALLOC_BASE_KB`,
  `MARKVIEW_FUZZ_ALLOC_KB_PER_KB` (B1) — no rebuild. Wall budgets carry
  a factor of two over the calibrated maximum so a descheduled small
  input in a parallel campaign does not read as a runaway; the layout
  allocation figure carries 2.5, because the detached highlight worker's
  tail can still allocate while the next input's guard is open. The wall
  budget detects *slow* inputs; a hung one is libFuzzer's `-timeout`'s
  job, since a post-hoc `finish` check cannot observe a hang.
- `pipeline.rs` — shared parse/layout plumbing. `warmup()` runs once per
  process before any layout-family input's guard starts: a fenced code
  block pays fontconfig's cold caches and the syntax-set build, and
  `prewarm_highlight` (a `fuzz`-only markview-core API) pays each
  syntax's one-time parse-state build, so none of that warm-up cost
  lands on an input's account.
- `mutators.rs` — structure-aware markdown and MVSS mutators for the text
  targets (line-level edits of headings, lists, fences, tables, math
  delimiters), with a 1/4 fallback to libFuzzer's own mutation. The
  `geometry` target instead takes an `arbitrary`-derived struct (F2):
  every field is an independent `f32` bit pattern.
- `oracle.rs` — finiteness/boundedness assertions, the layout snapshot
  fingerprint (three `DefaultHasher` passes over every geometry field;
  f32s hashed by bit pattern), and the deterministic input-hash helpers
  the targets share. Layout options **pin the committed test fonts**
  (`crates/markview-core/tests/fonts`), never the host's font set, so
  snapshots compare equal across machines.

## Building and running

```sh
cd fuzz
cargo +nightly fuzz build                       # ASAN (T2)
cargo +nightly fuzz build --fuzz-target parse   # one target
cargo +nightly fuzz run parse -- -runs=5000     # brief smoke
```

Formatting: `cargo +nightly fmt -p markview-fuzz -p mvfuzz` inside
`fuzz/`. A bare `cargo fmt --all` here climbs to the *root* workspace and
reformats the app with the wrong toolchain, so keep the root on stable
(`cargo fmt --all` at the root) and the harness on nightly, and never
cross-format.

Seeds (C1–C4): `fuzz/scripts/prepare_seeds.sh` downloads the CommonMark
spec examples (652), the GFM spec examples (648 of 672; the missing ones
nest fenced code inside the example block), KaTeX test-suite LaTeX (764),
and the built-in MVSS stylesheets into `fuzz/corpus/{parse,math,mvss}/`.
The script needs network access; corpora are gitignored and rebuilt on
demand.

## Campaign conventions (R2, R3)

A campaign is a run of one target long enough to stop finding new
coverage. Records are requirements, not optional:

- **Command** (target, flags, `-runs`), **build** (commit +
  `cargo +nightly fuzz build`), **wall clock** and **CPU time** (the
  libFuzzer exit summary), **ASAN on/off**, and the **seed corpus state**
  (output of `prepare_seeds.sh`).
- Known environment: fontconfig's C library keeps charset caches until
  process exit, so campaigns run with
  `ASAN_OPTIONS=detect_leaks=0`; a `LeakSanitizer` report on that is the
  known cache, not a regression (`artifacts/fuzz-runs/highlight-fontconfig-leak-known`).
- Crashes and minimized inputs land in `artifacts/fuzz-runs/<target>-<slug>`
  (the `crash-*`/`leak-*` files libFuzzer writes at the crate root are
  gitignored; move them into `artifacts/` before the next run).
- A found bug is: minimized → seeded into the corpus (and, for library
  bugs, a regression test in the crate) → recorded with attribution
  (markview vs comrak vs upstream) in `artifacts/fuzz-runs/`.

## Findings so far

| Input | Target | Finding | Status |
| --- | --- | --- | --- |
| `artifacts/fuzz-runs/reparse-lone-cr-duplicate-block` | reparse | A lone `\r` line break desynchronized the sourcepos line table; the degenerate range then duplicated a block across incremental reparse | fixed: `line_starts` follows comrak line endings; incremental falls back to full parse on empty ranges |
| `artifacts/fuzz-runs/parse-indented-details-closer-oor-range` | parse | An indented `</details>` put the element's source range past the tag (index out of bounds) | fixed: the tag end is measured through the block literal |
| `artifacts/fuzz-runs/reparse-mid-document-bom-window` | reparse | The parser drops a BOM only at the start of a document; an incremental window opening on a mid-document BOM parsed its first block shorter than a full parse | fixed: a BOM at the window start takes the full parse |
| `artifacts/fuzz-runs/reparse-4-byte-8a0a0908` | reparse | small control-character document that diverged across the incremental path | seeded; passes with the BOM and empty-range guards |
| `artifacts/fuzz-runs/reparse-lone-cr-hidden-list-window` | reparse | the fast path walked the source by `\n`-delimited lines, so a list marker behind a mid-line `\r` read as an ordinary line and the window cut through the list's range | fixed: the line, blank-line, and group model follows the parser's `\n`/`\r\n`/`\r` structure |
| `artifacts/fuzz-runs/geometry-extreme-finite-scaling` | geometry | the oracle demanded the `1e9` bound for finite inputs near the `f32` limit, where pure scaling over it | oracle relaxed: plausible-range operands (≤ `1e6`) owe the bound; near the limit only finiteness |
| `artifacts/fuzz-runs/geometry-zero-extent-denormal-nan` | geometry | `fit_edges` scaled a denormal pair by `0 / denormal` on a non-positive extent, producing `NaN` | fixed: the scale runs only for a positive extent |
| `artifacts/fuzz-runs/geometry-clip-subtraction-overflow-*` and `…/geometry-intersect-near-max-overflow` | geometry | finite operands near `f32::MAX` overflow the `x + w` / `height - top` sums to infinity — `f32` semantics, not a bug | oracle accepts infinity below the `1e38` no-overflow line |
| `artifacts/fuzz-runs/shaping-nan-size` | shaping | same contract for a `NaN` font size | oracle relaxed |
| `artifacts/fuzz-runs/layout-codeblock-cr-run-newline` | layout / highlight | a code line carried a `\r`, which the shaper classifies as a newline: inside a multi-glyph cluster `parley` asserts `!is_newline` | fixed: code and HTML-source lines split on the parser's `\n`/`\r\n`/`\r` structure before shaping and highlighting |
| `artifacts/fuzz-runs/shaping-negative-size-inf-width` | shaping | a finite size near the `f32` limit overflows the per-character advance sum to `-inf` — `f32` semantics, not a bug | oracle relaxed: finiteness owed below the no-overflow line, `NaN` still a bug |
| `artifacts/fuzz-runs/layout-newline-combining-mark-parley-debug-assert` | layout / shaping | `parley`'s `debug_assert!(!is_newline)` fires when a multi-component cluster starts on a line break (break + combining mark); release builds lay the same input out with finite geometry | upstream, seeded; no release impact |
| `artifacts/fuzz-runs/math-crash-1c7f07` | math / layout / layout_diff / pdf | a `\char` literal wider than `i64::MAX` overflows the accumulator in `ratex-parser` 0.1.14 (`macro_expander.rs:823`, `number = number * (b as i64) + d`); only a build with overflow checks panics, and libFuzzer's abort-on-panic hook turns that caught panic into a campaign abort | upstream: `ratex-parser` 0.1.14, harness-visible only; an allowance restores the `catch_unwind` (`mvfuzz::ratex`) |
| `artifacts/fuzz-runs/shaping-raw-size-hang` | shaping | a 6-byte input runs CPU-bound without returning (> 5 min, 99 % CPU); the target's post-hoc wall budget cannot observe a hang, so libFuzzer's `-timeout` caught it | fixed: explicitly disable Parley's height ceiling during shaping; finite extreme sizes overflow line height and otherwise yield forever. Reachable through a selected extreme MVSS rule or library parameters, not Markdown alone under bundled themes |

### Upstream `\char` overflow (harness allowance)

`ratex-parser` 0.1.14 accumulates the `\char` argument into an `i64`
without a width check (`macro_expander.rs:823`). With overflow checks on
— this profile, and `cargo-fuzz`'s default `-Cdebug-assertions` — a
literal at or past `i64::MAX` panics; the reader's release profile has
them off, where the multiply wraps into a negative `\@char` code point
that fails to parse, so `MathEngine::layout` returns `Err` either way
(pinned by `overlong_char_literal_is_an_error_not_a_crash`). It still
aborts a campaign, because `libfuzzer-sys` installs a panic hook that
calls `abort()` before unwinding, which the `catch_unwind` in
`crates/markview-core/src/math.rs` cannot prevent.

`mvfuzz::ratex::allow_char_overflow` swaps that hook for one that lets
exactly this panic — this file, this message — unwind, so the existing
`catch_unwind` absorbs it; every other panic still aborts, so the oracle
stays strict. The four targets that reach ratex call it (`math` directly;
`layout`, `layout_diff`, and `pdf` through a laid-out formula). A
byte-level skip would be the smaller-looking concession, but the literal
can be assembled from a macro or across a comment, so only the panic
itself marks the class. Delete the module and its call sites once the
dependency is fixed or bumped.

