# Markview Integration delivery

This follow-up builds on main's WASM preview and native export services.
The implementation, local update and review are complete. Marketplace publication,
release tags and merging remain subject to the user's final decision.
The upstream Release/Trusted Publishing follow-up passed its final Astra review
on 2026-11-05. Hosted execution of the updated workflows and actual publication
remain to be established.

## Acceptance

The final `gpt-6-astra` / medium review approved REL-1..5 on 2026-11-04
(`integration-review-final.log`). It inspected the committed implementation and
independently reran all 17 Node tests and verification of all six downloaded VSIX
files. That product validation used `e6c9797`; the upstream publishing follow-up
was subsequently rebased onto main `d3a2512`.

| ID | Requirement | Evidence / status |
| --- | --- | --- |
| REL-1 | Display name Markview Integration; retain `Stevvven.markview-export`; version higher than published 0.1.2 | **Approve.** Manifest 0.4.0 and real Marketplace 0.1.2-to-new installed upgrade pass. |
| REL-2 | Legacy export commands and document-scoped template paths continue working; explicit new settings win | **Approve.** Node integration tests and real VS Code `[markdown]` settings assertions pass. Export-only legacy templates do not override the reading preview. |
| REL-3 | Store README, icon, license and changelog ship in the VSIX | **Approve.** Packaged store files checked; existing Markview icon/license reused. |
| REL-4 | Six matched native packages, gated tests and same-artifact publication automation | **Approve, implementation.** All six hosted builds and actual downloaded packages pass identity/version/architecture validation. Both Windows binaries also pass independent LLVM runtime-import checks. Publication is deferred. |
| REL-5 | Installed extension renders unsaved edits, exports PDF/PNG and cleans up | **Approve.** Real old-to-new upgrade and a repeat using the actual CI-produced macOS package pass. Linux installed-host smoke and clean shutdown pass in CI. |
| REL-6 | Upstream reader releases attach all six verified VSIX files and their checksums | **Approve, implementation.** Complete-set staging and real six-package checksum handoff pass; cargo-dist generation and workflow checks pass. Hosted execution remains pending. |
| REL-7 | Stable upstream releases publish their exact attachments with OIDC; store failure does not block GitHub assets | **Approve, implementation.** Stable-release resolution, rejection paths and exact-package publish commands pass. Actual OIDC exchange/upload remain deferred. |
| REL-8 | Release notes expose the correct extension version/platforms; existing ID and independent version policy remain explicit | **Approve.** Six release tests and documentation checks pass; independent versions, immutable identity, authorization and retries are documented. |

## State of play

PR [#89](https://github.com/szdytom/markview/pull/89) contains the implementation,
rebased onto main `d3a2512`. Version 0.4.0 is installed locally under the existing
Marketplace identity. Six platform-specific VSIX files and `SHA256SUMS` are
retained under `artifacts/integration-release-packages` in the working checkout's
parent artifacts directory. These are the exact files from the earlier successful run, before this rebase.

Completed hosted verification of the earlier product revision:

- [Six-platform package workflow](https://github.com/szdytom/markview/actions/runs/41846873518).
- [Main CI](https://github.com/szdytom/markview/actions/runs/41846873646).
- [Editor CI](https://github.com/szdytom/markview/actions/runs/41846873700).

No release tag was pushed and no Marketplace upload was made. The follow-up
replaces the personal fork's extension-specific tag/PAT workflow with upstream
release attachments and Trusted Publishing. No credential is committed, and
neither the reader nor extension version is changed by this follow-up.
Package validation is distinct from physical GPU validation on every device;
Windows ARM64 is cross-built, while installed VS Code behavior is exercised on
macOS and Linux.

## Upstream Release and Marketplace

Source and official artifacts live in `szdytom/markview`. The existing personal
fork remains the PR source; `Stevvven777/homebrew-tap` remains a separate Cask
repository pointing at upstream reader assets. No fork release is required.

Pushing a normal reader version tag runs the six-target extension builder from
`Packaging`. Each tested package is uploaded as `artifacts-extra-vscode-<platform>`;
the aggregation job verifies the complete set and creates `SHA256SUMS-vscode`.
cargo-dist attaches these files to the same reader Release. Pull requests exercise
the same packaging path without publishing; the builder also supports manual runs.

After announcement, the generated Release workflow calls `extension-release.yml`.
Only `szdytom/markview` can publish, and only an already published stable reader
release is accepted. The job checks out that release tag, downloads its VSIX files
and checksums, verifies them, installs the locked `vsce`, and publishes with `--oidc`.
It never rebuilds the packages. Its `marketplace` environment and OIDC permission
apply only to the publishing job. PRs, forks and reader prereleases do not publish.

One-time setup by the upstream maintainer and publisher owner:

1. Merge the workflow changes upstream and create the GitHub environment
   `marketplace`. Restrict deployment to release tags as appropriate. Required
   reviewers are optional; leave them unset for unattended updates.
2. In publisher **Stevvven → Trusted Publishing**, authorize owner `szdytom`,
   repository `markview`, environment `marketplace`. Authorize workflow filenames
   `release.yml` for automatic releases and `extension-release.yml` for manual
   retries (the caller and reusable workflow are both in this repository).
3. No `VSCE_PAT` is needed. Follow the official
   [VS Code Trusted Publishing setup](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#publish-with-github-actions-and-trusted-publishing).

The Marketplace identity stays `Stevvven.markview-export`. The extension version
in `web/app/vsc-ext/vscode/package.json` remains independent of the reader version;
bump it whenever a preview, exporter, template or extension change should reach
store users, including bundled-engine updates. Update the store changelog as well.
An unchanged extension version is still attached to the reader Release, but
`--skip-duplicate` skips each platform already present in Marketplace; it does not
replace that platform's existing package. Release notes use the extension manifest's
version rather than the reader version.

If authorization or upload fails, the reader Release and VSIX attachments remain
available. Re-run the failed publishing job, or manually run **Publish Markview
Integration** with the existing stable reader tag. If the environment permits
only tags, select that tag under **Use workflow from** as well. It downloads the same release
attachments; already published platforms are skipped, allowing a partial upload
to resume. Do not rebuild, replace release attachments, or increment the version
just to retry. Real OIDC token exchange and store upload remain untested until the
publisher policy is configured and an actual publication is authorized.

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

## Upstream publishing follow-up validation

Both the first scoped review and the final review after rebasing returned
`VERDICT: approve` (`integration-upstream-review-1.log` and
`integration-upstream-review-final.log`, Astra medium). The final reviewer
independently reran all six release tests and approved REL-6..8 and the changed
REL-4 automation. Neither round reported actionable defects. Hosted execution
of the updated workflows and actual OIDC publication were explicitly outside
that approval's established evidence.

Completed logs in the parent artifacts directory:

- `integration-upstream-validation-rebased.log`: owned workflow lint, generated workflow schema, cargo-dist generation/plan, six release tests, four documentation tests and changelog checks.
- `integration-upstream-native-build.log`, `integration-upstream-wasm-build.log`: fresh builds against main `d3a2512`.
- `integration-upstream-extension-rebased.log`: types/formatting, 17 Node tests and 11 browser tests.
- `integration-upstream-native-tests.log`: two real native font/export tests.
- `integration-upstream-real-handoff.log`: actual six earlier CI packages pass the new staging and pre-publication verification commands without publication.

The PR update triggers fresh hosted checks. The earlier six-platform build links
above remain historical evidence; they are not new builds of the rebased engine.
