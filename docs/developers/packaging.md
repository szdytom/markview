# Packaging and releases

This page is for maintainers cutting a Markview release. It describes what each
release asset contains, how the assets are produced, and what a user needs on
each platform. The build procedure a contributor runs day to day is in the
[development guide](development.md).

## Artifact matrix

| Platform | Asset | Built by |
| --- | --- | --- |
| Linux x86_64 | `markview-x86_64-unknown-linux-gnu.tar.gz` | cargo-dist |
| Linux x86_64 | `markview_<version>_amd64.deb` | `scripts/package_deb.sh` |
| Linux x86_64 | `markview-<version>-x86_64.AppImage` | `scripts/package_appimage.sh` |
| Windows x86_64 | `markview-x86_64-pc-windows-msvc.zip` | cargo-dist |
| Windows x86_64 | `markview-x86_64-pc-windows-msvc.msi` | cargo-dist |
| Windows aarch64 | `markview-aarch64-pc-windows-msvc.zip` | cargo-dist |
| Windows aarch64 | `markview-aarch64-pc-windows-msvc.msi` | cargo-dist |
| macOS aarch64 | `markview-aarch64-apple-darwin.tar.gz` | cargo-dist |
| macOS aarch64 | `markview-<version>-aarch64.app.zip` | `scripts/package_macos_app.sh` |
| VS Code: Apple Silicon macOS; x64/ARM64 Windows and Linux | `markview-export-<extension-version>-<platform>.vsix`, `SHA256SUMS-vscode` | `extension-packages.yml` and `packaging.yml` |
| All | `markview-installer.sh`, `markview-installer.ps1` | cargo-dist |

GitHub supplies source archives and asset digests. VSIX packages additionally ship
`SHA256SUMS-vscode` for the Marketplace handoff; reader packages have no separate
checksum files or cargo-dist source tarball. The release-notes workflow removes
`dist-manifest.json` after publication; internal workflow manifests remain available
for the build jobs.

Every portable archive carries `LICENSE`, `README.md`, `THIRD_PARTY.md`, and
`licenses/KaTeX-OFL.txt`. The Linux packages additionally carry `third-party-notices.html`, which lists the complete dependency tree and
reproduces each dependency license; see
[third-party components](../../THIRD_PARTY.md).

The MSI installs only the executable. Windows users who need the full notices should
take the `.zip` or the notices file from the release page.

## Configuration ownership

`dist-workspace.toml` is the source of truth for archives and installers.
`dist init` generates the rest, and those files are managed by dist:

- `.github/workflows/release.yml`
- the `[package.metadata.wix]` table in `Cargo.toml`, which holds the stable
  upgrade and install-path GUIDs Windows needs to upgrade in place

Run `dist generate` after editing `dist-workspace.toml`, and never
hand-edit the generated files. `dist generate --check` in CI fails when
they drift.

`wix/main.wxs` is maintained by hand under `allow-dirty = ["msi"]` to preserve
Markdown associations and default-app registration. Keep its product/path GUIDs
and `$(var.*)` references consistent with cargo-dist. Both Windows architectures
use Windows Installer 5.0, required for ARM64 MSIs, and share an upgrade code so
switching architectures replaces the installed reader.

`.github/workflows/packaging.yml` is *not* generated. It runs as a reusable workflow inside
`Release`, after local archives are built and before the release is published.
Its failure blocks publication; all assets are uploaded together.

After publication, `.github/workflows/release-notes.yml` updates the download
table to link the macOS `.app.zip`, Windows MSI and portable ZIP, and Linux
AppImage, Android APK and five platform-specific VSIX files. It checks that each package is attached, and
writes each package’s GitHub SHA-256 digest directly in the checksum column, and
preserves the changelog and installation commands. It also adds macOS extraction,
Applications installation and quarantine instructions. This also runs for prereleases.

The VS Code builds run inside `Packaging`, including complete-set verification
and checksums, so missing or mismatched packages block the GitHub release.
After a stable upstream release, `extension-release.yml` downloads those exact
attachments, checks their hashes and metadata, and publishes them using OIDC.
Marketplace authorization or upload failures leave the GitHub assets available.
See [extension publishing setup and retries](integration-release.md#upstream-release-and-marketplace).

## Packaging inputs build every package

The `Packages` workflow builds the complete package set whenever a file it
depends on changes, on any branch and on pull requests. Nothing is published:
each package comes back as a workflow artifact you can download from the run.
The filter covers `Cargo.toml`, `Cargo.lock`, `dist-workspace.toml`, `.github/`,
`scripts/package_*.sh`, `scripts/verify_*`, `packaging/`, `wix/`, `assets/`, and
`xtask/`. A `src/`-only change is verified by `CI` instead, because it cannot
change how a package is assembled. Run `Packages` manually from the Actions tab
to rebuild the set without touching a packaging input.

| Artifact | Contents | Runner |
| --- | --- | --- |
| `markview-linux-packages` | Debian package, AppImage, portable archive, checksums | ubuntu-22.04 |
| `markview-windows-packages-x86_64-pc-windows-msvc` | x64 MSI, portable zip | windows-2022 |
| `markview-windows-packages-aarch64-pc-windows-msvc` | ARM64 MSI, portable zip | windows-11-arm |
| `markview-macos-packages` | `.app.zip`, portable archive, checksums | macos-latest |

Each platform builds its own archive; release packaging reuses the archives
from the same workflow run. The Linux job uses `ubuntu-22.04` for the same
reason releases do: the archive must stay usable on glibc 2.35.

The workflow ends in a single `ci` job that fails unless `check` and the
packaging lint succeeded. Point branch protection at `ci` rather than at the
matrix jobs. `Packages` is deliberately *not* required: a workflow that path
filtering skipped reports no status at all, so requiring it would block every
pull request that touches no packaging input.

## Validate packaging before a release

Pull requests run the complete `Release` build with `pr-run-mode = "upload"`.
This exercises the same platform tests, formatting, and clippy checks as
CI, plus local archives/MSI, global installers, and the reusable
`Packaging` workflow without creating a tag or publishing a release.
Download the `artifacts-*` workflow artifacts to inspect the outputs.

Windows ARM64 builds, Clippy, regular tests, property tests and package validation
run natively on `windows-11-arm`; cargo-dist specifies its ARM64 host explicitly.
Both Windows runners install pinned WiX 3.14.1 through the shared `setup-wix`
action; Release includes it through cargo-dist's `github-build-setup` hook.
The Windows checks extract both MSI and ZIP packages, run their executables, and
confirm their PE machine type matches the target and they use the Windows GUI
subsystem, so opening the reader creates no console window. They also check the
MSI's platform, install it for real, check that the three
Markdown extensions now resolve to Markview, and remove the product again.
The Linux checks extract both the Debian package and AppImage and run their
executables. The macOS check unpacks the application archive, validates its
plist, its declared Markdown document types and its signature, and runs the
bundled executable. The `Packages` workflow
also builds these packages whenever a packaging input changes.

## Why Linux archives build on Ubuntu 22.04

A binary inherits the glibc symbol versions of the machine that linked it. The
rolling-release host used for development produces binaries that require
glibc 2.43 or newer, which most distributions cannot satisfy. Linux release
archives are therefore built on `ubuntu-22.04`, and the package checks reject binaries that reference newer glibc symbols.
`dist.min-glibc-version.x86_64-unknown-linux-gnu = "2.35"` declares that
requirement to the generated installers.

## Cut a release

1. Update `version` in `[workspace.package]` and commit it.
2. Tag and push:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. The `Release` workflow builds archives, installers, and the supplementary
   packages, then verifies them.
4. Only after those jobs succeed does it create the GitHub release and upload
   all assets together.
5. Verify the release page against the artifact matrix above, including five
   VSIX files and `SHA256SUMS-vscode`; no cargo-dist source tarball or dist manifest is attached.

The tag must equal the manifest version without the `v` prefix; dist rejects a
mismatch during the plan step, before anything is built.

To build the archives locally without publishing:

```sh
dist build --artifacts=local --target x86_64-unknown-linux-gnu
```

`dist` can only build targets the host can link for. Windows archives and MSIs
require a Windows host, macOS archives require macOS, and the `.deb` and
AppImage need `dpkg-deb` and `appimagetool`, which is why CI owns them.

## WinGet distribution

The package identifier is `szdytom.Markview`. Once the initial submission is
merged into [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs),
Windows users can install and upgrade the machine-wide MSI with:

```powershell
winget install --id szdytom.Markview --exact --source winget
winget upgrade --id szdytom.Markview --exact --source winget
```

`packaging/winget/0.1.10/` holds the initial submission, using the SHA-256,
product code and upgrade code from the published MSI. It is a release snapshot,
not a template for newly built MSIs: WiX generates a new product code each build.
CI validates it with `winget validate`, installs the published MSI through
`winget install --manifest`, runs the installed executable, then uninstalls it.
To run the same check locally, use an elevated PowerShell on a clean Windows
machine with WinGet installed:

```powershell
./scripts/verify_winget_package.ps1
```

After a stable release is published, `.github/workflows/winget.yml` uses the
pinned Microsoft WinGetCreate tool to update the community manifest from the
public MSI URL. WinGetCreate downloads and inspects the installer to refresh
its hash and product metadata. The result is saved as a `winget-<version>`
workflow artifact. Until the initial community submission is merged, the
workflow reports a notice and successfully skips the update. GitHub releases
can continue at their usual pace. Prereleases are skipped.

Keep the initial submission on its tested release while it is reviewed. After
it merges, manually run the `WinGet` workflow for the latest stable tag; there
is no need to submit every intermediate version. Subsequent stable releases
then use the normal update workflow.

For automatic update PRs, configure a repository secret named `WINGET_TOKEN`
with a dedicated GitHub classic personal access token with `public_repo`
scope, as described in [WinGetCreate's token guide](https://github.com/microsoft/winget-create/blob/main/doc/token.md).
Without the secret, registered packages still produce the manifest artifact for
manual submission. The release's built-in `GITHUB_TOKEN` only reads metadata; it cannot
submit PRs to Microsoft's repository. A maintainer can also rerun the `WinGet`
workflow manually with a published stable tag, for example `v0.1.11`.

Community validation and review happen after submission, so a GitHub release
may be available before `winget upgrade` offers it. Do not replace a published
MSI after submission: WinGet verifies the manifest's SHA-256 when downloading.

## Runtime requirements

The [installation guide](../users/installation.md#runtime-requirements) owns the
per-platform runtime requirements and user-facing troubleshooting. Linux relies
on host glibc, Vulkan drivers and fonts; the Debian package declares required
libraries as `Depends`, while AppImage does not resolve host dependencies.

## macOS signing

The `.app` bundle is ad-hoc signed only. It is not Developer ID signed and not
notarized, so Gatekeeper blocks the first launch after a browser download. The
user clears the quarantine flag once:

```sh
xattr -d com.apple.quarantine /Applications/Markview.app
```

Distributing a signed and notarized build requires an Apple Developer ID
certificate and a `codesign`/`notarytool` step in `packaging.yml`; until that
exists, document the quarantine step in the release notes rather than
pretending the download is silent.

## Verify a build locally

The packaging scripts take an already compiled binary, so they can be exercised
before a tag exists:

```sh
cargo build --release --locked
scripts/package_deb.sh target/release/markview 0.1.0 amd64 /tmp/out
scripts/package_appimage.sh target/release/markview 0.1.0 /tmp/out
```

Inspect the result rather than trusting the exit status:

```sh
dpkg-deb -I /tmp/out/markview_0.1.0_amd64.deb
dpkg-deb -c /tmp/out/markview_0.1.0_amd64.deb
/tmp/out/markview-0.1.0-x86_64.AppImage
```

The icon assets are generated from `assets/markview-icon-color.svg`. Regenerate
them after changing the source SVG and commit the result:

```sh
cargo run -p xtask -- icons
cargo run -p xtask -- icons --check
```
