# Architecture

This document explains what the major parts of Markview own and why the boundaries exist. It is intended for someone reading the implementation, not for someone trying to add a feature; procedural guidance lives in [the development guide](development.md).

## The three-layer pipeline

Markview is a read-only desktop application split across three Cargo packages:

```text
Markdown / assets
        │
        ▼
markview-core: semantic document → immutable layout snapshot
        │
        ├── markview-render: snapshot → GPU frame
        │
        ├── markview-pdf: snapshot + pages → PDF content streams
        │
        └── markview application: files, settings, input, and lifecycle
```

`markview-core` is window- and GPU-independent. It parses Markdown and the supported raw HTML subset, represents semantic blocks and inline content, shapes text, lays out paragraphs, measures math and images, and exposes reading text, selection geometry, links, and draw instructions.

`markview-render` consumes those instructions. It owns the wgpu device and surface, glyph and image resources, clipping, colors that can be changed without reflow, and headless output. It does not contain a second document layout engine.

`markview-pdf` consumes the same snapshot for paper. It re-lays the document at the page's text measure, breaks the column into pages, and writes vector content through `krilla`: text as glyph runs with subset fonts and a character map, math, rules, boxes, images, and link annotations. It owns no window, no GPU, and no source parsing.

The root package owns effects that must touch the operating system: launching, file and settings I/O, file watching, image loading, clipboard access, platform link opening, window events, and background work. The UI translates gestures into commands; it does not define document semantics.

The separation matters because the same core layout is used by the interactive window, the renderer tests, and the offscreen render and benchmark modes.

## Semantic identity and immutable snapshots

Parsing produces a `Document` made of blocks and rich inline content. A block keeps its source range for diagnostics and a semantic identity for cache reuse. Source positions are not used as identity: inserting text above a block must not make every later block appear to be a different kind of content.

A reload re-parses the whole file, but a small edit to a document whose top-level blocks are plain leaves separated by blank lines re-parses only the block the edit fell in and reuses the rest, shifting the source ranges of the blocks after it. Documents with containers, code, tables or reference definitions take the full parse, because those can span a blank line or carry meaning outside their own block.

Layout produces an immutable `LayoutSnapshot`. A snapshot contains final geometry, logical reading text, text clusters, link hit regions, overflow information, and drawing instructions. The renderer and interaction code can therefore read the same result without mutating the layout engine or rebuilding text for copying.

The reading index is deliberately separate from glyphs. Grapheme boundaries, shaping clusters, formula ranges, image fallback text, and code whitespace all need a stable logical mapping even when visual layout inserts hyphens, expands tabs, or replaces an unavailable asset with a placeholder. Selection and copying operate on that logical mapping, so reflow changes rectangles but not the meaning of a selection.

## Versions and asynchronous work

The application distinguishes a content version from a request version. A file open or reload changes content; a type-size, column, alignment, or hyphenation change changes only the requested layout. The worker retains the last parsed document and publishes snapshots tagged with both versions.

Only the newest request may be accepted. A late result cannot replace a newer layout, while a reload cannot be lost merely because a reflow request occupied the worker's single pending slot. Failed reads keep the last usable snapshot, because a transient editor save should not blank the reader.

For sources of at least 32 KiB, the window worker publishes completed prefixes after they cover the current
viewport plus half a viewport of prefetch. Parsing still covers the entire source,
so references and other document-wide semantics are resolved before layout starts.
Prefixes and the final snapshot share immutable block geometry; publishing does
not restart layout. Without a new viewport target, subsequent publications need
both twice as many blocks and at least 32 ms since the previous publication, so
copying snapshot metadata does not grow quadratically with document length.
Smaller documents publish once to avoid extra snapshot and redraw overhead;
they still check cancellation between blocks.

Every prefix carries the same request and content versions as the final result.
Cancellation is checked between top-level blocks; changing the file or layout
settings supersedes the old work. The UI checks the version again before accepting
an event. Reload prefixes replace the old snapshot only when they cover its
reading anchor and visible area (and any existing selection); otherwise the old
snapshot remains visible until a sufficient prefix or the final result arrives.
An appended prefix preserves the active selection gesture and scroll position.

Scroll intent is separate from displayed scroll. Repeated PageDown presses
accumulate a target even beyond completed geometry; the worker prioritizes
publishing a prefix that covers that target. The current page stays visible until
the target is available. PageUp reverses the pending target and Home cancels it;
End waits for the final height. The document scrolls until its last line can
sit one third of a page below the top, leaving the rest blank; an end already
higher than that does not scroll. While geometry is incomplete, the footer
shows loading, the document scrollbar is hidden, and Select All waits for
completion.
The implementation does not estimate total height or skip preceding blocks.
One very large top-level paragraph, table, list, or code block can still delay
publication and cancellation until that block finishes.

Images follow the same model. Loading and decoding happen outside layout. A decoded image changes the version of the affected source, causing only dependent blocks to reflow; the document's semantic reading identity, selection, and reading position remain stable.

## Why layout is separate from painting

Paragraphs are shaped before painting because line breaking needs real glyph advances, language-aware break opportunities, hyphenation, inline formulas, and atomic image boxes. Inline code adds its own rule on top: since it carries no hyphenation dictionary, every character boundary inside a code run is offered as a break — free at a word edge, and at a small penalty inside a word — so a long identifier wraps rather than overflowing. Markview uses a bounded Knuth–Plass-style optimizer for ordinary paragraphs and falls back to legal greedy breaks when a paragraph exceeds the candidate budget or has no valid optimized solution. The fallback protects responsiveness without making invalid breaks.

Justification and line breaking share one microtypographic model, in `microtype`. Each shaped cluster carries how far its advance may stretch or shrink, which the optimizer sums into line metrics and the painter spends through a single ratio. Word spaces and a bounded amount of letter spacing are used first; whatever slack is left is then shared evenly over the clusters that can take it, which is what closes a CJK line that has no word spaces. Both bounds are reader settings, so a narrow column can trade even spacing against tighter or looser words.

East Asian punctuation gives back the blank half of its em box at a line start or end, and Han text is spaced a quarter em from Latin, both following the W3C Requirements for Chinese Text Layout. Which half a mark gives up depends on the reader's `cjk-type`, since the mainland, Taiwanese and Japanese conventions place the comma-like marks differently. A closing mark also hangs part of its advance into the end margin, which is what makes a justified line read as flush, and a CJK quotation mark may take the line edge its convention asks for even though UAX #14 forbids a break on either side of one. Because the same numbers drive measurement and painting, a drawn line is the line the optimizer chose.

Page breaking is the one layout step that exists only for paper. `markview-core::paginate` collects each block's drawn lines into bands, records the space each band needs together with the lines widow and orphan control refuses to separate from it, and distributes the bands over fixed-height regions, following the model Typst uses for flow layout. A block owns its whole vertical extent, so a fragment that opens a page starts at the block's top edge while a continuation starts at its first line. The reader never runs this pass.

Math is laid out as an atomic display list and images as atomic inline boxes. This keeps their baseline and height in the line model. Images do not create a float band: text never wraps around their sides. An image-only paragraph is centered and may receive a caption; mixed content remains an inline paragraph.

Painting is consequently a projection of an already-decided layout. Scrolling and selection only change which geometry is visible and which overlays are painted. Theme colors can often be late-bound; font, width, spacing, and other geometry changes require reflow.

## Interaction and platform effects

The application owns focus, hover, selection gestures, scrolling, scrollbar grabs, and modal input: the settings, stylesheet and export panels and the local-file confirmation. Core owns hit testing and selection geometry so those operations remain testable without a window or GPU.

Links are activated only on a matching, non-drag release. `src/link.rs` is the single policy for what a document-controlled link may do: Markdown opens as a reader tab, an inert allowlist of files and any directory goes to the system handler, and everything else is shown in a confirmation first, whose default action opens the containing folder. [Security and threat model](security.md#t6-local-links) owns the allowlist and its residual risks. A document that names more remote images than the per-revision cap allows shows a notice strip below the tab bar with Dismiss and Load all; the strip reserves its own band rather than covering text. A heading fragment moves the reader to that heading: `#anchor` inside the current document, or `file.md#anchor` after the target tab opens. Anchors are the GitHub slugs of heading text, and a link that uses a different slug rule is reported as a missing heading rather than guessed at. Markdown is never opened for writing. Clipboard output is reading text: code preserves meaningful whitespace, tables use tabs, formulas contribute LaTeX, and Markdown markers are omitted.

Settings are layered as defaults, user TOML, then explicit command-line overrides. Interactive changes may persist user preferences; render, benchmark, and smoke modes intentionally avoid personal configuration so their output is reproducible. Stylesheets are parsed and merged transactionally: an invalid update leaves the last effective stylesheet in place.

## Resource and performance boundaries

The practical performance boundary is not a promise about every Markdown file. Ordinary paragraphs have a line-break budget, image decoding has byte and pixel caps, and CPU image pixels and GPU textures have independent budgets. Caches are bounded or scoped to the current document where possible.

Measured baselines and their environment live in the [performance model](performance.md). Compare like-for-like runs: fonts, drivers, DPI, pathological paragraphs and large assets all affect the result.

## Internal ownership

The package boundaries also apply inside each crate. Entry points compose concrete
components; helpers receive borrowed inputs instead of an application-wide context.

| Component | Owns | Boundary |
| --- | --- | --- |
| Application `Tabs` | Active session, inactive tabs, request serial | Tab transitions return to the window adapter for watching, redraws and requests. |
| Application `Preferences` | Effective settings, persistence store, stylesheet catalog, save deadline | Stylesheet validation finishes before the effective sheet and UI appearance change. The application applies successful changes to the renderer. Export preferences live beside the reader's, never inside them. |
| Application export | One export's settings, its background job, the PNG strip loop and the watch target | Reads and lays the document out itself at the export's own options, so it never requests a reader layout; only PNG strips touch the shared GPU device, one per frame, with the export's own stylesheet. |
| Application tab strip | Scroll offset, drag gesture and cached filename widths | Pure strip geometry drives both painting and hit testing. Reordering moves sessions without submitting layout requests; clipped draw groups contain overflow. |
| Application chrome | Borrowed display state and compiled icon buffers | Controls, footer, tabs and styles produce geometry without window, worker or configuration I/O access. Shared buttons and grouped forms own drawing, clipped pointer regions and focus geometry; panel scroll offsets stay in interaction state. Selection-count caching remains in the application adapter; icons stay editable SVG files that the `markview-icon` macro parses into vector buffers at compile time, so no SVG parser reaches the binary. |
| Image scheduler | Versioned entries, jobs and published snapshot | Source reads, bounded decoding and allocation-aware pixel eviction are separate modules. |
| `LayoutEngine` | Document block cache, shaping/math resources, highlight owner | Snapshot assembly and invalidation stay at this entry point; immutable stylesheet identity is computed once per document pass. |
| `paginate` | Band segmentation, page distribution, page furniture | Pure geometry over a settled snapshot: no fonts, no I/O, and no effect on the reader's layout. |
| PDF painter | Font subsets, glyph runs, page content, annotations | One export owns its krilla document; nothing it embeds outlives the call. |
| Block layout context | Borrowed shaper, math engine, image snapshot and completed highlights | Inline preparation, paragraphs, code, images, tables and containers cannot start jobs or invalidate document caches. |
| Renderer `Gpu` | Device, queue, surface and device-loss state | Owns acquisition, resize/recovery, completion and offscreen readback. |
| Renderer raster cache | Atlas, raster keys, scaler and math fonts | Glyph/path preparation borrows the queue; paths write to the shared geometry buffer. |
| Renderer image textures | Texture cache, image runs and current-frame demand | Owns budget checks, version pruning and atomic demand publication. |
| Renderer geometry | Vertices and reusable GPU vertex storage | Produces clipped quads; the frame adapter preserves paint order and submission. |

Parsing and representation have separate source modules: Markdown translation
owns the comrak reader, settings persistence owns the serialized configuration,
and stylesheet parsing owns validation. Reading selection and visual hit testing
share the immutable text nodes without rebuilding their logical content.

Module extraction does not change line-break budgets, cache keys, worker counts,
image budgets, request acceptance, or ordering of draw commands. Hot paths use
concrete types and borrowed resources, with no dispatch registry or additional
synchronization. A source-file target of about 500 production lines is a review
heuristic, not a reason to split an otherwise cohesive algorithm. Tests live next
to their owning modules in separate sources when they obscure production code.
`scene.rs` remains a small exception at about 530 lines: it keeps the shared
immutable drawing, viewport and scrollbar geometry vocabulary together.
