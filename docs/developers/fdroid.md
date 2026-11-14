# Prepare F-Droid distribution

Markview targets the official F-Droid repository with F-Droid's own signing key.
The app is not yet listed. GitHub APKs keep the upstream signing key; switching
between these two channels requires reinstalling the app.
Publication progress is tracked in [issue #96](https://github.com/szdytom/markview/issues/96).

## Store metadata

[`fastlane/metadata/android`](../../fastlane/metadata/android) contains English
and Simplified Chinese descriptions, icons and screenshots. Fastlane itself is
not required. Updates must be included in a release tag before F-Droid can use
them. Keep summaries below 80 characters, titles within 50, full descriptions
within 4,000, and each version-code changelog within 500 characters.

The `195.txt` changelogs describe the published `v0.5.2` release. Every new tagged
release needs an incremented `[workspace.metadata.android].version-code` and a
matching `<code>.txt` update in both languages.

The store assets are unmodified copies of existing repository images, covered
by the repository's MIT license:

| Store asset | Source |
| --- | --- |
| Both icons | `assets/icons/markview-512.png` |
| English phone 1 | `docs/screenshots/readme/android-en.png` |
| Chinese phone 1 | `docs/screenshots/readme/android-zh.png` |
| English phone 2 | `docs/screenshots/android/settings.png` |
| English phone 3 | `docs/screenshots/android/fonts.png` |
| English tablet 1 | `docs/screenshots/android/tablet-tabs.png` |

The phone reading captures show Markview's own sample text. The other captures
show the Android integration fixture and application settings. No third-party
showcase articles are used. Screenshot capture details live in the
[image documentation](../screenshots/readme/README.md).

Run `python3 scripts/test_fdroid.py` to check text, image and update-detection
constraints.

## Build recipe

[`packaging/fdroid/io.github.szdytom.markview.yml`](../../packaging/fdroid/io.github.szdytom.markview.yml)
is the candidate recipe for `fdroiddata/metadata/`. Its first build is disabled:
the existing `v0.5.2` tag predates the unsigned build option and store metadata.
After publishing the first tag containing this preparation, update the build's
version name, code and commit, update `CurrentVersion` and `CurrentVersionCode`,
then remove `disable`. Do not move or replace the old tag.

The recipe builds ARM64 from source and outputs an unsigned APK. Rust,
`cargo-about`, SDK and NDK pins must match the Android configuration. Toolchain
upgrades can require an `fdroiddata` recipe update. Stable `vMAJOR.MINOR.PATCH`
tags are checked automatically; prerelease tags are excluded. `UpdateCheckData`
reads the version code directly from `Cargo.toml` and the version name from the
tag. The tag must match the workspace version, as required by cargo-dist.

The source scanner removes the compressed JSON memory-test baselines under
`fuzz/fuzzlib/tests/baselines/doc-memory/`. They are test data, unused by the
Android build. Other source files and the resulting APK remain subject to scans.

For validation, use the official build image and a separate `fdroiddata` checkout:

```sh
podman pull registry.gitlab.com/fdroid/fdroidserver:buildserver
```

Copy the candidate to `metadata/io.github.szdytom.markview.yml` in that checkout.
Validate an enabled copy against the actual preparation commit; the disabled
`v0.5.2` seed is not a buildable release. Run `fdroid readmeta`, `fdroid
rewritemeta`, `fdroid lint`, `fdroid scanner` and `fdroid build` inside the image.
Lint with the real upstream HTTPS URL. For a local source snapshot, override
only the validation copy's repository and commit after linting; the linter
rejects local `file://` URLs. Preserve the real upstream URL in the candidate
submitted to F-Droid.

On 2026-11-14, an enabled local preparation snapshot passed all five commands
in the official image (digest below), using fdroidserver commit
`fc2c86473e66a44fc641f403c338a8b2cccfad88`:

```text
sha256:1ea8bef1820bff06c2974e31b4fc4c79c364cd24231b86e71fa4b926797e0dac
```

`fdroid build --on-server --scan-binary` produced an unsigned ARM64 APK with
version `0.5.2` / code `195`, API 30 minimum, API 35 target, debugging disabled,
16 KiB ZIP and native load alignment, and bundled licenses. This verifies the
preparation snapshot; the first release tag still needs validation before submission.

## Submission

Publish a release containing the preparation before opening the `fdroiddata`
merge request. Include Android user feedback such as
[issue #51](https://github.com/szdytom/markview/issues/51) and refresh GitHub APK
download counts to support the existing-user exception to the three-month
development guideline. A download count is not a unique-user count.

After acceptance, watch the first build and publication on F-Droid Monitor.
Then add the official installation link to the user guide and READMEs. Keep
future stable tags and version-code changelogs consistent with the recipe.

References: [submission guide](https://f-droid.org/docs/Submitting_to_F-Droid_Quick_Start_Guide/),
[build metadata](https://f-droid.org/docs/Build_Metadata_Reference/),
[store metadata](https://f-droid.org/docs/All_About_Descriptions_Graphics_and_Screenshots/).
