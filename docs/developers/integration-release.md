# Markview Integration delivery

This follow-up builds on main's WASM preview and native export services.
The implementation, local update and review are complete. Marketplace publication,
release tags and merging remain subject to the user's final decision.

## Acceptance

The final `gpt-6-astra` / medium review approved REL-1..5 on 2026-11-04
(`integration-review-final.log`). It inspected the committed implementation and
independently reran all 17 Node tests and verification of all six downloaded VSIX
files. Product code was verified at `e6c9797`; this record adds evidence only.

| ID | Requirement | Evidence / status |
| --- | --- | --- |
| REL-1 | Display name Markview Integration; retain `Stevvven.markview-export`; version higher than published 0.1.2 | **Approve.** Manifest 0.4.0 and real Marketplace 0.1.2-to-new installed upgrade pass. |
| REL-2 | Legacy export commands and document-scoped template paths continue working; explicit new settings win | **Approve.** Node integration tests and real VS Code `[markdown]` settings assertions pass. Export-only legacy templates do not override the reading preview. |
| REL-3 | Store README, icon, license and changelog ship in the VSIX | **Approve.** Packaged store files checked; existing Markview icon/license reused. |
| REL-4 | Six matched native packages, gated tests and same-artifact publication automation | **Approve, implementation.** All six hosted builds and actual downloaded packages pass identity/version/architecture validation. Both Windows binaries also pass independent LLVM runtime-import checks. Publication is deferred. |
| REL-5 | Installed extension renders unsaved edits, exports PDF/PNG and cleans up | **Approve.** Real old-to-new upgrade and a repeat using the actual CI-produced macOS package pass. Linux installed-host smoke and clean shutdown pass in CI. |

## State of play

PR [#89](https://github.com/szdytom/markview/pull/89) contains the implementation,
rebased onto main `a735959`. Version 0.4.0 is installed locally under the existing
Marketplace identity. Six platform-specific VSIX files and `SHA256SUMS` are
retained under `artifacts/integration-release-packages` in the working checkout's
parent artifacts directory. These are the exact files from one successful run.

Completed hosted verification:

- [Six-platform package workflow](https://github.com/szdytom/markview/actions/runs/41846873518).
- [Main CI](https://github.com/szdytom/markview/actions/runs/41846873646).
- [Editor CI](https://github.com/szdytom/markview/actions/runs/41846873700).

No release tag was pushed and no Marketplace upload was made. Publishing requires
`VSCE_PAT` in the personal fork's `marketplace` environment. No credential is
committed. The reader's version and cargo-dist release workflow are unchanged.
Package validation is distinct from physical GPU validation on every device;
Windows ARM64 is cross-built, while installed VS Code behavior is exercised on
macOS and Linux.

## Validation record

Completed logs are retained under the parent artifacts directory:

- Native/WASM builds: `integration-native-main-sync.log`, `integration-wasm-main-sync.log`.
- Formatting/types, 17 Node tests and 11 browser tests: `integration-lint-main-sync.log`, `integration-node-main-sync.log`, `integration-browser-main-sync.log`.
- Native service tests: `integration-native-tests-main-sync.log`.
- Actual Marketplace upgrade, scoped settings and dirty-buffer export: `integration-installed-main-sync.log`, `integration-installed-ci-package.log`.
- Six actual archive checks and independent Windows DLL checks: `integration-final-artifact-verification.log`, `integration-independent-windows-runtime.log`.
- Completed hosted logs/snapshots: `integration-cloud-packages-final.log`, `integration-cloud-{packages,ci,fonts}-final.json`.
- Final review: `integration-review-final.log`.

Review round 1 caught missing Python 3.11+ on Ubuntu 22.04 and language-unaware
legacy lookup. Round 2 approved Python 3.12 setup and language-aware configuration
scopes, including real VS Code assertions. Artifact inspection then found a
Windows VC++ runtime dependency: target-specific static linking and import checks
fix it, with an old binary rejected and both new binaries verified. The final
review covers these fixes and the completed CI/artifact results.

An existing browser edit-follow test sampled a preceding animation in flight.
It now waits for that animation to settle before checking the next visible edit;
the exact position assertion and production scrolling behavior are unchanged.
