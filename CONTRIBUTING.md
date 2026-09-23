# Contributing to Markview

Thanks for wanting to help. Patches of every size are welcome.

Development is a Rust workspace: build, test and check commands, the layer each
change belongs in, and the workflow for a new document node are in the
[development guide](docs/development.md). The boundaries a change should
preserve are in the [architecture](docs/architecture.md). The local conventions
are in [AGENTS.md](AGENTS.md) — they apply to human contributors too. Add a
one-line [changelog](CHANGELOG.md) entry as you go.

## Optimistic merging

Markview is maintained with **semi-optimistic merging**, in the spirit of Pieter
Hintjens' [*Optimistic Merging*](http://hintjens.com/blog:106): a pull request
that is broadly in the right direction and does not break anything gets merged,
even if it is not exactly what a maintainer would have written. Fixing a merged
patch afterwards is cheaper than negotiating it beforehand.

The "semi" is the short list of things that will be held back:

- Personal or sensitive content, such as a key or a credential, anywhere in the
  diff. This is not a merge conflict, it is a leak, and the patch will be
  rejected outright.
- A failing CI run — a test, a lint, or a formatting check.
- A patch that is too large for one pull request. Split it, or say why it cannot
  be split.
- Commit messages that do not follow the convention above.
- A feature that is clearly outside the project's goals, or unrelated to them.

Holding a patch is about the patch, not about you, and a maintainer should say
which item it is and what would make it mergeable. Leaving **Allow edits by
maintainers** on helps: a maintainer can fix the last mile instead of asking for
another round.

## Policy on LLM contributions

**Using a large language model is allowed and encouraged.** These tools let more
people fix more things, and the project does not care which editor or assistant
produced a patch. What it cares about is that a person is behind it. The tool is
welcome; the accountability does not move.

> **Human in the loop.** Review every line you submit and be able to explain why
> it is correct. If you cannot defend a change in review, do not submit it.

Concretely: read the diff, run the checks in the development guide, and be sure
you understand the behavior you are changing. "The model wrote it" is not an
answer to a review question, and a patch nobody can explain will be held until
someone can.

`Assisted-by:` is an optional trailer, `Assisted-by: Claude Code`, for recording
that a tool helped. It is a courtesy, not a requirement, and it does not stand in
for the review above. `Signed-off-by:` must come from a human; a model must never
add one.
