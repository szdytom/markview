# Changelog conventions

Maintain [CHANGELOG.md](../../CHANGELOG.md) for notable release changes under
`Unreleased`. CI checks formatting and published history with
[check_changelog.py](../../scripts/check_changelog.py); entry selection and
consolidation require review.

## Scope

Record notable changes that help users understand the next release: features,
behavior changes, meaningful fixes, performance improvements, compatibility
changes, and security fixes.

Routine documentation edits, tests, fuzzing, CI, tooling, refactoring, and
dependency maintenance do not need entries on their own. Include them only when
they have a notable effect on users or supported public interfaces, installation,
or platform requirements. Describe that effect. A `Documentation` category does
not make every documentation edit worth recording.

A task, commit, or pull request does not automatically need an entry. Skipping a
changelog update is expected for routine internal work.

## Maintaining Unreleased

- Use the latest published release as the baseline.
- Before adding an entry, read `Unreleased` for related entries.
- Update an existing entry when the same feature or behavior changes again. Keep
  distinct user-visible effects separate when that improves clarity.
- Describe the final behavior. Omit intermediate implementations and fixes for
  defects introduced and resolved entirely within `Unreleased`.
- Remove entries for changes fully reverted before release, unless a notable net
  effect remains.
- Keep entries concise and focused on what changes for users.
- Edit only entries related to the current task unless broader cleanup is
  requested. Preserve published release entries.

For example, if a released label changes from `foo` to `A` and then to `B` before
the next release, keep one entry describing `foo` to `B`. If it returns to `foo`,
remove the entry. Adding a regression test alone needs no entry; fixing a bug in
the published reader warrants an entry describing the user-visible fix.

## Published history

The checker compares against the nearest reachable release tag (`vVERSION`).
Sections published at that tag and all earlier release sections are protected;
new entries and ongoing revisions belong in `Unreleased`. Cutting a new release
above the protected sections is allowed.

For typo corrections and similar maintenance, every commit that changes published
content must modify only `CHANGELOG.md`. Other commits in the same branch may
change code and `Unreleased`. A later correction or revert does not exempt an
earlier mixed commit. Uncommitted edits to published content fail until committed
separately.

History checks require a full clone with release tags. CI uses `fetch-depth: 0`.
Repositories without a release tag and standalone files outside Git receive only
formatting checks.

## Entry format

`Unreleased` must appear exactly once and be the first release section. Group
entries under unique `###` category headings: `Added`, `Changed`, `Deprecated`,
`Removed`, `Fixed`, `Security`, or `Documentation`.

Keep each entry on one `- ` line. Do not wrap entries, put blank lines between
entries in the same category, or use Markdown line breaks or trailing whitespace.
Historical release bodies keep their existing format.

## Release headings

Use `## VERSION - YYYY-MM-DD`, with an unbracketed SemVer version and a valid
calendar date. Versions must be unique; prerelease versions and build metadata
are supported. These heading checks apply to every release.

## Run the check

From the repository root:

```sh
python3 scripts/check_changelog.py
```

Pass a file path to check another changelog. Failures print the file, line number
and violated rule, and exit with status 1.
