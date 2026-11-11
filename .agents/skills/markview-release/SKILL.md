---
name: markview-release
description: Cut a Markview release — bump the workspace version, cut the changelog section, tag, push, and confirm the cargo-dist Release workflow. Use when asked to release or publish a Markview version.
---

# Markview release

A release request authorizes the version commit, the tag, and the push that triggers publication.

## Before you start

1. **Start with a clean, synchronized `main`.** The user's workspace must be on `main`
   with no uncommitted changes. Fetch the remote `main` and confirm its HEAD equals the
   local HEAD.
2. **All CI checks for that HEAD must pass.** Use `gh` to inspect the latest attempts of
   all CI checks and their jobs for that exact commit. Do not limit the
   check to the workflow named `CI` or accept a passing run for an older commit.
   If local and remote HEAD are synchronized and checks are queued or running with no
   failed check or job, wait by default until they finish, without a timeout. Inspect
   intermediate job results too: stop and report immediately if any check or job fails,
   even while others are still running. Pending checks alone do not mean a failed
   precondition; do not ask the user whether to wait. Keep the user informed and use
   short polling intervals so the release-opening discussion below can continue.
   After waiting, fetch again and recheck the clean workspace, synchronized HEAD and
   checks before proceeding, so a stale green commit cannot authorize the release.
3. **Check the changelog.** The "Unreleased" section of `CHANGELOG.md` must be present and in good shape. For minor errors, fix them in the release commit; for larger issues, stop and report.
4. **Check the version.** The user must explicitly name the version to release. The new specified version must be reasonable (greater than the current version, not skipping a version, etc.). E.g., if the current version is `0.1.1`, the user may release `0.1.2` or `0.2.0`, but not `0.1.3` or `0.1.0`.

If a precondition fails, stop and report the problem to the user. While CI is pending
under the conditions above, keep waiting and discuss the release opening with the user.
Do not begin the release steps until all preconditions pass and the user has confirmed
the opening paragraph.

## Discuss the release opening

Draft an opening paragraph from the release's changelog entries, then discuss its
emphasis and wording with the user. Obtain explicit confirmation of the final text
before proceeding with the release. A release request alone does not approve this text.
Start this discussion while waiting for CI when possible; CI passing does not replace
the user's confirmation, and confirmation does not replace passing CI.

## Steps

1. **Version.** Set `version` in `[workspace.package]` in `Cargo.toml`, then
   `cargo update --workspace --offline` to update the workspace members in `Cargo.lock`.
   Refresh the separate fuzz workspace too with
   `cargo update --workspace --offline --manifest-path fuzz/Cargo.toml`, so its
   lockfile does not drift behind the new version.
   Discover all publishable packages in the `web/` pnpm workspace and set their
   `package.json` versions to the same release version; skip private packages.
   Then run `pnpm --dir web install --lockfile-only --offline` to refresh the pnpm lockfile if needed.
   Also set the `--tag v<version>` in the `cargo install` instruction of both `README.md`
   and `README.zh-cn.md` to the release version, so the documented command is pinned.
2. **Changelog.** Directly under `## Unreleased` in `CHANGELOG.md`, insert
   `## <version> - <YYYY-MM-DD>` (today's date from `date`) followed by the opening paragraph
   confirmed by the user, then the release's category headings and entries. Leave
   `## Unreleased` in place and empty. cargo-dist takes
   the H2 whose version matches the tag as the GitHub Release title and body, so keep it
   bracket-free and unique. Reflow the release's own entries, being careful not to move the
   entries the `## Unreleased` section keeps: on the way in, `## Unreleased` stays directly
   above the new H2.
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

## WinGet submission

For stable releases, `Release` automatically runs the WinGet job and uploads
`winget-<version>`. Download that artifact from the Release run with `gh run download`;
do not trigger `winget.yml` again. Only for backfilling an existing release without
an artifact, run `gh workflow run winget.yml --repo szdytom/markview -f tag=v<version>`.
Prereleases skip WinGet.

Use the local `gh` login to submit the generated manifests; no `WINGET_TOKEN` is needed
in Actions. Check for an existing upstream version or update PR before submitting.
Verify the version, MSI URL and SHA-256 against the release asset metadata. On a new
branch in the publisher's `winget-pkgs` fork, add only the three generated YAML files
under `manifests/s/szdytom/Markview/<version>/`, preserving older versions. Review the
diff, then use `gh pr create --repo microsoft/winget-pkgs --base master` with the fork
branch and title `New version: szdytom.Markview version <version>`. Return the PR URL
and check status; submission is complete while upstream review and source sync remain
pending.

## Release notes style

New releases open with the prose paragraph confirmed by the user, directly below the
version heading and before the first `###` category. Do not add a Highlights list.
Keep all published release sections unchanged, including their existing openers.

Rules:

- Write for users, not developers. Name what a reader can now do or no longer suffers, not the
  module, oracle, target or script that changed. Fuzzing, campaign tooling, packaging and CI work
  belongs in the body: mention it only when it changes what a user can install or run.
- Drop filler such as "experience" and "improvements"; state the change or its payoff.
- Cite issue numbers in the category entries, not in the opening paragraph.
- Do not advertise distribution or features that are not live yet. A pending upstream submission
  (e.g. a first-time WinGet registration) does not belong in the opening paragraph.
- Avoid the bare word "security" unless the release fixes a genuine vulnerability; use
  "robustness and security hardening" when it covers both.
- Leave a blank line before and after the opening paragraph.

## Invariants

- The tag must equal the manifest version without the `v`; dist rejects a mismatch at plan.
- Every publishable package in the `web/` pnpm workspace must have the same version
  as `[workspace.package]` in `Cargo.toml`; discover packages from the workspace configuration
  rather than maintaining a fixed list of names or paths.
- `release.yml`, `wix/main.wxs`, and `[package.metadata.wix]` are dist-owned: never hand-edit
  them, and run `dist generate` after editing `dist-workspace.toml`.
- `dist` is not on `PATH`; use `~/.cargo/bin/dist`.
- The `cargo install` instructions in both READMEs are pinned to the latest released tag;
  they go stale if a release skips them.
- Release from a clean `main`. Pre-1.0, feature releases have shipped as patch bumps, so use
  the version the user names.
- Artifact matrix and platform requirements: `docs/developers/packaging.md`.

## Additional hints

- If CI waiting seems abnormal, especially a prolonged queue, check
  [GitHub Status](https://www.githubstatus.com/) for an active incident affecting
  GitHub Actions. If one is reported, tell the user, link the incident and stop the
  release process. Otherwise, continue the normal waiting policy.
