# Direct web extraction comparison

> Archived from `37f22d622d41d5409321b8db7cf9f36b38b0d5a1`. The baseline revision below identifies the old implementation; these commands reproduce that investigation, not a general performance gate.

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
