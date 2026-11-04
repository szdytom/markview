# Markview Integration delivery

This follow-up builds on the current main's WASM preview and native export services;
it replaces the older unmerged Path B implementation rather than porting that code.
Marketplace publication and release tags require a final user decision after review.

## Acceptance

| ID | Requirement | Evidence / status |
| --- | --- | --- |
| REL-1 | Display name Markview Integration; retain `Stevvven.markview-export`; version higher than published 0.1.2 | Manifest 0.4.0; real old-to-new installed upgrade passes. Review approved, round 2 (`integration-review-2.log`). |
| REL-2 | Legacy export commands and document-scoped template paths continue working; explicit new settings win | Node integration tests and installed upgrade PDF/PNG pass. Export-only legacy templates do not apply to the reading preview. Review approved, round 2 (`integration-review-2.log`). |
| REL-3 | Store README, icon, license and changelog ship in the VSIX | Local darwin-arm64 VSIX packaged. Existing Markview icon/license reused. Review approved, round 2 (`integration-review-2.log`). |
| REL-4 | Six matched native packages, gated tests and same-artifact GitHub/Marketplace publication | New package/release workflows and architecture verifier; hosted builds pending. No tag pushed. |
| REL-5 | Installed extension renders unsaved edits, exports PDF/PNG and cleans up | Real VS Code old-to-new upgrade passes locally; cloud installed smoke pending. |

## Validation record

Logs are retained outside the worktree under `artifacts/integration-*.log`.
Native and WASM release builds pass. The 17 Node tests, 11 browser tests and
2 native service integration tests pass. The installed VSIX upgrade test uses
an actual Marketplace 0.1.2 package and an isolated profile, preserves its old
template setting, renders an unsaved buffer and exports both formats. Browser
acceptance waits for a previous edit's animation before asserting that the next
visible edit does not move the viewport; production scrolling is unchanged.

The publishing job is restricted to the personal fork and the `marketplace`
environment. It requires `VSCE_PAT`; no credential is committed, and the secret
is not configured at implementation time. The reader's cargo-dist workflow is
unchanged. Package success is distinct from runtime verification on every OS.

Review round 1 rejected missing Python 3.11+ on Ubuntu 22.04 and language-unaware
legacy template lookup. Round 2 approved explicit Python 3.12 setup and
language-aware scopes, including actual VS Code `[markdown]` upgrade assertions.
Final local evidence: `integration-lint-6.log`, `integration-node-6.log`,
`integration-browser-6.log`, `integration-native-tests-2.log` and
`integration-installed-6.log`. All are completed passing logs; six-platform
hosted builds remain pending at commit time.
