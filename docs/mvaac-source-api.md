# MVaaC source navigation and TOC

The iteration contract is [mvaac-design.md](../artifacts/mvaac-design.md).
This page describes the implemented source/navigation API. CodeMirror composition,
new package entry points and instance font sets are still being implemented.

## Coordinates and publication

`SourceRange { start, end }` counts zero-based UTF-16 code units in the original
Markdown string, with a half-open end. CRLF counts as two units; offsets inside a
surrogate pair snap to the character start. `SourceGeometry.rect` uses document
CSS pixels, with y growing downwards. Its `documentVersion` changes when source
is replaced; `revision` changes on layout publication. A replacement can retain
the prior document's picture while laying out its new prefix, but source queries
return `null` until geometry for the new version is published.

`Markview.sourceToPreview(offset)` locates the rendered cluster containing or
nearest to a source position. `Markview.previewToSource(y)` selects the closest
visible line and its leftmost source cluster. Long prose follows wrapped lines;
code follows its own lines. Wide code and tables use their current horizontal
scroll offsets and clip queries to visible content. Lists, quotes and cells retain internal semantic
ranges, including when their geometry is reused after source insertion.

Syntax markers, blank lines and transformed text use nearby visible content.
Math/images are atomic ranges. Collapsed bodies map to their visible disclosure
content; ordinary source navigation does not expand them. These queries describe
approximate source geometry, not a glyph-to-source bijection. Geometry queries do
not advance or synchronously finish layout.

## Mounted viewer

After `init({ wasmUrl, fonts })`, `Viewer.mount(container, { markdown })` creates
its own canvas and uses the existing `CanvasReader` input and frame loop. Give
the container an explicit height. `getMarkdown`, `setMarkdown`, `setOptions`,
`outline`, `sourceToPreview`, `previewToSource`, `readingPosition`,
`currentSection`, `scrollToSource`, `navigateHeading`, `onReadingPosition` and
`destroy` form the component API. `reader` provides the same low-level canvas API.

`scrollToSource(offset, fraction = 0)` waits for unpublished geometry through the
normal budgeted layout loop. A new navigation, document replacement or user input
cancels the old target. It never takes focus. `fraction` is the displacement into
a rendered line divided by its height, and may be negative in inter-block gaps.
`setMarkdown(markdown, preserveOffset?)` starts progressive replacement; callers
that edit source can pass the reading offset mapped through their edits.

`ReadingPosition` contains document version, layout revision, source `offset`,
line `fraction`, current heading, and `reason`: `user`, `programmatic`, or
`reflow`. Subscribe using `onReadingPosition(listener)`; the returned function
unsubscribes. Following a programmatic event should not start reverse following.
Width, options and asynchronous image reflow retain the source reading anchor.
Destruction is idempotent and removes the owned DOM, input handlers, frame loop,
subscriptions and pending resource requests. Calls after destruction throw.

## Outline and headings

`outline()` returns `{ documentVersion, entries }`. Every entry contains `text`,
`level`, unique `anchor`, and original `source` range, in document order. The TOC
is complete as soon as parsing finishes, including headings inside collapsed
containers; it does not wait for layout. A document without headings returns an
empty list and has no current section.

`navigateHeading(anchor)` returns whether a heading exists, expands its enclosing
disclosures, and waits for its layout. It uses the same navigation path as reader
anchor links. The active heading is the last heading whose source start precedes
the top reading reference. `onSectionChange` in mount options reports changes,
including replacement with a new document version. Hosts own the TOC UI.

## Verification

`cargo test -p markview-core --test source` verifies Unicode coordinates, CRLF,
long paragraphs/code, cells/quotes, cached geometry, deferred targets and nested,
adjacent and quoted disclosure headings. `pnpm --dir web test
 tests/navigation.spec.mjs` exercises the built package with real canvas rendering,
version replacement, pending navigation, focus, reflow, input cancellation and
repeated mounting. Existing reader/resource/font regression suites remain in use.
