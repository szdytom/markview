---
name: markview-release
description: Cut a Markview release — bump the workspace version, cut the changelog section, tag, push, and confirm the cargo-dist Release workflow. Use when asked to release or publish a Markview version.
---

# Markview release

A release request authorizes the version commit, the tag, and the push that triggers publication.

## Before you start

1. **Start with a clean `main`.** The user's workspace must be up to date with `main` and have no uncommitted changes.
2. **The latest CI must be green.** The current `main` must have a passing CI run. Use `gh` to check the latest run for `main` and confirm it is green.
3. **Check the changelog.** The "Unreleased" section of `CHANGELOG.md` must be present and in good shape. For minor errors, fix them in the release commit; for larger issues, stop and report.
4. **Check the version.** The user must explicitly name the version to release. The new specified version must be reasonable (greater than the current version, not skipping a version, etc.). E.g., if the current version is `0.1.1`, the user may release `0.1.2` or `0.2.0`, but not `0.1.3` or `0.1.0`.

If any of these preconditions are not met, stop and report the problem to the user. Do not proceed with the release.

## Steps

1. **Version.** Set `version` in `[workspace.package]` in `Cargo.toml`, then
   `cargo update --workspace --offline` to move the five workspace members in `Cargo.lock`.
2. **Changelog.** Directly under `## Unreleased` in `CHANGELOG.md`, insert
   `## <version> - <YYYY-MM-DD>` (today's date from `date`) and a one- or two-line summary of
   the release. Leave `## Unreleased` in place and empty. cargo-dist takes the H2 whose version
   matches the tag as the GitHub Release title and body, so keep it bracket-free and unique.
3. **Verify.** `cargo fmt --all --check`, then
   `cargo clippy --workspace --locked --all-targets -- -D warnings`, then
   `cargo test --workspace --locked --all-targets`. Confirm the announcement with
   `~/.cargo/bin/dist plan` and that generated files are current with
   `~/.cargo/bin/dist generate --check`.
4. **Commit.** `chore: release <version>`.
5. **Tag and push.** Existing tags are annotated with the message `markview <version>`:
   `git tag -a v<version> -m "markview <version>"`, then push `main` and the tag.
6. **Confirm.** The tag push runs `Release` (plan → per-platform builds → global installers →
   `Packaging` and cross-platform checks → GitHub Release with every asset), plus `CI` and
   `Packages`. Watch with `gh run watch <id> --exit-status` and check the release page exists.
   The workflow verifies the assets, so do not download them to re-check checksums.

## Invariants

- The tag must equal the manifest version without the `v`; dist rejects a mismatch at plan.
- `release.yml`, `wix/main.wxs`, and `[package.metadata.wix]` are dist-owned: never hand-edit
  them, and run `dist generate` after editing `dist-workspace.toml`.
- `dist` is not on `PATH`; use `~/.cargo/bin/dist`.
- Release from a clean `main`. Pre-1.0, feature releases have shipped as patch bumps, so use
  the version the user names.
- Artifact matrix and platform requirements: `docs/packaging.md`.
