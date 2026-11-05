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
   `## <version> - <YYYY-MM-DD>` (today's date from `date`) followed by a `**Highlights**`
   list in the house style below. Leave `## Unreleased` in place and empty. cargo-dist takes
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

## Release notes style

Every release section from `0.1.11` on opens with a `**Highlights**` bullet list instead of a
summary paragraph. Entries before `0.1.11` keep their existing prose openers; do not rewrite them.

```markdown
## 0.1.11 - 2026-10-03

**Highlights**:

- Start a search with `/` and jump straight to the next match.
- Refined touchpad and wheel scrolling.
- New setting for singleton mode.

### Added
```

Rules:

- Write for users, not developers. Name what a reader can now do or no longer suffers, not the
  module, oracle, target or script that changed. Fuzzing, campaign tooling, packaging and CI work
  belongs in the body: mention it only when it changes what a user can install or run.
- Keep 4 to 6 bullets, one line each. More than that means the body should carry the rest.
- Use one consistent voice across the list. Telegraphic noun phrases (`Refined ...`) and
  imperatives (`Start ...`) both work, but do not mix them with third-person `Fixes ...` lines.
- Drop filler such as "experience" and "improvements"; state the change or its payoff.
- Cite issue numbers in the body, not in the highlights.
- Do not advertise distribution or features that are not live yet. A pending upstream submission
  (e.g. a first-time WinGet registration) is not a highlight.
- Avoid the bare word "security" unless the release fixes a genuine vulnerability; use
  "robustness and security hardening" when it covers both.
- Keep the colon on `**Highlights**:`, one blank line before the first bullet, and one blank line
  after the list before the next `###` heading, so GitHub and the release body render the list.

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
