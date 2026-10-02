# Markview Web components

Start with [the MVaaC component guide](../docs/mvaac.md) for package ownership,
quick start, editor/viewer APIs, resource injection, deployment and migration.
[Source navigation and TOC](../docs/mvaac-source-api.md) defines UTF-16 offsets,
document versions, geometry and scrolling behavior. The
[initial demo contract](../docs/mvaac-web-demo.md) is preserved as historical
reference; its scope exclusions do not apply to the reusable components.

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

Official WASM enables WOFF/WOFF2; hosts explicitly supply per-instance font sets.

## Build and validate

Install Node.js 22+, pnpm, Python 3.11+ and [Rust via rustup](https://rustup.rs).
From the repository root, the same commands work in Windows shells, macOS and Linux:

```sh
pnpm --dir web install
pnpm --dir web run setup
pnpm --dir web build
pnpm --dir web serve
```

`setup` prepares the browser target and matching bindings tool automatically.
Use `pnpm --dir web build:ts` for subsequent TypeScript-only changes and reload
the page. `build` includes an incremental WASM build; `build:wasm` rebuilds only
the engine. See [build and test](../docs/mvaac.md#build-and-test) for prerequisites,
generated directories and advanced tool/output-path configuration.

To validate a build:

```sh
pnpm --dir web typecheck
pnpm --dir web exec playwright install chromium
pnpm --dir web test
pnpm --dir web test:packages
```

Build examples and tests consume package `dist` entries rather than source
aliases. The static site is `web/dist`. Serve it through HTTP, not `file:`.
WASM is copied under `markview_web_bg.wasm`; downstream bundlers can import the
`@markview/viewer/wasm` asset or provide its deployed URL explicitly.

Use `MV_GPU=1 pnpm --dir web test --project=chromium-gpu` for the opt-in Vulkan
browser project. Default browser tests use SwiftShader. Native engine tests run
with `cargo test -p markview-core`; WASM lint runs with
`cargo clippy -p markview-web --target wasm32-unknown-unknown --features woff -- -D warnings`.
