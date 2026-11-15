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

The regular CI suite and Clippy also run on native ARM64 Linux. Its renderer
checks cover Vulkan, forced GLES, and automatic GLES fallback with the Vulkan
driver unavailable. To reproduce the fallback check on ARM Linux with Mesa:

```sh
env -u WGPU_BACKEND VK_DRIVER_FILES=/nonexistent-vulkan-icd.json \
  EGL_PLATFORM=surfaceless LIBGL_ALWAYS_SOFTWARE=1 \
  cargo test -p markview-render --locked --test render gles_renders_markdown -- --ignored --exact --nocapture
```

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

## Real-file test paths

Use `markview_test_support::TestDir` for real storage in tests and fuzz harnesses.
Application unit tests re-export it as `crate::test_support::TestDir`. The small
`markview-test-support` crate is a development dependency of fixture consumers;
it does not add platform services to the reading core or the shipped reader.

The fixture owns the temporary directory and exposes a canonical root through
`path()`. Create files and derive expected paths from that root when constructing
session state or matching directory-tree entries. Canonicalize an existing target
separately when its path contains symlinks or `..`. Canonical path equality is a
state representation contract; use the production file-identity helper when
testing identity through hard links.

Keep virtual paths for display and URI-conversion tests. Paths to missing files
cannot be canonicalized; derive them from the fixture root. Tests of raw user
input should pass raw paths through the application entry point and assert the
resulting state, rather than normalizing the input in advance. Test helpers must
keep the fixture alive while its files are in use; do not leak directories with
`TempDir::keep()` to return a bare path.

```sh
cargo run -p xtask --locked -- test-paths
cargo test -p xtask --locked test_paths::
```

The audit discovers all tracked and untracked, nonignored Rust source files
through Git, including new crates, integration tests, tools and fuzz harnesses.
It parses every platform branch without expanding macros. It rejects direct
`tempfile::tempdir*` and `tempfile::TempDir` constructor expressions, directory
API imports with aliases, and builder `.tempdir*()` calls. Fully qualified storage types and named
file operations remain available for production storage and caller-chosen roots.

Functions whose contract explicitly requires raw paths need a reason:

```rust
#[test]
#[cfg_attr(any(), markview::allow_raw_paths(reason = "Exercise raw paths at the application entry point"))]
fn raw_path_regression() {
    // Raw user input reaches the production entry point.
}
```

The exception applies to that function body, including closures, but not nested
or sibling functions. Imports needed by an exception belong inside its body.
The production platform directory factory and the canonical fixture constructor
also document their raw-storage contracts with this marker. The audit ignores
comments, strings and attributes, and inspects literal directory API references
in macro arguments. It does not trace re-exports, helper calls or assignments
into `session.path`.

For a Unix regression run, use the normal font and graphics environment described
above, then run:

```sh
python3 scripts/test_paths.py
python3 scripts/test_paths.py --diagnostics
```

The runner builds all workspace test binaries once and runs them in separate
processes with ordinary and symlinked `TMPDIR` roots. It includes property tests;
ignored tests and `rendering_baselines` retain their dedicated runners. It counts
only runnable tests, allows empty individual targets, rejects an empty overall
run, and prints counts and timings. Pass `--binary /absolute/path/to/test-binary`
(repeatable) to reuse existing binaries. The runner never changes the parent
process environment.

Linux x64 CI uses `--symlink-only` after its ordinary native suite. Other native
jobs retain Windows and macOS coverage; Unix symlinks exercise local canonical
path mismatches without simulating Windows-specific path behavior.

## Regression evidence

Keep minimized failing inputs and assertions about behavior. Use independent
oracles for property tests. Record visual diffs for visible changes and paired
[measurements](performance.md) for performance claims. A simulated input event
does not prove native IME, desktop handoff or physical display behavior.
