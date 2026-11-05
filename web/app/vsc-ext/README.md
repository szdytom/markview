# Editor font services

This private Node package supplies reusable font services for editor integrations.
The VS Code development extension registers the font management command.

```sh
npm ci --prefix web/app/vsc-ext
npm --prefix web/app/vsc-ext run lint
npm --prefix web/app/vsc-ext test
```

`FontIndex` enumerates system fonts, the download cache and configured directories.
It reads internal and localized family names from fonts and TTC collections, caches
metadata by path/size/mtime, and resolves a template's ordered family candidates.
`statuses()` reports matches and download families; `paths()` includes the matched
family's faces from the preferred source. This checks family availability, not glyph
coverage or exact rendering equivalence. Unreadable font files are skipped.

`installFamily()` stages a validated download, replaces one family's cache directory
and records its declared license/source metadata. Failed or cancelled downloads keep
the prior installed family. Callers serialize installations for the same family.
`runNative()` transports stdin and JSON progress to/from a subprocess; cancellation
uses an export marker followed by bounded process termination.

Tests use committed font fixtures and temporary directories. No internet downloads
or globally installed fonts are needed for deterministic matching tests.

`FontClient` wraps the native catalog/download protocol and snapshots custom
templates for batch operations. It requires a compatible `markview fonts` CLI;
scanning and cache operations remain usable without a native binary.

## Font manager development

Uses the native font CLI and reusable editor font services.

```sh
npm --prefix web/app/vsc-ext run build
cd web/app/vsc-ext
npx playwright install chromium
npm run test:browser
MARKVIEW_BINARY=/absolute/path/to/markview node --test tests/native-fonts.mjs
MARKVIEW_BINARY=/absolute/path/to/markview npm run package
```

Install the generated platform-specific VSIX from `artifacts/vscode`, then open
**Markview: 字体管理…**. The CLI binary must support `fonts catalog` and `fonts download --json`.
Browser tests exercise the production panel host and DOM with mocked VS Code
services; native transport tests exercise the real CLI separately.

Use the M icon in a Markdown editor title to open a side-by-side reading preview.
The preview renders unsaved edits with MVaaC/WASM, follows the editor color theme,
supports selection/copy, local and remote images, links and a visible scrollbar.
Saved reading position is restored when VS Code reloads the window.

This slice does not add synchronized scrolling or configurable reading templates.
Fonts are resolved through the existing native font catalog and local font index.
Only local trusted workspaces are supported.

Right-click a Markdown editor to export PDF or PNG, or choose a built-in or custom MVSS template before exporting.

## Development

```sh
npm ci --prefix web/app/vsc-ext
node web/scripts/wasm.mjs --setup
node web/scripts/wasm.mjs
npm --prefix web/app/vsc-ext run lint
npm --prefix web/app/vsc-ext test
cargo build --release --locked
cd web/app/vsc-ext
npx playwright install chromium
npm run test:browser
MARKVIEW_BINARY=/absolute/path/to/markview node --test tests/native-export.mjs
MARKVIEW_BINARY=/absolute/path/to/markview npm run package
```

VSIX output is written to `artifacts/vscode`. Build WASM and the native binary
from the same checkout before packaging.

## Markview Integration delivery

The Marketplace identity is `Stevvven.markview-export` (display name **Markview Integration**).
The extension version is independent of the native workspace version. Package
WASM and the native executable from the same checkout. `package.mjs` refuses
an exporter whose architecture does not match the requested platform. Windows targets statically link the C runtime; CI and the final package verifier reject extra VC++ runtime DLL imports.

`extension-packages.yml` tests and packages all six desktop targets inside the
upstream reader's `Packaging` workflow, on PRs and release tags, and also supports
manual dispatch. cargo-dist attaches the verified VSIX files and
`SHA256SUMS-vscode` to the same reader Release.

After a stable release, `extension-release.yml` downloads and verifies those
exact attachments and publishes them from `szdytom/markview` using OIDC Trusted
Publishing. Marketplace failure leaves GitHub downloads available; retry the
published tag without rebuilding. See the [publishing setup and version
policy](../../../docs/developers/integration-release.md#upstream-release-and-marketplace)
for one-time publisher authorization, independent extension versions and retries.
