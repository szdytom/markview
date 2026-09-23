# AGENTS.md

Markview is a native Markdown reader. It renders Markdown end-to-end, directly from the source text to the screen, without a browser, WebView, JavaScript, or an external TeX process. Its main goals are fast, low-memory, and publication-quality typography. It is a read-only reader, not an editor.

To AI Agents: NO editing of this file is allowed. This file is for human editing exclusively.

## General Instructions

- Always run tests after making changes.
- Write tests, not too many, mostly integration.
- No error handling for impossible scenarios.
- Simplicity is a virtue: If you write 200 lines and it could be 50, rewrite it.

## General Coding Instructions:

- Use English for all comments in code.
- Write comments using Markdown syntax, even though they are not rendered. For example, "// `object` is borrowed", not "// object is borrowed".
- Keep comments concise: let the code explain itself if possible.

## Rust Specific Instructions:

- Use the latest Rust features whenever applicable.
- Use the usual Rust naming conventions (see Rust RFC 430).
- Any unsafe block must be preceded by a // SAFETY: comment describing why the code inside is sound. Even if trivial, this comment is required to ensure that no extra implicit constraints are applied to the reasoning.

## Git

- Use the Conventional Commits style used by this repository:

	```text
	<type>: <short, lowercase summary>
	```

	Preferred types are `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, and `chore`. Keep the type lowercase, write the summary in the imperative mood, describe one focused change, and omit a trailing period.

- When merging and handling pull requests, keep the commit history clean and linear.

- Format and lint before committing.

- Do not commit unless the user explicitly requested it.

## Changelog

Update the [changelog](CHANGELOG.md) as you go. Never be too detailed. Fit every item in 1-2 lines.

## Runtime Specific Instructions

Some agent harness hides the GPU inside the sandbox. Request a out-of-sandbox command to run related tests.

## Performance Measurement

Build and backup a binary of the current state of the code before making code changes. This makes it easy to compare performance before and after changes later. You can safely delete the backup binary after committing the changes. If you are not editing Rust sources, you can safely ingore this backup requirement.
