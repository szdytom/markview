# Architecture

Use this page to choose where a change belongs and identify the contracts it must
preserve. Build and test instructions are in [Development](development.md).

## Document processing and rendering

```text
Markdown → markview-commonmark → Document + optional source map
HTML     → markview-webpage    → Document + article metadata
                                  ↓
                           markview-layout
                                  ↓
                           LayoutSnapshot
                            ↙           ↘
                  markview-render    markview-pdf
```

The native application and Web adapter supply scheduling, configuration and
resources. They share the semantic document and layout engine. The native reader
is read-only; editing is a Web host capability.

| Owner | Responsibility | Keep outside it |
| --- | --- | --- |
| `markview-document` | Semantic content, identities and document traversal | Parsing, fonts, I/O and geometry |
| `markview-commonmark` | Markdown dialects, parsing, source mapping and incremental reparsing | Window state and resource authorization |
| `markview-webpage` | Article extraction and HTML lowering | Markdown round trips and HTTP transport |
| `markview-layout` | Shaping, line breaking, reading text, geometry, selection mapping and pagination | Windows, GPU objects and application settings |
| `markview-render` | GPU resources, rasterization and painting snapshots | Reinterpreting document syntax or deciding line breaks |
| `markview-pdf` | Vector PDF output from paper layout | Window/GPU dependencies |
| `markview-interaction` | Pointer-driven selection state | Document storage and platform events |
| `markview-ui-core` | Pure component updates, widget identity and interaction | Files, clocks, workers and native services |
| `markview-platform` | Native dialogs, clipboard, opening, input and window adapters | Reader policy and document layout |
| Root application | Sessions, settings, resource policy, workers and UI orchestration | A second parser or layout engine |

The dependency boundary permits the same layout to serve native reading, Web,
headless export and regression tests. [Platform boundary checks](../../scripts/check_platform_boundary.py)
enforce platform isolation; changes to exceptions need a concrete platform reason.

## Semantic identity and immutable snapshots

Source coordinates, semantic identity and drawing indices answer different
questions. An insertion above a paragraph changes its source offset without
changing its content. Reflow changes its drawing geometry without changing the
reader's selection. Do not persist a geometry index or use a source offset as a
content-cache identity.

A semantic position identifies a document revision, block, field and text
boundary. Generated reading text may require a projection into that field.
Positions resolve against the current snapshot; rebasing across a source change
is explicit and can fail when the selected text changed. Link occurrences remain
distinct even when they point to the same address.

`LayoutSnapshot` is immutable and carries both drawing geometry and logical
reading text. Selection, copy, search and painting must agree on that text.
Glyphs alone cannot represent it: ligatures, tabs, inserted hyphens, formulas and
image placeholders break a one-character/one-glyph assumption.

A source map is optional. HTML articles have semantic reading positions without
pretending to possess original Markdown coordinates. The Web API translates
source coordinates to UTF-16 at its boundary; see [source navigation](../library/mvaac-source-api.md).

## Versions and asynchronous work

Content changes and layout requests are separate versions. Width, font and image
changes can reuse a parsed document; source changes cannot. A completion must
match the receiving session and current request before it becomes visible.
Cancellation reduces wasted work but does not replace this acceptance check.

The reader publishes usable prefixes while longer work continues. Publication
must cover the current reading anchor and visible area before replacing a usable
snapshot. Failed reads preserve the last usable document. Pending scroll intent
is separate from the displayed offset, so navigation can wait for geometry
without exposing blank, unfinished content.

Hosts schedule work differently: the native reader uses workers, Web advances
layout from its frame loop, and export waits for settled resources. Layout owns
geometry and caches; it does not decide when a host should accept or display a
result. A time budget cannot preempt every shaping operation, so a very large
atomic block can still delay publication.

Images follow the same version discipline. The host resolves and authorizes a
source, loads pixels, and supplies metadata to layout. Late results cannot attach
to a replacement document. Cache identity must include the receiving authority;
an identical URL or relative path alone does not establish permission to reuse it.

## Why layout is separate from painting

Line breaking needs actual glyph advances, language-aware opportunities and the
sizes of inline objects. Measurement and painting must use the same typographic
model. The paragraph optimizer has a work budget and a legal greedy fallback;
responsiveness must not depend on finding an optimal solution for every input.

Scrolling and selection normally reuse geometry. Font, measure, spacing and
intrinsic image-size changes require reflow. Color-only changes can often repaint.
PDF uses the same layout at the paper's text measure, followed by pagination;
it is not a screenshot of the reader window.

Disclosures retain body geometry and project it through their current open state.
This makes toggles predictable without reshaping, at the cost of initial work and
memory even for closed content. Painting, hit testing and navigation share that
projection. Export opens disclosures for the paper rendition.

## Session persistence

Persist reading intent, not allocated IDs. Saved positions are tied to input
identity and front-end semantics; when those no longer match, restoration uses
scroll fallbacks instead of attaching an old position to unrelated text.
Disclosure overrides follow the same rule. Resource grants are never restored.
The session implementation owns schema versions and migration details.

## Resource and performance boundaries

The host owns filesystem/network authority, image decoding, font downloads and
cache lifetimes. The document only describes resources; layout only consumes
prepared inputs. All entry points must preserve the [security policy](security.md),
including export and clipboard staging.

Bounds apply to specific resources and operations, not to an unconditional
promise about every document's total memory or latency. Measure changes using
[paired performance comparisons](performance.md), including first readable frame,
complete layout and retained memory. A throughput improvement can still delay
first publication or increase memory.

For UI changes, read [Native UI](ui.md). For a new input or semantic construct,
follow [Add a document node](development.md#add-a-document-node).
