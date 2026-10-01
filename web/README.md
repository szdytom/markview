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
- `packages/fonts`: optional `@markview/fonts` explicit loading/cache.
- `packages/resources`: optional `@markview/resources` browser transport.
- `packages/web`: deprecated `@markview/web` compatibility entry.
- `apps/editor`: split editing example, built as `/editor.html`.
- `apps/demo`: original reader regression host, built as `/index.html`.

Official WASM enables WOFF/WOFF2; hosts explicitly supply per-instance font sets.

## Build and validate

```sh
pnpm --dir web install
scripts/build-web.sh
pnpm --dir web build
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web test:packages
pnpm --dir web serve
```

Build examples and tests consume package `dist` entries rather than source
aliases. The static site is `web/dist`. Serve it through HTTP, not `file:`.
WASM is copied under `markview_web_bg.wasm`; downstream bundlers can import the
`@markview/viewer/wasm` asset or provide its deployed URL explicitly.

Use `MV_GPU=1 pnpm --dir web test --project=chromium-gpu` for the opt-in Vulkan
browser project. Default browser tests use SwiftShader. Native engine tests run
with `cargo test -p markview-core`; WASM lint runs with
`cargo clippy -p markview-web --target wasm32-unknown-unknown --features woff -- -D warnings`.
