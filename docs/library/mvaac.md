# Markview Web components

MVaaC is the framework-independent Web/WASM interface to Markview. The native
application remains a read-only Markdown reader; Web editing belongs to
`@markview/editor`. This guide is the authoritative reusable-component contract.

## Packages and ownership

| Package | Responsibility |
| --- | --- |
| `@markview/viewer` | Engine, canvas input/frame loop, source geometry, reading events and TOC navigation |
| `@markview/editor` | CodeMirror Markdown editing, split layout and automatic bidirectional source following |
| `@markview/scroll-sync` | Continuous position maps, source-anchor projection, input ownership and versioned synchronization requests |
| `@markview/fonts` | Explicit font-file descriptors, loading and reusable caches |
| `@markview/resources` | Explicit browser image transport/decoding, base URL and request configuration |
| `@markview/web` | Deprecated compatibility re-export of viewer and legacy image helpers |

The viewer does not depend on CodeMirror or the resource helper. The resource
helper performs no work until explicitly passed to a viewer. Hosts may supply
`resources.onResources` instead and own transport, caching and concurrency.

## Quick start from built packages

For a source checkout, [build the packages](../developers/web.md#build-and-test)
before importing workspace entries. Give
the mount container a height; text fonts are explicit host assets, never bundled
in the viewer. The official WASM accepts OTF/TTF/TTC and WOFF/WOFF2
files, and only KaTeX fonts are embedded.

```ts
import { Editor } from "@markview/editor";
import { browserResources } from "@markview/resources";
import { loadFontSet } from "@markview/fonts";
import wasmUrl from "@markview/viewer/wasm?url"; // Vite asset import

const fonts = await loadFontSet(
  { sources: ["/fonts/body.woff2", "/fonts/mono.ttf"] }, { wasmUrl },
);
const editor = await Editor.mount(document.getElementById("editor")!, {
  markdown: "# Hello\n\nStart writing here.",
  viewer: {
    fonts,
    resources: browserResources({ baseUrl: new URL("/documents/", location.href) }),
  },
  onChange: ({ markdown, documentVersion }) => console.log(documentVersion, markdown),
});
// Call when the host removes the component.
editor.destroy();
fonts.destroy();
```

An independent preview uses the same explicitly loaded set and resources:

```ts
import { Viewer } from "@markview/viewer";
import { loadFontSet } from "@markview/fonts";
import { browserResources } from "@markview/resources";
import wasmUrl from "@markview/viewer/wasm?url";
const fonts = await loadFontSet({ sources: ["/fonts/body.woff2"] }, { wasmUrl });
const viewer = await Viewer.mount(document.getElementById("preview")!, {
  markdown: "# Preview\n\nRead this independently.", fonts,
  resources: browserResources({ baseUrl: document.baseURI }),
  onSectionChange: heading => console.log(heading?.anchor),
});
viewer.navigateHeading(viewer.outline().entries[0]!.anchor);
viewer.destroy();
fonts.destroy();
```

No CodeMirror or frontend framework is involved.

WASM is exported at `@markview/viewer/wasm` and shipped beside the viewer ESM
entry. Unbundled ESM loads that sibling by default. When bundling JavaScript,
copy the WASM to a served URL and pass `wasmUrl`; Vite's `?url` above is one
example, not a required global or asset convention. esbuild users can use a
`.wasm` file loader or copy the asset explicitly, as `web/build.mjs` does.

## Editing and automatic following

The editor defaults to Markdown highlighting, line numbers, wrapping, standard
editing shortcuts, history, indentation and Markdown list/quote continuation.
`view` exposes the CodeMirror view, and `extensions` accepts CodeMirror
extensions. [CodeMirror's reference](https://codemirror.net/docs/ref/) documents
its state, extension, measurement and keymap interfaces.

`getMarkdown()` returns the current source; `setMarkdown(source)` applies an
editor transaction, updates the preview progressively and sends `onChange`.
CodeMirror normalizes incoming LF/CRLF/CR line endings to LF. Viewer offsets in
an editor refer to that normalized string, matching CodeMirror's UTF-16 document
positions. Independent viewers preserve their exact input string and CRLF
coordinates. Unicode and IME composition use CodeMirror's normal editing path.

`setOptions({ theme, orientation, split, toc, extensions, onChange })` updates
configuration and preserves the editor/history. `theme` is `light` or `dark`;
`orientation` is `horizontal`, `vertical` or `auto`; `split` is the source-pane
fraction (`0.15..0.85`); `toc: false` hides the default directory. Auto layout
stacks narrow containers and hides the TOC on small screens. The separator is
draggable and keyboard accessible. Component styles are scoped to its root;
`--mv-*` CSS variables provide host color overrides.

Following interpolates between matching source-line and preview positions using
one monotonic map in both directions, with shared document-top and scroll-bottom
endpoints. Blank lines and preview margins interpolate between adjacent anchors;
images, inline SVG and math span their complete source extent, including wraps.
Rust batches source-line extents using precomputed UTF-16 line offsets. The editor
retains anchor batches for a progressive layout pass and queries only newly
published blocks; edits and reflows start a new pass. The map is cached until
source geometry or preview layout changes. Input in
either pane takes ownership, cancels pending motion from the other, and prevents
programmatic follow events feeding back. Following
never focuses the other pane or changes its selection. Editor changes map the
previous reading reference through CodeMirror changes. See the
[source, version and TOC reference](mvaac-source-api.md) for pending geometry,
reflow preservation, atomic content and collapsed-content behavior.

The editor delegates projection and synchronization policy to
`@markview/scroll-sync`; its CodeMirror adapter measures source ranges and applies
scroll coordinates. The core has no runtime dependencies, DOM, editor APIs or
WASM initialization. Other editors can reuse the same logic:

```ts
import { ScrollSync, ScrollMap, type ScrollPoint } from "@markview/scroll-sync";
import type { ReadingPosition } from "@markview/viewer";

// Supply these operations from the host editor.
declare const source: {
  scrollTop(): number;
  maxScroll(): number;
  matchingPositions(): ScrollPoint[];
  scrollTo(top: number): void;
};
const sync = new ScrollSync(viewer.outline().documentVersion);
// Rebuild after source, pane size, font, image or layout changes.
let map = new ScrollMap(
  source.matchingPositions(), source.maxScroll(), viewer.reader.markview.maxScroll(),
);

function sourceMoved() {
  const request = sync.begin("source");
  if (!request) return;
  const top = map.map("source", source.scrollTop());
  if (sync.isCurrent(request)) viewer.scrollTo(top);
}

function previewMoved(position: ReadingPosition) {
  const request = sync.begin("preview", position.documentVersion);
  if (!request) return;
  const top = map.map("preview", viewer.reader.markview.scroll());
  if (sync.isCurrent(request)) source.scrollTo(top);
}
```

On source input, call `sync.takeControl("source")` and
`viewer.cancelNavigation()`. On preview input or explicit TOC navigation, call
`sync.takeControl("preview")`. Do not take control for a programmatic follow
event: `begin` rejects events from the follower. After replacing Markdown, call
`sync.setDocumentVersion(viewer.outline().documentVersion)` before following.

Pass `ScrollMap` matching positions in document order, using each pane's document
coordinates. `scrollAnchors` excludes relocated footnotes before filtering so
their document-end geometry cannot displace the main prose's anchors.
Duplicate or backwards positions from hidden content or table cells
are skipped, and positions past either scroll limit give way to the shared bottom
endpoint. `sourceToAnchor` and `anchorToSource` remain available for hosts that
exchange source offsets and fractional progress instead of scroll coordinates;
their measurer must cover the entire range, including all wrapped lines.

For asynchronous measurements or extension/webview messaging, send the plain
`SyncRequest` ticket with the work and call `isCurrent` immediately before
applying its result. A newer request, user takeover, document replacement or
`cancel()` invalidates it. Call `cancel()` when removing the host or invalidating
an in-flight geometry snapshot, then schedule a new request if still mounted.
The core supplies no event listeners, timers or transport. Layout publication
and reading-anchor preservation remain owned by the viewer.

The default TOC uses the complete parsed heading list. Hosts can read
`editor.viewer.outline()` and own their own UI, or disable the component's panel.
Heading navigation opens containing disclosures; ordinary source following keeps
them collapsed. New document versions retire prior targets and resource requests.

## Font ownership and loading

`FontSet.create(bytes, { wasmUrl })` from `@markview/viewer` validates all faces
and snapshots them in WASM. Supply `fonts` to `Viewer.mount`, `Editor`'s `viewer`
options or `CanvasReader.attach`. Low-level `Markview.create` accepts it as the
fourth argument. A set can be shared by many readers; different sets stay
independent. `viewer.setFonts(set)` (or `Markview.setFonts`) starts budgeted
reflow and preserves the source reading reference. `setOptions` preserves faces.
`FontSet.destroy()` is idempotent: existing readers retain their snapshots,
while subsequent attachments/replacements using the released set throw.

The optional `@markview/fonts` helper exports:

| API | Behavior |
| --- | --- |
| `loadFontSet({ sources }, { wasmUrl }?)` | Load with a shared default cache; return a reusable `FontSet` |
| `new FontLoader({ baseUrl, requestInit, fetch }?)` | Separate cache with an explicit URL base and fixed request policy |
| `loader.load({ sources }, { wasmUrl }?)` | Parallel downloads; cache successful bytes and sets; failed entries can retry |
| `loader.clear()` | Evict cache entries without destroying host-owned sets or cancelling in-flight work |
| `fontFiles(baseUrl, files)` | Describe explicit self-hosted/CDN file URLs without fetching |

Sources are `string`, `URL`, `ArrayBuffer` or `Uint8Array`; byte views retain
their bounds. Cached byte sources must be treated as immutable. Equal URL lists
share a set within a loader; byte identities form cache keys. Destroyed sets
are recreated from cached bytes on the next load. The host owns returned sets
and decides when to clear caches/release them. No text faces ship in any package,
no CDN is selected by default, and no CSS parsing, Unicode-range sharding or
runtime missing-glyph downloading occurs. A future VS Code host can read system
font bytes and call `FontSet.create`; the Webview does not discover system fonts.

Official `pnpm --dir web build:wasm` enables Rust's optional `woff` feature. A custom
`cargo build -p markview-web --target wasm32-unknown-unknown --release
--no-default-features` excludes the decoder; WOFF inputs then reject with a
feature-specific message while OTF/TTF/TTC remain valid. A JavaScript option
cannot shrink an already built WASM. Decode and invalid-font failures identify
the input index. [Codec measurements and coverage](../developers/mvaac-font-measurements.md)
record the tested formats, dependency licenses and measured overhead.

## Stylesheet registration and selection

Register custom MVSS after mounting a viewer, then select themes by ID. The host
owns downloads and request caching, so a theme catalog can fetch only the themes
the user selects. Registration parses and validates a sheet once and caches it
in that viewer's engine; switching back reuses its parsed rules.

```ts
const response = await fetch("/themes/paper.mvss.toml");
if (!response.ok) throw new Error(`Theme download failed: ${response.status}`);
viewer.registerStylesheet("paper", await response.text());
viewer.setStylesheets(["paper"]);
viewer.setStylesheets(["bundled:dark"]);
viewer.setStylesheets(["paper"]);
```

`registerStylesheet(id, source)` accepts a nonblank, case-sensitive host ID and
UTF-8 TOML text. `meta.name` remains optional display metadata, independent of
the ID. Registered sheets must support the `ui` destination; omitted `targets`
retain MVSS's default of both destinations. Registration does not change the
current appearance. Successful re-registration replaces that ID's cached rules;
the replacement is applied on the next selection. A failed registration throws
and retains the previous cache entry and appearance.

`setStylesheets(ids)` replaces the complete selection, highest priority first.
For example, `["site-overrides", "bundled:light"]` places custom rules above the
light theme. Every nonempty selection is layered above the shared `builtin`
fallback; light/dark is not an implicit parent. All IDs are resolved before
application, so an unknown or incompatible ID throws without changing the
current selection or interrupting pending layout. Successful selection starts
budgeted reflow; the viewer preserves its source reading position.

Bundled reader IDs are `bundled:light`, `bundled:dark`, `bundled:celadon`,
`bundled:blueprint`, `bundled:rosewood` and `bundled:8-bit`. The `bundled:` prefix
is reserved and cannot be registered or overwritten by hosts. PDF-only themes
and the hidden `builtin` are not selectable reader themes.

`setStylesheets([])` restores the current `MarkviewOptions.theme` default.
`setOptions` retains an explicit style selection while changing reading
parameters; `theme` applies again after clearing that selection. Each viewer has
its own registry, released by `destroy()`. The low-level `Markview` exposes the
same registration and selection methods, including through
`CanvasReader.markview`. Fonts remain explicit `FontSet` assets: registering or
selecting a stylesheet never downloads fonts.

## Images and host transport

`browserResources({ baseUrl, requestInit, onError })` from `@markview/resources`
fetches explicit image requests and delivers decoded pixels. `decodeImage`
accepts Blob/ArrayBuffer/Uint8Array; `loadImageUrl(request, options)` combines
fetching with request delivery. All respect the request's cancellation signal
and preserve byte-view bounds. To replace transport entirely, provide
`resources.onResources(events)` and call each request's `resolve(pixels)` or
`reject(error)`; priority events allow host scheduling. See the
[resource protocol](mvaac-resources.md) for the
low-level contract.

Layout completion means source geometry is published, not that all images have
arrived. Late pixels cause budgeted reflow with source-position preservation.
Replacing source or destroying a component cancels obsolete requests; stale
results cannot update the new document. URL bases belong to the host, including
any future local-file protocol supplied by a VS Code plugin.

## SVG and content support

SVG file URLs, `data:image/svg+xml` URLs, Blob/ArrayBuffer/Uint8Array decoding
and raw `<svg>` elements all render as static image pixels. Inline elements
travel through the same host image protocol as other images, with a percent
encoded data URL and an atomic source range. Complete elements may span blank
lines, contain nested SVG/comments/CDATA and appear in paragraphs or containers.
A missing root `xmlns` is supplied for inline elements; supplied SVG files/bytes
must be valid XML. The browser determines text/font painting inside SVG, which
is separate from WASM text `FontSet` shaping. SVG is rasterized once at its
intrinsic browser size; scripts, live DOM interaction, animation playback and
vector export are outside this interface.

The resource helper accepts internal fragment references (for example
`<use href="#shape">` and `url(#gradient)`) and embedded `data:image` references.
External dependencies, including relative image/`use` URLs and external CSS
URLs/imports, are unsupported and rejected with `SVG external resources are
unsupported`. Absolute external dependencies are also excluded by image-mode
SVG. Hosts needing them must produce a self-contained SVG before delivery.
Malformed XML rejects decoding; resource rejection produces the reader's image
error placeholder. An incomplete/unclosed inline SVG retains the raw-HTML
fallback while it is being edited. This is static SVG support, not a full HTML
or SVG document renderer. Mermaid remains deferred and its image request is
unsupported by the browser helper.

| Content | Rendering and source following |
| --- | --- |
| Paragraphs / emphasis | Shaped/wrapped lines; nearest semantic cluster, including long paragraphs |
| Headings | Complete ordered TOC, duplicate-safe anchors, source ranges and deferred navigation |
| Nested lists / quotes | Internal lines/items retain source ranges; no whole-document percentage mapping |
| Code | Syntax colors and line following; wide blocks can pan horizontally |
| Tables | Styled cells and rows, internal text following and horizontal overflow handling |
| Inline / display math | Embedded KaTeX faces; formulas are atomic ranges; unsupported TeX can show errors |
| Raster / SVG images | Explicit host pixels or optional browser transport; atomic ranges and late-image reflow |
| Links | Internal anchors use reader navigation; external targets use the host `onLink` callback |
| Disclosures / front matter | Folded visible-container fallback; TOC navigation opens containing disclosures |
| Supported raw HTML | Small semantic subset plus SVG; unsupported HTML displays source, not a browser page |

Escapes/entities, invisible syntax, whitespace and shaped ligatures do not give
one source character per rendered glyph. Horizontal clipping and hidden bodies
use the available visible geometry. Content/image bounds and unsupported input
follow engine limits; these packages do not promise arbitrary HTML/CSS or TeX
compatibility. Actual canvas and split-editor integration tests cover the matrix,
including tall late images, long wrapped tables, nested containers, collapsed
bodies, Unicode, editing, font replacement and source-anchor preservation.

## Lifecycle and examples

Every mount creates owned DOM and engine state. Multiple instances are supported;
call `destroy()` before removing/replacing an instance. Destruction is idempotent
and stops listeners, frame loops and resources. No filesystem, file manager,
PDF export, framework wrappers, VS Code plugin or Mermaid rendering is supplied
by these packages.

`apps/demo` consumes built public entries and presents one SPA at `/index.html`.
Read and Edit share the same document, editor history and viewer; it demonstrates
source following, TOC, themes, a draggable split, file opening and Markdown
downloads. `/editor.html` redirects to `/index.html#edit`. Sample drafts remain
in memory while switching documents; download changes before reloading. The
legacy low-level reader regression host lives under `tests/fixtures/reader` and
is built only for tests. The SPA composes the font and resource helpers with
version-pinned Noto font files. Fontsource WOFF2 downloads use npmmirror,
falling back to jsDelivr on HTTP or network failure. Latin Serif and Sans cover
100–900 weights in upright and italic variable faces; Latin Mono covers the
same upright weights. Simplified Chinese Serif supplies 400/500/600/700 and
Sans supplies 400/500/700 static faces. The 600 Serif face keeps disclosure
summaries from falling through to unrelated fonts and Emoji spaces. Full CJK
monospace OTF and bitmap color emoji TTF remain upstream jsDelivr files.
Its UI serif face reuses the same WOFF2 bytes through
the browser's `FontFace` API, avoiding a separate CSS download. The demo
requires network access on first load and stores successful font responses in
Cache Storage (`markview-demo-fonts-v1`), keyed by their version-pinned URLs.
Reloads reuse these bytes without contacting the CDN; changed URLs fetch new
files. Recovered fallback bytes are cached under the primary URL as well.
Storage failures fall back to downloading, and failed HTTP responses are
never cached. A fully downloaded batch that fails validation is evicted before
reporting the startup error, allowing retries to fetch recovered CDN files,
including when malformed bytes came from an existing cache entry.
This persistence belongs to the demo; `FontLoader` itself caches
only in memory. Tiny pinned subsets stay under `tests/fixtures` for deterministic
regression tests. No text fonts ship in the demo or libraries.

## Migration

Use `@markview/viewer` for `init`, `Markview`, `LayoutUpdate`, `CanvasReader`,
`Viewer`, `FontSet` and their types. Move `decodeImage`/`loadImageUrl` imports to
`@markview/resources`. `@markview/web` re-exports both for existing callers;
it is a compatibility package rather than a separate engine instance.
Legacy `init({ fonts })` remains a first-successful-initialization default for
readers without explicit sets. Migrate to `loadFontSet`/`FontSet.create` and
per-reader `fonts` for independent instances.
`packages/markview` is now the viewer's source directory; `packages/web` is the
compatibility directory. The old demo contract is historical and its exclusions
of editing/outlines do not constrain this iteration. Public source/navigation
API lives in this guide and [source navigation and TOC](mvaac-source-api.md).

### Markdown dialect

`MarkviewOptions.markdownFlavor` selects `"gfm"` (the compatibility default) or
`"obsidian"`. `hardBreaks` controls single-newline hard breaks independently;
`highlightMarks` enables `==highlight==` in GFM and is forced on in Obsidian.
`gridTables` enables Grid tables independently of the preset and defaults to
`true`. Set it to `false` to read grid borders as ordinary Markdown.
The Obsidian preset uses the core reader's supported callouts, comments and
inline HTML extensions; it does not implement full vault semantics.

Changing these options with `setOptions()` reparses the current source, including
the newest source in an unfinished layout. Existing layout handles become stale
and source geometry receives a new document version. Layout-only changes do not
reparse the document.
