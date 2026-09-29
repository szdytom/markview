# Maintaining Markview4vsc

`editors/vscode` is the sole extension package. Its Marketplace identity stays
`Stevvven.markview-export`, so existing Better markdown PDF installations upgrade
in place. `displayName` is Markview4vsc. Do not rename `name` or `publisher`.
Uninstall the separate local `Stevvven.markview-preview` development extension.

## Code and compatibility

- `src/extension.ts` owns activation and one lazy engine per window.
- `src/export.ts` owns PDF/PNG commands, including `markviewExport.*` aliases.
- `src/template.ts` resolves document-scoped settings for both preview and export.
- `src/panel.ts` displays native pixels and maps editor interactions.
- `editors/shared` owns the framed protocol client and binary validation.
- Rust owns layout, rasterization, templates and export. Engine updates normally
  require rebuilding the bundled binary and running these checks; protocol
  changes require updating the client in the same PR.

`markview.template` wins over `markviewExport.template` when explicitly set,
including an empty value. Old command IDs stay registered but are hidden from
the palette; editor context menus use the canonical commands. A command URI
means the source document; programmatic destinations use `{ target: uri }`.

## Validation

Run `cargo test --workspace --locked --all-targets`, `cargo fmt --all --check`
and `cargo clippy --workspace --locked --all-targets -- -D warnings`.
Raster tests are separate: `cargo test --locked -p markview --lib serve::tests -- --ignored --test-threads=1`.
Build with `cargo build --release --locked`, then:

```sh
cd editors/vscode
npm ci
npm run compile
node test/pickers.js
node --test ../shared/test/frames.cjs
./package-vsix.sh darwin-arm64
MARKVIEW_VSIX="$PWD/dist/markview-export-darwin-arm64.vsix" ./run-tests.sh
PATH="$PWD/node_modules/.bin:$PATH" python3 test/reload-window.py
```

The local host defaults to installed macOS VS Code; `VSCODE_EXEC_PATH` overrides
it. CI downloads stable VS Code and tests the installed Linux VSIX under Xvfb
with software Vulkan. CPU protocol and picker tests run without a GUI.
`preview-extension.yml` builds all six native platform packages. Successful
packaging is not evidence that every platform's GPU/driver has been exercised.
Requirements close only through the review gate documented in
[plugin-requirements.md](plugin-requirements.md).

## Release

Bump `editors/vscode/package.json` and its lockfile, update the extension changelog,
and merge the reviewed PR. Configure a Marketplace publishing credential as the
repository/environment secret `VSCE_PAT`; never commit or log it. The optional
`marketplace` GitHub environment can require a human approval.

Push an annotated `markview4vsc-v<version>` tag on the reviewed commit.
`extension-release.yml` checks the tag version, runs CI and builds all six VSIXs,
then publishes those exact artifacts with `vsce`. A test/build failure prevents
publication; a failed partial upload can be retried using `--skip-duplicate`.
This tag does not release the standalone Markview reader.

Credential setup and any configured environment approval are the only manual
steps outside version/release authorization. GitHub CI does not replace the
local review-gate approval or manual dialog usability checks.
