# Embed an editor and synchronize scrolling

Mount `Editor` when the host needs editable Markdown beside the reader.
[Build the packages](../developers/web.md#build-and-test) first. Fonts, image
transport and WASM deployment follow the [component setup](mvaac.md).

## Mount

Give `#editor` an explicit height and serve the named font assets:

```ts
import { Editor } from "@markview/editor";
import { browserResources } from "@markview/resources";
import { loadFontSet } from "@markview/fonts";
import wasmUrl from "@markview/viewer/wasm?url";

const fonts = await loadFontSet(
  { sources: ["/fonts/body.woff2", "/fonts/mono.ttf"] }, { wasmUrl },
);
const editor = await Editor.mount(document.getElementById("editor")!, {
  markdown: "# Hello\n\nStart writing here.",
  viewer: {
    fonts,
    resources: browserResources({ baseUrl: new URL("/documents/", location.href) }),
  },
  onChange: ({ markdown }) => console.log(markdown),
});
const viewer = editor.viewer;
// When unmounting: editor.destroy(), then fonts.destroy() once no host needs it.
```

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
