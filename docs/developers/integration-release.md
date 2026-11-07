# Markview Integration delivery

Markview Integration reuses main's WASM preview, font manager and native exporter.
The Marketplace identity remains `Stevvven.markview-export`; existing Better
markdown PDF installations upgrade in place. Implementation and publishing
acceptance are tracked here; detailed review history belongs in
[PR #89](https://github.com/szdytom/markview/pull/89).

## Acceptance

Astra medium approved REL-1..8 for `c4d0020` and the subsequent shared-export
and configuration cleanup. Astra also approved the five-target release scope.
Actual GitHub Release/OIDC publication remains deferred.

| ID | Requirement | Established evidence |
| --- | --- | --- |
| REL-1 | Markview Integration branding with the existing extension ID | Manifest 0.4.0; installed upgrade from Marketplace 0.1.2 passes. |
| REL-2 | Legacy commands/settings and document-scoped template resolution | Node and real VS Code tests cover language overrides, explicit new settings, empty values and relative paths. Export-only legacy styles do not affect preview. |
| REL-3 | Store README, icon, license and changelog ship in the VSIX | Packaged store assets verified. |
| REL-4 | Five matched native packages and gated publication: `darwin-arm64`, `win32-x64`, `win32-arm64`, `linux-x64`, `linux-arm64` | **Approve, implementation.** Five-target staging and real-package verification pass; Intel macOS packages are rejected. Hosted builds of the revised matrix remain pending. |
| REL-5 | Installed preview handles unsaved edits, PDF/PNG and cleanup | Real VS Code upgrade, dirty-buffer preview/export and clean exit pass. |
| REL-6 | Reader releases attach verified VSIX files and checksums | Five-target complete-set staging and checksum handoff pass; actual release attachment remains unverified. |
| REL-7 | Publish exact release attachments using OIDC, with independent retries | Stable-release validation, checksums and retry commands pass tests; actual token exchange/upload remains unverified. |
| REL-8 | Correct version/platform downloads and publishing policy | Release-notes tests and documentation checks pass. |

## State of play

The latest existing revision `383f1fa` passed [CI](https://github.com/szdytom/markview/actions/runs/42160393904),
[Editor](https://github.com/szdytom/markview/actions/runs/42160393674),
[Packages](https://github.com/szdytom/markview/actions/runs/42160393712) and
[Release packaging](https://github.com/szdytom/markview/actions/runs/42160393673).
Those builds used the prior six-target matrix; hosted verification of the new
five-target matrix remains pending. No release tag or Marketplace upload was made.
Windows ARM64 is cross-built; these checks do not establish physical GPU behavior
on every platform. Publication requires the authorization below.

## Upstream Release and Marketplace

Official source and release assets live in `szdytom/markview`. The personal fork
is the PR source. `Stevvven777/homebrew-tap` remains a separate Cask repository
pointing to upstream reader assets.

Normal reader version tags run the five-target builder inside `Packaging`.
Each tested VSIX is uploaded as `artifacts-extra-vscode-<platform>`; aggregation
verifies the complete set and creates `SHA256SUMS-vscode`. cargo-dist attaches
both to the same reader Release. PRs exercise packaging without publication;
`extension-packages.yml` also supports manual builds.

After announcement, `extension-release.yml` accepts only published stable releases
in `szdytom/markview`. It checks out the release tag, downloads its attachments,
verifies checksums/metadata and publishes them using OIDC without rebuilding.
The publishing job alone uses the `marketplace` environment and OIDC permission.
PRs, forks and reader prereleases do not publish.

One-time setup by the upstream maintainer and publisher owner:

1. Merge the workflows and create GitHub environment `marketplace`. Restrict
   deployments to release tags as appropriate; required reviewers are optional.
2. Under **Stevvven → Trusted Publishing**, authorize owner `szdytom`, repository
   `markview`, environment `marketplace`, and workflow filenames `release.yml`
   for automatic releases and `extension-release.yml` for manual retries.
3. No `VSCE_PAT` is needed. Follow the official
   [Trusted Publishing setup](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#publish-with-github-actions-and-trusted-publishing).

The extension version in `web/app/vsc-ext/vscode/package.json` is independent of
the reader version. Bump it and the store changelog for any change intended for
store users, including bundled-engine updates. An unchanged version still appears
in reader Release attachments, but `--skip-duplicate` preserves packages already
in Marketplace. Release notes use the extension manifest's version.

A store failure leaves GitHub assets available. Re-run the failed publishing job,
or manually run **Publish Markview Integration** with the existing stable tag.
If the environment permits only tags, select that tag under **Use workflow from**.
Retries download the same attachments and skip already uploaded platforms.
Do not rebuild, replace attachments or bump the version just to retry.
