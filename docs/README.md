# Documentation map

Markview's documentation is deliberately split by reader intent. Each page has one job.

## Start with the product

The root [README](../README.md) is the user-facing entry point: screenshots, a
summary of the measured performance, installation, reading controls, supported
content, limitations, and a short customization example. The `docs/screenshots`
images it embeds are reproduced by `scripts/capture_screenshots.sh`.

- [Comparison](comparison.md) records how the README's typography figure is produced, and what it does and does not claim.

## Understand the implementation

- [Architecture](architecture.md) explains ownership, snapshots, versions, layout, interaction, and resource boundaries. It focuses on what the system guarantees and why.
- [Performance model](performance.md) explains the measured terms, current baselines, and the limits of those numbers.
- [Latency and memory analysis](performance-analysis.md) is the diagnostic page: first-frame, edit-latency and memory measurements against explicit targets, with the responsible code and ranked optimization points.
- [Security and threat model](security.md) explains which parts of the system an untrusted document can reach, what has already been shown to break, and which risks are knowingly accepted.

## Change the implementation

- [Development guide](development.md) is the how-to page for building, testing, changing behavior, and adding a new document node.
- [Stylesheet guide](stylesheets.md) is the how-to/reference page for authoring and installing MVSS themes.

## Ship the implementation

- [Packaging and releases](packaging.md) is the maintainer page for release assets, the cargo-dist configuration, and per-platform runtime requirements.

When a fact belongs to more than one page, keep the detailed explanation in the page that owns the concept and link to it elsewhere. In particular, keep commands and procedures out of architecture documentation.

