# Document memory evidence

The document/layout storage plan is implemented with its differential and
allocation harness. The benchmark compares independent pre-optimization
production code with the current representation; it does not use cache budget
estimates as live-memory measurements.

## Representation

- Inline images are boxed. On this x86-64 build, `Inline` is 88 bytes instead of
  160, and `InlineKind` is 40 instead of 112. Ordinary text stays inline.
- Retained drawings use ordered runs, local style tables and 12-byte glyph
  positions. Expanded logical drawing indices still address individual glyphs,
  including link boundaries inside a run. Non-text order and clipping remain intact.
- Clusters use checked compact offsets and commands, 28-byte records and shared
  rows/spacing records. Float bit patterns, mixed baselines, direction and atomic
  selection are preserved. Native-width storage handles oversized producer
  values; tiny nodes keep native storage when it costs less.
- Grapheme boundaries use checked `u32` arrays with a native-width fallback.
  Search maps merge only contiguous, nonempty unit-slope pairs; generated and
  non-affine mappings retain their boundaries.
- Immutable draw/text geometry, reading strings, boundaries and maps are shared
  across rebinding. Link IDs have a separate binding array. Scene and auxiliary
  geometry arrays detach on mutation. Nested cache entries own independent leaf
  geometry and command placement metadata, without retaining parent containers.
- Document paths use one pooled buffer while preserving sparse `BlockId`s.
  Semantic arrays and strings shrink once at sealing; packed arrays become boxed
  slices. Sharing uses scoped ownership rather than a global intern pool.

The benchmark also records enum-layout candidates. Boxing only tables does not
shrink the block-kind slot. Further math/reference/callout boxes trade small slot
savings for another allocation and payload per occurrence, so they were not
retained. Joint table/callout boxing remains an alternative with costs on dense
container inputs. Private packed arrays use boxed slices. Root vector metadata could save another
8 bytes per document with a boxed slice; public node payloads would require more
API changes. Sealing existing vectors/strings removes spare capacity without
those changes. Shared reading storage is used where
nested cache/snapshot reuse needs it.

## Independent behavior baseline

The baseline was captured from production commit
`6439953422d6acc659fea196f05e7c6aa7f45908`, the unchanged checkout immediately
before this optimization. The plan's older
`cc4fc12` revision predates separate parser/table fixes; the optimization does not
undo those fixes. The preparation script adds only the harness, its dependencies
and diagnostic accessors to the independent checkout.

Version 2 gzip JSON records live in
`fuzz/fuzzlib/tests/baselines/doc-memory`. The 24 records cover all nine full
benchmark inputs at both 720 px and 660 px,
reader/print excerpts, full Unicode/complex-table/nested-flow probes and
source-free direct/webpage inputs. Records include semantic identity/path/source
relationships, explicit block attributes, exact glyph
and cluster geometry, Unicode boundaries, selection/search/image metadata,
interaction bindings to actual drawings, and visible plus retained content.
Print records include page geometry, pagination fragments and destinations,
PDF text order, link regions and actual page operators, including furniture.
Addresses, run/style IDs, capacities and transient process identities are excluded.
Equivalent affine search segmentation is canonicalized in the exporter, first
validated on the old build before production coalescing.

The local-height accumulation fix updates geometry expectations in 11 records.
An exhaustive field comparison found only floating-point geometry corrections,
at most `0.00048828125` layout units; text, identities, source mappings, command
ownership, line breaks and pagination structure remain unchanged. These reviewed
corrections are frozen in the records; comparisons still require exact float bits.

Laying out a cached leaf at its own origin updates 8 of those records. The
corrections are again only draw, cluster, link and overflow positions, at most
`0.000244140625` layout units; PDF text extraction is unchanged, and only the
exporter's run grouping follows the corrected positions.

Tests compare float bit patterns and emit actual JSON plus the first differing
field under `artifacts/doc-memory-regression`. Sensitivity checks deliberately
change glyph geometry/bold, typed link targets, command ownership, clipping and
invalid ranges. Existing state histories compare fresh engines, serial/parallel
paths and held old snapshots. Ownership tests check geometry sharing, mutation
isolation, reclamation and that a small leaf does not keep a large sibling alive.
Fresh-engine checks also exposed stale occurrence rebinding in cached tables;
cells/captions now participate, and ambiguous truncated mappings remeasure.
The existing fuzz CI runs the frozen-record integration test and uploads failures.
Screen/PDF pixel baselines and the per-channel tolerance of 2 are unchanged.

## Measurement boundaries

`doc_memory` records five samples for each existing 10k prose, code, math and
details fixture, plus repeated prose, dense inline, image, container and mixed
code/math/table inputs. Both executables use Rust 1.99.1, the same lockfile and
committed font bytes, bundled light SC typography, width 720 px and resize width
660 px. Highlighting is synchronous; images are unloaded. `environment.json`
records the setup, and the comparison tool rejects different manifests.

`memory.json` contains exact live Rust allocator-requested bytes, allocation
calls and live allocation counts: parsed document/source/map, cold live/peak,
hot snapshot coexistence, old-snapshot release, resize coexistence/peak and final
release. Dropping the semantic document, source map and original source separately
measures their exclusive retained heap. Timings include parse, first-screen geometry readiness, complete layout, warm
layout and resize; distributions are observations, not CI thresholds. The
first-screen timer ends when progressive layout supplies 800 px of geometry; it
excludes GPU rasterization and presentation. Builds/tests are stopped before timing the executables.

`breakdown.json` is collected in a separate profiling pass, outside all measurement
windows. It deduplicates allocation identities across engine caches and cold,
warm and resized snapshots, recording payload bytes, lengths, capacities and
allocation counts for known semantic/layout buffers and sharing headers. Fonts
are deduplicated as resource byte spans; decoded image bytes are zero in this
controlled workload. Hash/B-tree buckets, shaper/highlight caches and nested
clipped/math/image payloads are not fully itemized. They remain included in the exact
allocator totals. Mapped files, allocator bookkeeping, retained allocator pages
and RSS are not counted as live Rust objects.

The six `storage_counts` columns are expanded draw length/capacity, text-node
length/capacity and cluster length/capacity. The four `draw_storage` columns are
physical commands, compact glyphs, shared styles and drawing storage bytes.
These representation statistics are not behavior baselines.

## Results

Five idle samples per executable; medians, in KiB. These are complete parsed
document + engine + snapshot live heaps, excluding mapped resource bytes.

| Corpus | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| ordinary_10k | 883.4 | 329.3 | 62.7% |
| long_code_10k | 2855.4 | 758.8 | 73.4% |
| math_10k | 903.0 | 339.5 | 62.4% |
| details_10k | 2243.7 | 693.2 | 69.1% |
| prose | 2921.9 | 815.5 | 72.1% |
| inline | 3006.0 | 848.8 | 71.8% |
| images | 1433.3 | 341.3 | 76.2% |
| containers | 1261.7 | 745.6 | 40.9% |
| code_math_table | 472.1 | 332.7 | 29.5% |

| Existing fixture | Resize peak before | After | Reduction |
| --- | ---: | ---: | ---: |
| ordinary_10k | 1571.4 | 480.1 | 69.4% |
| long_code_10k | 5908.3 | 3452.6 | 41.6% |
| math_10k | 1559.9 | 490.8 | 68.5% |
| details_10k | 4135.0 | 1159.3 | 72.0% |

Cold long-code layout is 44.919 → 46.098 ms. Cold details and
nested-container layout cost 3.650 → 4.388 ms and 3.154 → 3.992 ms.
Code resize costs 7.035 → 7.928 ms. Packing rows/styles, independent leaf
ownership and sealing spare capacity add allocation/reallocation work. The time
cost is reported alongside the heap gains; no latency improvement is claimed.

Final-release medians are zero for every corpus in both builds. First-use
process-wide syntax/math caches can remain: the long-code first-sample residual
is 1,193,764 bytes in both builds. Raw distributions retain these outliers; weak
ownership assertions separately verify that document geometry is reclaimed.

The complete raw distributions, allocation counts and component measurements are
in `artifacts/doc-memory-comparison/report.md`, with `memory.png`, `memory.svg`
and machine-readable `comparison.json`. Raw before/after directories contain the
manifests, behavior exports, `memory.json` and `breakdown.json`.

## Reproduce

```sh
git worktree add --detach /tmp/markview-memory-before 6439953
python3 scripts/prepare_doc_memory_baseline.py /tmp/markview-memory-before
cargo build --release --locked --manifest-path /tmp/markview-memory-before/fuzz/Cargo.toml \
  -p mvfuzz --bin doc_memory
cargo build --release --locked --manifest-path fuzz/Cargo.toml -p mvfuzz --bin doc_memory
# Run the built executables after other build/test jobs finish.
/tmp/markview-memory-before/fuzz/target/release/doc_memory artifacts/doc-memory-before
fuzz/target/release/doc_memory artifacts/doc-memory-after artifacts/doc-memory-before
python3 scripts/compare_doc_memory.py artifacts/doc-memory-before \
  artifacts/doc-memory-after artifacts/doc-memory-comparison --plot
```

Use the same toolchain, native libraries and font configuration for both runs.
The optional plot requires Matplotlib; the comparison otherwise uses Python's
standard library. The baseline installer was checked on a fresh independent
checkout. Use `doc_memory --records-only <output> [expected]` for a thin behavior-only
export/comparison. Records stream one case at a time to bound test memory.
Golden tests never use `--update` for this storage change.

## Validation

Passed: complete workspace tests (967 application tests, with existing ignored
cases), layout with parallel workers, the full fuzz suite, the final frozen/state
regressions, strict workspace and storage-feature Clippy, WASM compilation,
rendering coverage and the unchanged screen/PDF golden runner. No pixel baselines
were updated. Android built with the pinned NDK and passed all 15 phone integration
classes on a headless API 35 emulator, the 599dp phone and 600dp tablet boundaries,
and tablet layout at 800dp. Six Android tooling tests passed. The final packing
path was also rebuilt for Android and exercised by a reader smoke run.

No commits were made.
