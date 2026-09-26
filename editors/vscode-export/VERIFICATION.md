# Export-only submission verification — 2026-09-26

Base: upstream `main` at `67f1955`; extension 0.1.2; bundled engine 0.1.8.
The candidate was rebuilt from this branch, not the old 0.1.4 development binary.

- `cargo test --workspace --all-targets --locked`: 758 passed, 0 failed, 19 ignored across 13 test suites.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo fmt --all --check` and shell syntax checks: passed.
- TypeScript compilation and picker/context-menu command tests: passed.
- Installed-VSIX tests: PDF/PNG, five bundled templates, custom MVSS, dirty buffers,
  untitled documents, scoped defaults, None override, invalid template rejection,
  source-overwrite refusal, A5 template geometry, read-only source folders,
  multi-tile SVG scaling and engine exit all passed.
- Extracted PDF text confirms template exports use the unsaved buffer and `{path}` uses the source path.
- The packaged engine matches the new release binary byte for byte; no preview
  panel is packaged. VSIX SHA-256: `c6bf61e1e87f2fe57fbb700a701a2376716dfe49bfb981a61be1ee297e2bcace`.
- Native process integration verifies private template storage, no settings-file
  parsing, malformed-request recovery and termination on stdin EOF, including a blocked resource read.

Local evidence: `/private/tmp/markview-export-tests.log`,
`/private/tmp/markview-export-clippy.log`, `/private/tmp/markview-export-build.log`,
and `dist/integration-main.log`. These generated logs are not committed.
Git dependencies were cached locally from the exact locked GitHub commits to
work around slow Git transfers; the dependency manifest and lockfile are unchanged.

Fresh-machine macOS distribution, other platform VSIX packages and Marketplace
remain unverified; the user has registered personal publisher `Stevvven`. No Marketplace release has been made.

Review gate: **approve**, round 4 (`dist/review-main-4.log`). Round 2 found
page geometry, read-only source, path header, blocked EOF and SVG issues; round 3
found remaining SVG demand loss across tiles. Round 4 independently verified
short, first-tile and last-tile SVG cases with no remaining findings.

## Marketplace candidate — 2026-09-27

- Identity: `Stevvven.markview-export`, version 0.1.2, target `darwin-arm64`.
- The native engine is byte-identical to the reviewed package above; no Rust code changed.
- TypeScript compilation, picker tests and installed-VSIX export/lifecycle tests passed
  under the new publisher identity (`dist/integration-marketplace.log`).
- Candidate VSIX SHA-256: `c1f029e57b00676720271a4b96cbc25c98dce5631aa6e72bfa76d46548489fbe`.
- Publisher metadata review: **approve**, first round (`dist/review-marketplace.log`). No Marketplace upload has been performed.
