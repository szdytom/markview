# Internal API property testing

Property tests run as ordinary Rust unit tests using Proptest. Keep them in a
`#[cfg(test)] mod proptests` next to their owning API; longer modules use
`module/proptests.rs`. Private access comes from the test child module, without
expanding production visibility. The dependency is development-only in five
crates, with `std` enabled and the default fork, timeout and bit-set features
disabled. Proptest 1.12 requires Rust 1.88, below the workspace's 1.96 MSRV.

## Run and reproduce

From the repository root:

```sh
cargo test --locked -p markview-geometry -p markview-document -p markview-layout -p markview-ui-core -p markview-interaction --lib proptests::
PROPTEST_CASES=4096 cargo test --locked -p markview-geometry -p markview-document -p markview-layout -p markview-ui-core -p markview-interaction --lib proptests::
```

Use a longer test name or one `-p` argument to isolate a failure. Native build
prerequisites are in the [development guide](development.md). These local API
fixtures need no system fonts, renderer, OS events, sleep or asynchronous loop.
The scroll tests pass explicit times derived from one clock origin.

Default configuration supplies 256 cases and a finite shrinking budget. All
strategies bound collection lengths, text sizes and operation sequences. No
property overrides `cases`, so `PROPTEST_CASES` expands every randomized test,
including the directed 64 KiB search tests. Those tests shrink padding within
65520–65544 bytes rather than losing the boundary condition. There are no fixed
exploration seeds or rejection filters.

Proptest prints the reduced parameters and saves replay seeds in the owning
crate's `proptest-regressions/` directory, mirroring the source module path.
Keep these files in Git. A failure can be reproduced with the persisted seed,
or rerun an exploratory run with `PROPTEST_RNG_SEED` set to its recorded
numeric RNG seed using the same Proptest version and strategies. The persisted
`cc` hash is a replay record, not the numeric value for that environment variable. A reduced input is a counterexample found by shrinking,
not a claim of global minimality. Diagnose the first divergent step and retain
an ordinary concrete regression test for a confirmed production defect; a seed
alone can change meaning when a generator changes. Seeds from model mistakes
are useful too, with their diagnosis recorded in the change evidence.

CI runs these with the workspace tests on Linux, Windows and macOS. Failed jobs
upload `**/proptest-regressions/**` as `proptest-regressions-OS` for 14 days;
the Actions log retains assertions and reduced inputs. Download the artifact,
copy the files back to their crate paths and rerun the focused test. CI does not
write seeds back to the branch. A local saved failure has verified the path;
remote upload requires an actual CI run.

## Author a behavioral contract

Before writing a generator, identify the existing documentation, specification
or caller dependency that justifies the guarantee. Write a short checklist:
inputs and state preconditions, observable outputs, allowed fallbacks, properties
and their oracle, and boundaries still outside the model. Stable guarantees
belong in English Rust documentation at the API, rather than a separate contract
registry. Do not turn incidental implementation choices into commitments.

Generate semantic inputs directly. Construct valid identity, encoding and state
relationships, retaining them under shrinking. Include empty, singleton,
adjacent, duplicate, exact-threshold and just-over-threshold cases deliberately.
Use a small ordinary test where a category must be visited on every run. Separate
input domain from sample budget: lowering cases must not remove a category.

For numeric tests, declare bounds that prevent intermediate overflow. Small
integers and binary fractions permit exact checks; general floats need an error
bound stated in units and tied to operations and magnitude. Merely accepting
finite inputs does not prove intermediate arithmetic is safe.

Use a simpler oracle: prefix tables, a set plus suffix enumeration, a field list,
small exhaustive partitions or a sequence of requested destinations. Do not use
the tested function to calculate expected values, or rely on paired round trips
that could hide compensating errors. Stateful tests start fresh per case and
check after each operation; include repeated cancellation, empty flushes and
release after removal. Keep the model smaller than the implementation.

## Review by behavior

Read the purpose and contract first, and independently describe the API's inputs,
states and observable guarantees. Review the contract itself: passing tests do
not authorize a semantic change. Inspect generator domains and shrinking, then
check that the oracle does not reuse the core algorithm. Run existing properties
and concrete regressions; investigate a specific uncertainty by changing the
input distribution, adding a sequence or enumerating a small domain.

Classify each counterexample as an implementation defect, ambiguous contract,
model error or generator error before changing code. Read the implementation and
callers to explain the result and locate defects. Resolve clearly established
bugs directly; leave disputed semantics explicit for a human decision while
continuing independent work. Never filter a disagreement out of the generator.

Record the behavior hypothesis, domain, oracle, command, result and limitations
in the change evidence. Optional fault injection belongs in an isolated temporary
copy and must be cleaned up. It is useful when it answers a concrete question;
there is no mutation count or score gate. Successful exploration is evidence
within its domain, not a proof or a requirement to find a bug.

## Examples and boundaries

| Boundary and tests | Model and limits |
| --- | --- |
| [Encoding positions](../../crates/markview-layout/src/source/proptests.rs) | UTF-8/UTF-16 prefix tables independently check every interior and beyond-end offset. Encoding boundaries, not grapheme boundaries. |
| [Link identity](../../crates/markview-document/src/proptests.rs) | Explicit address comparison and semantic cache hashing versus address-plus-occurrence-ID identity; independent allocations of the same address with the same ID agree, while `with_occurrence` preserves address sharing. |
| [Message scheduling](../../crates/markview-ui-core/src/component/proptests.rs) | Ordered payload lists, maximum invalidation, map preservation and regrouped flush batches; guarantees only the simple recorder component. |
| [Pan feed](../../crates/markview-interaction/src/pan/proptests.rs) | Active gesture intervals and accumulated deltas; exact binary fractions and bounded float error, no native adapter. |
| [Rectangles](../../crates/markview-geometry/src/proptests.rs) | Integer interval model, positive-area intersections, inclusive hit edges, degenerate boxes and bounded floats. NaN, infinity and overflowing extents are outside the caller domain. |
| [Heading anchors](../../crates/markview-document/src/slug/proptests.rs) | Explicit Unicode token semantics and a reserved-name set with explicit reservations, membership checks and suffix enumeration; no claim of compatibility with every external slug implementation. |
| [Reading mapping](../../crates/markview-layout/src/layout/mapping/proptests.rs) | Declared ordinary/atomic segments with all ordered start/end pairs, byte origins, typed footnote definitions/numbers/occurrences, half-open notes and span-link priority. Fixtures bypass shaping; note ranges are nonoverlapping. |
| [Image resources](../../crates/markview-layout/src/image/proptests.rs) | Complete versioned maps (including snapshot key sets), shared snapshot views and one consumable latest frame, demand joins, bounded sizes and independent attribute precedence. Sequential behavior only. |
| [Widgets](../../crates/markview-ui-core/src/widget/proptests.rs) | Ordered hit candidates, minimum touch distance, cyclic eligible identities, capture across removal/disable/movement/message changes, and release at the moved position. Unique IDs are required; equal-distance touch ordering is deliberately unspecified. |
| [Scroll](../../crates/markview-interaction/src/scroll/proptests.rs) | Requested destination versus displayed offset under partial/complete bounds, actual intermediate `Scroll` frames and active state, pending-target animation restart, endpoints and cancellation. Continuous wheel momentum/prediction is outside this phase. |
| [Microtype](../../crates/markview-layout/src/microtype/proptests.rs) | Capacity and direction constraints plus independently summed gains, including unfillable requests. No claim that every target is feasible. |
| [Line breaking](../../crates/markview-layout/src/linebreak/proptests.rs) | Ordered content conservation, trimmed glue, visible widths/hyphens, budget fallback and exhaustive short legal partitions that must take the normal path. No general optimality proof; fallback may advance at nonoptional boundaries. |
| [Semantic search](../../crates/markview-layout/src/search/proptests.rs) | Builder-created paragraph/heading/code/block-cell table/list/nested-disclosure/definition-list/figure fields with an independent field list, revision and typed identity checks, nonoverlapping literal scans, ASCII folding, chunk boundaries and cancellation. Whole-word checks cover stability/subset behavior; full ICU segmentation and Unicode case folding proofs are deferred. |

These tests complement the [fuzz harnesses](../../fuzz/README.md): property tests
exercise local semantic objects and short sequences, while fuzzing explores
coverage-guided composition and full pipelines. Preserve existing example and
integration tests. Reuse relevant properties when changing an API; new properties
are necessary when the current evidence does not constrain the changed behavior.
