# Release the editor integration

The extension keeps the Marketplace identity `Stevvven.markview-export` so existing
installations can upgrade. Its version is independent of the reader version.
This guide describes the repository's delivery procedure; check actual workflow
runs and store state before declaring a publication successful.

## Acceptance

Before release, verify packaged metadata, legacy commands/settings, installed
upgrade behavior, unsaved-buffer preview/export and cleanup in real VS Code.
Verify the complete platform set and its hashes, not only a source build.
The [extension workspace](../../web/app/vsc-ext/README.md) owns build/test commands.
Windows ARM64 cross-compilation does not establish physical GPU behavior.

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
