# MVaaC implementation contract

This document records implementation decisions for `mvaac-iter-plan.md`.
Implementation and acceptance evidence are tracked below; a design decision alone is not acceptance evidence.

## Packages

- `@markview/viewer`: engine, canvas input/frame loop and container-mounted `Viewer`.
- `@markview/editor`: CodeMirror Markdown editor and viewer composition. Uses only the viewer public mapping/navigation APIs.
- `@markview/fonts`: explicit URL/byte font sets and reusable fetch cache; no bundled text faces or automatic CDN requests.
- `@markview/resources`: opt-in browser image transport, decoding, base URL and request options.
- `@markview/web`: deprecated compatibility entry re-exporting the viewer's low-level API. Existing global initialization fonts remain a compatibility default, while explicit per-instance font sets take precedence.

The viewer ships WASM beside its ESM entry, with an exported WASM asset path and explicit `wasmUrl` override for bundlers. Auxiliary packages remain optional. No frontend framework is required.

## Coordinates, versions and mapping

All public source offsets/ranges count UTF-16 code units from zero; ranges are half-open. Rust source byte ranges convert against the original string, preserving CRLF. An offset inside a surrogate pair snaps to the character start. Geometry is in document CSS pixels, with downward-positive y; viewport positions add the engine scroll offset.

A document version changes only when source is replaced. Layout revision changes on publication, including progressive publication and reflows. Queries and events identify both; old-version targets never drive a replacement document.

Mapping follows rendered line geometry and inline source ranges for prose, and source lines for code. Nested blocks/cells use their own ranges. Atomic content maps to its source range. Invisible syntax and whitespace choose the nearest rendered source position, preferring the following position on a tie; collapsed bodies fall back to their disclosure header. Mapping is approximate for transformed text, ligatures, math and generated labels, and does not promise one rendered glyph per source character.

Navigation to source keeps disclosures collapsed by default; heading navigation opens enclosing disclosures. A source navigation request for unpublished content remains pending and advances only through the normal budgeted frame loop. New source, navigation or user input cancels an obsolete target.

## Scrolling and preservation

The synchronization reference is the top visible content position, with the line's fractional vertical displacement retained. Events distinguish user motion, programmatic navigation and reflow; programmatic following never initiates reverse following. Wheel, pointer and keyboard input on either pane immediately take ownership. Synchronization does not focus either pane or interfere with text selection.

On reflow, retain the source reference rather than the document scroll fraction. On edits, map the retained source offset through CodeMirror's changes before restoring it in the new viewer document. Pending navigation is version-scoped. Resources are independently pending after layout completion and can cause reflow; replacing a document or destroying a component aborts prior requests.

## TOC and components

TOC comes from the complete parsed document, including nested/closed headings, in source order. Entries contain text, level, unique anchor and source range. Heading navigation and active-section notifications use that same data. A no-heading document has an empty outline and no current section.

`Viewer.mount(container, options)` and `Editor.mount(container, options)` own only their created DOM. Destruction is idempotent, removes listeners and releases engine state. An editor exposes its CodeMirror view and extension configuration, document get/set/change notifications, layout/theme configuration, draggable separator and optional TOC. Styles are scoped to component roots.

## Delivery and verification

1. Contract and package decisions: this document; baseline TypeScript checks and 53 browser tests passed. Native release binary and current WASM backed up in `artifacts/`.
2. Engine source mapping, UTF conversion and TOC implemented, including disclosure source restoration and cached geometry binding. Mounted viewer navigation/events implemented; CodeMirror synchronization remains pending. Evidence: core source integration tests, built-package browser navigation tests and existing reader regressions; API documented in `docs/mvaac-source-api.md`.
3. Public viewer and CodeMirror synchronization implemented and verified with real long paragraph/code scrolling, focus checks, history/list continuation, Chromium IME, TOC, divider, responsive layout and multiple-instance tests.
4. Viewer/editor/resource/compatibility package extraction and built-entry examples implemented. Independent resource helper has no runtime/type dependency on the viewer. Instance font sets, the independent font helper and optional WOFF codecs implemented and verified.
5. SVG scope, regression matrix and authoritative user documentation: pending.

Each implementation stage updates the changelog, formats/lints, runs relevant tests and is committed separately. Final acceptance also exercises built package entries and rendered browser output. WOFF decoder selection requires WASM compilation, license/format checks and measured size/startup evidence. Confirmed SVG scope: file references, data URLs, host bytes and inline `<svg>` rendered as static images. Relative external resources inside SVG are unsupported. Mermaid is deferred.

### Source/viewer stage evidence (2026-10-02)

- Initial reader baseline: 53 browser tests passed; TypeScript checks passed; release native binary backed up before Rust edits.
- Core suite and 14 web-state unit tests passed. After final disclosure-coordinate changes, 81 parser tests and four source integration tests passed.
- Full browser regression: 56 tests passed; after the last source changes and added cancellation test, all 14 navigation/input tests passed. The new viewer screenshot was inspected and contains rendered heading/body text.
- Workspace/all-target native Clippy, WASM Clippy, Rust formatting, TypeScript checks and the official WASM/package builds passed.
- Uncompressed WASM baseline: 14,864,809 bytes; source/viewer stage: 14,887,278 bytes (+22,469 bytes). Codec and startup measurements remain pending.
- This is partial iteration progress; subsequent sections record CodeMirror/package/font completion. SVG and the final content/support audit remain required.

### Editor/package stage evidence (2026-10-02)

- Full built-package browser regression: 62 tests passed. After isolating resource helper types and formatting, all 13 editor/resource tests passed.
- TypeScript checks cover all libraries and both examples; strict declaration consumption also passed with `skipLibCheck: false`. Added `pnpm --dir web lint` and scoped formatting commands.
- The editor screenshot was inspected: source highlighting, directory, reading text and separator render correctly. The responsive narrow layout test passed.
- Native outline regression: 22 tests passed; its optional GPU screenshot test was ignored (browser rendering was independently verified).
- `pnpm --dir web test:packages` produced and installed actual tarballs in an isolated consumer, checked declarations with `skipLibCheck: false`, bundled and initialized/mounted/destroyed in Chromium successfully. Final font/SVG and full requirement-by-requirement acceptance remain pending.

## Font stage verification

- Per-instance/shared FontSet ownership, explicit FontLoader cache/CDN descriptors, retry, byte views, raw TTC and CFF/TrueType-color WOFF/WOFF2 paint validated through built entries.
- Native font/publication suite: 15 tests pass with codecs enabled and 15 disabled. WASM Clippy passes with both feature configurations; official enabled and custom disabled release WASM builds pass. Disabled decoder rejection verified in Chromium.
- A long-document font change exposed loss of the source anchor while a progressive prefix omitted its target. Viewer now retains a source target until geometry and enough viewport content are published; 7 font/navigation tests pass.
- Codec overhead: +243,141 raw bytes, +105,576 gzip bytes. Compilation/instantiation and font input medians, licenses and fixture limitations are recorded in `docs/mvaac-font-measurements.md`; raw measurements remain ignored artifacts.
- Editor example and isolated tarball consumer now explicitly compose all four public component/helper packages.
