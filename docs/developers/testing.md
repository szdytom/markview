# Test a change

Start with the behavior being changed, then choose the cheapest test that can
observe it. Run broader integration checks when the change crosses a boundary.

| Change | Verification |
| --- | --- |
| Parsing, identity, selection or geometry | Owning crate tests; [property tests](property-testing.md) for general contracts |
| Layout, styles, renderer or PDF | [Rendering suite](rendering-tests.md) plus semantic assertions |
| UI state or input | Pure UI tests, reader integration tests, native interaction checks |
| Search | [Search checks](search-verification.md), including real IME behavior |
| Resource authority or lifecycle | [Security checks](security-verification.md) and stale-completion tests |
| Web packages | [Web build and consumer tests](web.md#build-and-test) |
| Android | [Emulator verification](android.md#emulator-verification) |
| Packaging | [Package verification](packaging.md) |
| Documentation | `python3 scripts/test_documentation.py` and relevant examples |

## Native suite setup

Run from the repository root:

```sh
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Offscreen tests need a graphics backend but no physical GPU or window. On Linux,
select Mesa Lavapipe and hide system fonts to reproduce the controlled setup:

```sh
driver=$(find /usr/share/vulkan/icd.d -name 'lvp_icd*.json' -print -quit)
test -n "$driver"
VK_DRIVER_FILES="$driver" WGPU_BACKEND=vulkan \
  FONTCONFIG_FILE="$PWD/tests/fontconfig.conf" \
  cargo test --workspace --all-targets --locked
```

The driver path depends on the distribution. Debian/Ubuntu provides Lavapipe in
`mesa-vulkan-drivers`; Arch uses `vulkan-swrast`. Follow `AGENTS.local.md`, when
present, for machine-specific paths. Windows tests can use WARP; macOS uses Metal.

CI separates regular tests, property tests and Linux visual baselines. See the
[workflows](../../.github/workflows) for runner and dependency pins. Do not
copy those pins into this guide. Visual comparisons are run explicitly through
`scripts/test_render_goldens.sh`.

## Complexity audit

```sh
cargo run -p xtask --locked -- complexity
```

The limit lives in `workspace.metadata.lint` in `Cargo.toml`. Refactor a violation
when it obscures behavior. When a cohesive exhaustive mapping should remain
intact, document the reason on that function:

```rust
#[cfg_attr(any(), markview::allow_complexity(reason = "Keep the exhaustive mapping together"))]
fn map_kind(kind: Kind) -> Value {
    // ...
}
```

The marker applies only to that body, not nested closures or an entire module.
For a closure, annotate its expression inside a block. The checker is a source
AST audit; it does not expand macros. Its implementation owns the scoring rules.

## Regression evidence

Keep minimized failing inputs and assertions about behavior. Use independent
oracles for property tests. Record visual diffs for visible changes and paired
[measurements](performance.md) for performance claims. A simulated input event
does not prove native IME, desktop handoff or physical display behavior.
