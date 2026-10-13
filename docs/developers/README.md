# Develop Markview

For contributors changing Markview and maintainers verifying or shipping it.
Start with [contributing](../../CONTRIBUTING.md) and the development guide.

## Build and change

- [Development guide](development.md): build, test, choose a layer and change behavior.
- [Changelog conventions](changelog.md): entry format, release headings and automated checks.
- [Architecture](architecture.md): pipeline, ownership, snapshots, versions and resource boundaries.
- [Native UI](ui.md): pure component updates, host effects and shared widget geometry.
- [Web component development](web.md): WASM/TypeScript workspace, demo, builds and verification.
- [Android development](android.md): platform implementation, APK builds and headless emulator tests.
- [Packaging and releases](packaging.md): release assets, configuration and release procedures.

## Measure and verify

- [Complexity triage](complexity-triage.md): per-function decisions and commits, with explicit exception policy.
- [Performance model](performance.md): methods, baselines and measurement limits.
- [Latency and memory analysis](performance-analysis.md): diagnostics and optimization priorities.
- [Comparison](comparison.md): typography figures and comparative measurements.
- [Security and threat model](security.md): policy, trust boundaries and accepted risks.
- [Security reference](security-reference.md): threats, controls, budgets and historical findings.
- [Security verification](security-verification.md): evidence, coverage and outstanding work.
- [Document search verification](search-verification.md): automated checks and GUI checklist.
- [Rendering regression tests](rendering-tests.md): visual fixtures, MVSS coverage, pinned fonts and baseline review.
- [Web font codec measurements](mvaac-font-measurements.md): format coverage and size/startup measurements.
- [Fuzzing](../../fuzz/README.md): targets, oracles and campaigns.
- [Test fonts](../../tests/fixtures/fonts/README.md) and [Web test fonts](../../crates/markview-web/tests/fonts/README.md): fixture provenance and regeneration.

## Historical design

- [Initial MVaaC demo contract](history/mvaac-web-demo.md): historical scope and interfaces; current consumer APIs live in the [library guides](../library/README.md).

[Documentation](../README.md) · [Changelog](../../CHANGELOG.md) · [Third-party notices](../../THIRD_PARTY.md)
