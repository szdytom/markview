# Maintain documentation

Write a page only when it saves a reader work: completing a task, using a public
contract, locating a change, or understanding a design decision. A prose copy of
control flow is usually less useful than the code it duplicates.

## Choose one owner

| Information | Owner |
| --- | --- |
| Install, read, configure, export | `docs/users/` |
| Web host setup and cross-API contracts | `docs/library/` |
| Build, change, test, diagnose, release | `docs/developers/` |
| Types, signatures and local invariants | Source documentation beside the API |
| CLI flags and exact defaults | CLI help and its declarations |
| MVSS syntax and composition | Stylesheet reference, backed by parser tests |
| Measurements and completed investigations | Dated reports in `docs/developers/history/` |
| User-visible release changes | `CHANGELOG.md`, following [its conventions](changelog.md) |

Link to the owner instead of reproducing its table. Do not document field counts,
file lengths, exhaustive internal module inventories or transient tuning values
in architecture prose. Explain the reason for a boundary and its failure modes.

## Write for a task

Begin with the reader's task and prerequisites. Give the shortest working path,
its expected result and likely recovery steps. Put specialized options after it.
Distinguish a supported guarantee from an example, current mechanism or historical
observation. Keep platform-specific setup with the platform guide.

User pages should not contain internal test names or work logs. Developer pages
should explain what to preserve and how to validate a change, not translate every
branch into prose. Public protocols must still specify units, ownership,
completion, cancellation and invalidation: callers should not need source access.

## Change and verify

- Behavior change: update its user task and relevant contract.
- API change: update source documentation and a compiling consumer example.
- Format change: update the reference, parser tests and representative examples.
- Internal refactor: update only explanations whose boundary or rationale changed.
- Investigation: record revisions, environment, commands, evidence and limitations;
  do not present an old measurement as “current”.

Run `python3 scripts/test_documentation.py` for local links, anchors, tables and
navigation. Execute relevant example commands or reuse integration tests that
exercise them. After building Web packages, run `python3 scripts/test_web_documentation.py`
to compile the actual guide snippets against public declarations. Check MVSS
examples with the stylesheet parser. A passing link
check does not establish semantic accuracy.

English guides are the complete reference. The Chinese quick guide is a curated
entry point; label partial translations by their actual scope and link to the
complete guide. Keep commands and API spelling consistent across languages.

## Archive deliberately

Preserve an investigation when its evidence or rejected alternatives are useful.
State its date and source revision when known; never invent missing provenance.
An archive revision only identifies the saved text. Historical claims need not
track current implementation. Keep archived pages out of the main task sequence
and link them through the history index. Routine implementation narration can be
deleted: Git already preserves it.
