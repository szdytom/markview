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
- Candidate VSIX SHA-256: `ab4ca2a69fa6e514f292f9584187d368259a058b2ca6319e25d71905c5ffd670`.
- Publisher metadata review: **approve**, first round (`dist/review-marketplace.log`). No Marketplace upload has been performed.

Display name updated to **Better markdown PDF**, including command titles and
settings. The extension ID stays `Stevvven.markview-export`. Repackaging and
installed-VSIX tests passed after the rename; name-change review **approve** (`dist/review-final-name.log`).

README refresh: linked MVSS authoring guide confirmed through GitHub API; JSON
settings and section anchors validated. The exact starter template passes
`ss validate` and exports PDF/PNG offline (`dist/readme-validation.log`).
Installed-VSIX tests pass (`dist/integration-readme.log`); documentation review **approve** (`dist/review-readme.log`).

Final submission candidate: removed the redundant installation section per user
review. Package identity and native binary verified; packaged README differs
only by vsce expanding the relative Changelog link. Installed-VSIX tests pass
(`dist/integration-final.log`). Final README review **approve** (`dist/review-final.log`).

Marketplace submission — 2026-09-27: version 0.1.2, publisher Stevvven,
display name Better markdown PDF, target darwin-arm64. The publisher management
page confirms the uploaded extension is Public with status **Verifying**.
This submission record supersedes the earlier not-yet-uploaded candidate notes.
