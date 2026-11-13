# Web component development

[Developer documentation](README.md) · [Component user guide](../library/mvaac.md)

## Workspace

- `packages/markview`: `@markview/viewer`, including sibling WASM asset.
- `packages/editor`: `@markview/editor`, CodeMirror and preview composition.
- `packages/scroll-sync`: `@markview/scroll-sync`, editor-independent anchor projection and scroll coordination.
- `packages/fonts`: optional `@markview/fonts` explicit loading/cache.
- `packages/resources`: optional `@markview/resources` browser transport.
- `packages/web`: deprecated `@markview/web` compatibility entry.
- `apps/demo`: one reading/editing SPA at `/index.html`, with shared source, history, and reading position.
- `/editor.html` redirects to `/index.html#edit` for existing links.
- `tests/fixtures/reader`: legacy renderer regression host, built only by `pnpm test`.

Package paths above are relative to `web/`. Public consumer contracts live in
[the library guides](../library/README.md). The demo supplies its own network fonts;
tests use pinned subsets. Keep CDN selection and cache policy in the demo, not
in the reusable viewer.

## Build and test

### First build

Install Node.js 22 or newer, pnpm, Python 3.11 or newer and
[Rust via rustup](https://rustup.rs). Add them to `PATH` and reopen your terminal
after installation. On Windows, follow rustup's prompt to install the Visual
Studio C++ build tools; they compile the bindings tool. You do not need the
native Markview application's windowing or PDF dependencies to build MVaaC.

Run these commands from the repository root in PowerShell, cmd, or a Unix shell:

```sh
pnpm --dir web install
pnpm --dir web run setup
pnpm --dir web build
pnpm --dir web serve
```

Open `http://127.0.0.1:4173/index.html`. `setup` installs the browser compilation
target and a matching `wasm-bindgen` tool under the ignored `.tools/` directory.
It reuses an existing matching tool. The version comes from the Rust package's
`Cargo.toml`; you do not need to maintain a second version pin. Initial setup
and compilation need network access and can take several minutes.

### Everyday TypeScript work

After the first build, changes in `web/` only need:

```sh
pnpm --dir web build:ts
pnpm --dir web serve
```

Reload the browser after rebuilding. If you change the Rust engine, run
`pnpm --dir web build` again; Cargo reuses unchanged code. `build:ts` also works
with existing generated bindings when Rust is unavailable on the machine.

| Command | What it does |
| --- | --- |
| `setup` | Prepare the Rust browser target and matching bindings tool |
| `build:wasm` | Compile the engine and generate JavaScript/TypeScript bindings |
| `build:ts` | Bundle all packages, emit declarations and build the demo |
| `build` | Run `build:wasm`, then `build:ts` |
| `serve` | Serve the built demo over HTTP |

Think of WASM as the engine asset that the TypeScript viewer loads, like an
image or font asset. The build has three outputs:

1. `web/packages/markview/wasm/`: generated engine bindings and binary.
2. `web/packages/*/dist/`: public ESM packages, declarations and the viewer's
   sibling WASM binary.
3. `web/dist/`: the static demo site, ready to serve or deploy.

The build scripts locate the repository and Cargo output directory automatically.
If overriding `WASM_BINDGEN`, its version must match the Rust binding dependency.
Use the package scripts unless diagnosing the build itself.

### Validation

Build first, then run:

```sh
pnpm --dir web typecheck
python3 scripts/test_web_documentation.py
pnpm --dir web exec playwright install chromium
pnpm --dir web test
pnpm --dir web test:packages
```

Linux may also need `pnpm --dir web exec playwright install-deps chromium`.
Build-script regression tests need no Rust compilation or browser:
`python3 scripts/test_build_web.py` (`py -3 scripts/test_build_web.py` on Windows).

`web/build.mjs` builds the
viewer, helpers, compatibility entry and editor in dependency order, emits type
declarations, copies WASM, then bundles examples using built entries. Tests
also install actual tarballs into an isolated consumer, typecheck with `skipLibCheck: false`, bundle without source aliases and initialize/mount/destroy in Chromium. The normal suite starts with DOM-free Node tests of the scroll core, then runs the original reader regressions, built viewer/source navigation, real
CodeMirror scrolling/editing, Chinese composition, resize, TOC and lifecycle.

## Source navigation verification

`cargo test -p markview-layout --test source` verifies Unicode coordinates, CRLF,
long paragraphs/code, cells/quotes, cached geometry, deferred targets and nested,
adjacent and quoted disclosure headings. `pnpm --dir web test
 tests/navigation.spec.mjs` exercises the built package with real canvas rendering,
version replacement, pending navigation, focus, reflow, input cancellation and
repeated mounting. Existing reader/resource/font regression suites remain in use.

## Renderer and GPU checks

Use `MV_GPU=1 pnpm --dir web test --project=chromium-gpu` for the opt-in Vulkan
browser project. Default browser tests use SwiftShader. Native engine tests run
with `cargo test -p markview-commonmark -p markview-document -p markview-layout`; WASM lint runs with
`cargo clippy -p markview-web --target wasm32-unknown-unknown --features woff -- -D warnings`.
