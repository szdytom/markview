# Write property tests

Use a property test when a contract spans many inputs or operation sequences.
Keep a small ordinary regression when a specific boundary must execute on every
run. Property tests supplement integration and visual tests.

## Run and reproduce

Property modules are named `proptests` and run as ordinary Rust tests:

```sh
cargo test --workspace --locked --lib proptests::
PROPTEST_CASES=4096 cargo test --workspace --locked --lib proptests::
cargo test -p xtask --locked proptests::
```

Select one package and a longer test filter when diagnosing a failure. See
[testing](testing.md) for native build prerequisites. CI runs the property suite
separately in `pbt.yml`; the workflow owns its case budgets and artifact retention.

Keep Proptest's regression seeds in Git. Reproduce with the saved seed and the
same strategies/version; `PROPTEST_RNG_SEED` takes a numeric exploration seed,
not the persisted `cc` hash. For a confirmed product defect, also keep a concrete
regression test: a saved seed can change meaning when its generator changes.

## Define the contract first

State the input domain, preconditions, observable result and allowed fallback.
Justify the guarantee from an API contract or caller requirement. Do not freeze
an incidental implementation choice because it is convenient to assert.
Stable API guarantees belong in source documentation; a separate prose inventory
of every property quickly becomes another implementation to maintain.

Use a simpler, independent oracle: a set, a prefix table, a small exhaustive
model or a sequence of requested destinations. Computing expected results with
the production function, or only checking paired round trips, can hide matching
bugs on both sides.

## Generate and shrink meaningful inputs

Construct valid identities and state relationships directly, and preserve them
under shrinking. Bound text lengths, collections and operation sequences.
Include empty, singleton, duplicate, adjacent and exact/just-over-limit cases.
Lowering the number of cases must not remove an input category from the domain.

For numeric properties, prevent intermediate overflow where the contract requires
it. Use exact integers/binary fractions when appropriate; otherwise state an
error bound in meaningful units. Finiteness alone does not establish accuracy.

Stateful tests start fresh and check after each operation. Include replacement,
cancellation, empty flushes and completion after removal. Inject time and effects
instead of sleeping or reaching real network services. Keep the model smaller
than the implementation.

## Review a failure

1. Find the first divergent step, rather than only the final mismatch.
2. Decide whether the implementation, oracle or stated contract is wrong.
3. Preserve the reduced input and diagnosis. Shrinking finds a smaller example,
   not necessarily the globally smallest one.
4. Fix the owner, then rerun the focused property and relevant integration tests.

Check that a property would fail if its intended behavior broke. Review the
oracle's independence and the generator's reachable states, not just its case
count. Historical expansion reports and defects live in
[the investigation archive](history/property-testing.md); fuzzing procedures live
in [fuzz/README.md](../../fuzz/README.md).
