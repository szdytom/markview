# MVaaC — Markview as a Component: Contract

This is the **historical initial demo contract**. The current reusable viewer/editor
API, package organization and migration are documented in [MVaaC components](mvaac.md)
and [source navigation/TOC](mvaac-source-api.md). The exclusions and frozen API below
record the initial demo and do not limit the subsequent iteration.

## What the work delivers

1. A **resumable progressive layout API in `markview-core`**, so a front end can
   suspend a layout between blocks and resume it without re-scanning what it
   already laid out.
2. `markview-web`, the `wasm-bindgen` front end, rebuilt on that API.
3. A **pnpm monorepo under `web/`**: a reusable TypeScript package
   (`@markview/web`) and a very thin demo app, both bundled with **esbuild**.
4. A Playwright suite that drives the built demo and passes.

Out of scope, exactly as before: tabs, settings UI, font management, file IO,
PDF export, search, outlines. Configuration is injected from JavaScript.

## Repository layout

```
crates/markview-core/            Rust: the engine. Owns ProgressiveLayout.
crates/markview-web/             Rust: wasm-bindgen front end.
scripts/build-web.sh             cargo + wasm-bindgen -> the package's wasm/
web/                             pnpm monorepo root (private)
  package.json                   private, "workspaces" via pnpm-workspace.yaml
  pnpm-workspace.yaml
  tsconfig.base.json
  packages/markview/             @markview/web — the reusable component
    src/index.ts                 public surface, re-exports
    src/markview.ts              Markview class
    src/layout-update.ts         LayoutUpdate (progressive layout handle)
    src/reader.ts                CanvasReader convenience (loop + input)
    src/types.ts                 MarkviewOptions, MarkviewStats, ...
    wasm/                        wasm-bindgen output (generated, gitignored)
    dist/                        esbuild + tsc output (generated, gitignored)
  apps/demo/                     @markview/demo — private, thin
    index.html
    src/main.ts
    src/style.css
    dist/                        generated, gitignored
  dist/                          the built demo site (generated, gitignored)
  serve.mjs                      static server for web/dist
  tests/                         Playwright
  README.md
```

`web/dist/` is a **self-contained static site**: `index.html`, `main.js`,
`main.css`, and the wasm binary, with no runtime bare-specifier resolution and
no CDN. `web/serve.mjs` serves exactly that directory.

## Core: the resumable layout API (frozen)

`crates/markview-core/src/layout/progressive.rs`, re-exported from
`markview_core::layout::ProgressiveLayout`.

```rust
pub struct ProgressiveLayout { /* opaque */ }

impl ProgressiveLayout {
    /// The blocks laid out so far, as a snapshot a renderer can draw.
    pub fn snapshot(&self) -> &LayoutSnapshot;
    /// Whether every block has been laid out.
    pub fn is_complete(&self) -> bool;
    /// How many blocks are laid out so far.
    pub fn blocks(&self) -> usize;
    /// The finished snapshot and the document it came from.
    pub fn into_snapshot(self) -> LayoutSnapshot;
}

impl LayoutEngine {
    /// Starts a layout pass on `document` without laying out any block.
    pub fn begin_layout(
        &mut self,
        document: &Document,
        options: &LayoutOptions,
        images: &ImageSnapshot,
    ) -> ProgressiveLayout;

    /// Lays out blocks until `budget` is spent, and returns whether the
    /// document is complete. A zero budget still lays out exactly one block, so
    /// a caller can never spin without progress.
    pub fn advance(
        &mut self,
        layout: &mut ProgressiveLayout,
        document: &Document,
        budget: Duration,
    ) -> bool;
}

impl ProgressiveLayout {
    /// The pass's own id; every pass an engine begins gets its own.
    pub fn pass_id(&self) -> u64;
}
```

Rules the implementation must hold:

* A pass visits each block **at most once**. Nothing already laid out is
  re-measured or re-scanned on a later `advance`, so the cost of a step does
  **not** grow with the already-laid-out prefix.
* `budget` is honoured within one block: a step stops as soon as the elapsed
  time reaches `budget`, having completed the block in flight.
* `layout_progressive` keeps its current signature and semantics (the closure
  visits every prefix before the block that follows it, and a `false` return
  cancels), and is implemented **on top of** the session so there is one block
  loop, not two.
* `layout` / `layout_with_images` behave exactly as before.
* The block cache keeps one pass's worth of geometry: entries not touched by
  the completing pass are dropped when the pass completes.
* A pass carries the stylesheet, fonts and math limits it began with and
  re-applies none of them per block, so **only the newest pass may be
  advanced**. Resuming an older one would measure the rest of its document
  against whatever configuration replaced it, so `advance` asserts the pass is
  the engine's current one, exactly as it asserts the document identity.

## `markview-web`: the wasm surface (frozen)

Unchanged from the previous contract except for the progressive path and one
added `Stats` field:

| Method | Meaning |
| --- | --- |
| `create(canvas, configJson?) -> Promise<Markview>` | as before |
| `setMarkdown(text) -> string` | parse and lay out **completely**, publish |
| `beginUpdate(text) -> string` | parse and start a **resumable** layout |
| `stepUpdate(budgetMs) -> string` | lay out for at most `budgetMs`; strictly bounded |
| `updatePending() -> boolean` | true until the started layout completes |
| `finishUpdate() -> string` | complete the layout synchronously |
| `frame()`, `resize`, `setScroll`, `scrollBy`, `scroll`, `maxScroll`, `contentHeight`, `pointerDown/Move/Up`, `selectAll`, `clearSelection`, `selectedText`, `stats`, `adapter`, `setConfig` | as before |

`Stats` keeps its 12 keys and gains `"pending": boolean` — whether a started
layout still has blocks to lay out — and `"width": number` — the reading column
actually laid out, which narrows to fit the canvas. Every `Stats`-returning
method returns a JSON string. `resize` returns a boolean: whether the column
narrowed and a reflow began.

`width` is a **cap**, not a fixed geometry: the column is
`min(width, canvasWidth - 40)`. The engine reflows when that value changes, as a
resumable pass, so a window drag never blocks a frame.

Non-negotiable, carried over: real values for non-finite arguments, never a
panic across the FFI boundary, `readback` off, no threads.

## `@markview/web`: the TypeScript package (frozen)

### Types

```ts
export interface MarkviewOptions {
  width?: number;            // layout column width, logical px (760)
  fontSize?: number;         // (18)
  theme?: "light" | "dark";  // ("light")
  justify?: boolean;         // (true)
  hyphenate?: boolean;       // (true)
  paragraphIndent?: number;  // (0)
  greedy?: boolean;          // (false)
  hideFrontMatter?: boolean; // (false)
  frontMatterLabel?: string; // ("Metadata")
}

export interface MarkviewStats {
  revision: number; blocks: number; contentHeight: number; reused: number;
  parseMs: number; layoutMs: number; frameMs: number; frames: number;
  glyphs: number; backend: string; adapter: string; selectionLength: number;
  pending: boolean;
}
```

Unknown option keys are ignored; a malformed value rejects with a JS error.
`selectionLength` counts UTF-16 code units, matching `selectedText().length`,
not Unicode scalar values. It is cached until the selection actually changes,
so pointer movement that leaves the selection where it was reuses it.

### `Markview`

```ts
export class Markview {
  static create(canvas: HTMLCanvasElement, options?: MarkviewOptions): Promise<Markview>;
  setMarkdown(markdown: string): MarkviewStats;
  beginLayout(markdown: string): LayoutUpdate;
  stepPending(budgetMs?: number): boolean;
  frame(): MarkviewStats;
  resize(cssWidth: number, cssHeight: number, dpr?: number): boolean;
  setScroll(y: number): void;
  scrollBy(dy: number): void;
  scroll(): number;
  maxScroll(): number;
  contentHeight(): number;
  pointerDown(x: number, y: number, modifiers?: Partial<Modifiers>): void;
  pointerMove(x: number, y: number): void;
  pointerUp(x: number, y: number): PointerAction | null;
  selectAll(): void;
  clearSelection(): void;
  selectedText(): string;
  /** Writes `selectedText()` to the clipboard. Resolves false when empty. */
  copy(): Promise<boolean>;
  stats(): MarkviewStats;
  adapter(): string;
  setOptions(options: MarkviewOptions): void;
  /** Releases the wasm handle. Later calls throw. */
  destroy(): void;
}
export interface Modifiers { shift: boolean; control: boolean; alt: boolean; meta: boolean }
```

### `LayoutUpdate` — the progressive layout API

```ts
export class LayoutUpdate {
  /** Blocks this layout published so far; zero before the first step. */
  readonly blocks: number;
  /** Whether the whole document is laid out. */
  readonly done: boolean;
  /** Whether a later `beginLayout` replaced this pass. */
  readonly stale: boolean;
  /** Lays out for at most `budgetMs` (default 8). Returns `done`. */
  step(budgetMs?: number): boolean;
  /** Completes the layout synchronously. */
  finish(): void;
}
```

A handle keeps one pending pass, so a later `beginLayout` supersedes an earlier
`LayoutUpdate`. A superseded update reports `stale`, reports `done`, and neither
steps nor finishes: it can never drive the document that replaced it. `blocks`
counts what *this* layout published, not whatever the handle last published for
a previous document.

`step()` is the resumable entry point: calling it never re-lays out what an
earlier call already laid out, so a caller can drive it from
`requestAnimationFrame` and render `frame()` after each step. `finish()` is
the same layout run to completion.

### `CanvasReader` — the convenience that keeps the demo thin

```ts
export interface CanvasReaderOptions {
  markdown?: string;
  markview?: MarkviewOptions;
  /** Layout budget per animation frame (8). */
  stepBudgetMs?: number;
  onStats?: (stats: MarkviewStats) => void;
  onError?: (error: unknown) => void;
}

export class CanvasReader {
  static attach(canvas: HTMLCanvasElement, options?: CanvasReaderOptions): Promise<CanvasReader>;
  /** Replaces the document and starts a progressive layout. */
  setMarkdown(markdown: string): void;
  readonly markview: Markview;
  destroy(): void;
}
```

`CanvasReader` owns the `requestAnimationFrame` loop (advance a pending layout
by `stepBudgetMs`, then present a frame), the device-pixel-ratio sizing, the
wheel/pointer/keyboard bindings, and `Ctrl`/`Cmd`+`C` copying. This is what
makes the demo a wiring file rather than an application.

`attach` makes the canvas focusable when the caller has not, and takes that
back on `destroy`; a canvas is not focusable by default, so the keyboard
bindings would otherwise never fire. A caller's own `tabindex` is left alone.

A canvas the page sizes with its `width`/`height` attributes has its measured
CSS box pinned while the reader owns the backing store, because those very
attributes would otherwise let each write resize the element that measured it.
`destroy` puts the original attributes back before it takes the pin off, so a
teardown leaves the canvas the size it was found at and repeated attach cycles
do not grow it.

A `LayoutUpdate` owns the pass it started. A later `beginLayout`, a full
`setMarkdown`/`setOptions`, or a `resize` that reflows supersedes it, and a
superseded update reports `stale` and is inert. `stepPending` advances a pass
the handle started on its own — today, the reflow a narrower column triggers —
which no `LayoutUpdate` owns.

### Entry points

```ts
import init, { Markview, CanvasReader } from "@markview/web";
await init({ fonts: ["/fonts/NotoSerif-Regular.otf"] });
await init({ wasmUrl: "/assets/markview_web_bg.wasm", fonts });
```

The binary ships as `markview_web_bg.wasm` beside the package's JavaScript, and
`init()` resolves that plain name against `import.meta.url`. It is deliberately
**not** imported: an imported asset is only an asset to the bundler that
resolves it, so a published entry point that already holds a rewritten filename
would make every downstream bundler emit JavaScript alone and leave the binary
behind. A sibling reference survives bundling, and a deployment that moves the
JavaScript away from the binary passes `wasmUrl` instead. The name carries no
content hash, because the package has to be able to find it.

Text fonts come from the host through `InitOptions.fonts`, a readonly array of
`FontSource` (`string | URL | ArrayBuffer | Uint8Array`). URL sources are fetched
in parallel with wasm initialization; byte sources are copied into wasm with
their view bounds preserved. OpenType, TrueType and collections are accepted;
WOFF/WOFF2 and CSS fonts are not. Only KaTeX's math fonts remain embedded.
Missing or invalid font files reject initialization and allow a corrected retry.
Repeated calls share the first successful initialization's options and fonts;
omitting `fonts` leaves the text collection empty. Paragraph metrics, including
math placement, still need a host text face. The demo imports its pinned
faces through esbuild's `file` loaders, which emit hashed assets under `assets/`.

## Viewport (frozen)

The reading column is the configured width, narrowed until 20 logical pixels of
margin survive on both sides of the canvas. A narrower canvas therefore
**reflows** the document rather than clipping it: text that runs off the side
cannot be reached, because the reading view never pans horizontally.

Scrolling is a requested offset. While a pass is still publishing, the document
on screen is temporarily shorter, and the request is held rather than clamped
away; what the renderer draws and what `scroll()` reports is the request clamped
to what exists so far. A replacement pass also leaves the snapshot the reader is
reading on screen until its own prefix has grown back past the top of the
current view, so a debounced edit never jumps the reader to the top of a
document they were 15 000 px into. `stats().width` is the column actually used.

The backing store is capped by the device's texture edge. When the requested
device ratio would push an edge past it, the ratio itself is reduced instead of
truncating an edge, so the renderer, hit testing and the scroll range keep
describing the same logical viewport. Initial sizing happens only after the
device that supplies that edge exists.

## Publishing (frozen)

A pass over a document already published only ever appends blocks, so a
budgeted step appends the blocks it added instead of copying the prefix that is
already on screen. Otherwise every step would cost the whole published prefix,
which reintroduces exactly the prefix-dependent work the resumable layout
exists to remove. The selection and a drag in flight keep their reading
positions across such a step; they are re-anchored only when the whole snapshot
is replaced by a different document.

A new pass over the same document keeps the displayed snapshot until its
prefix covers both the selection and any held multi-click drag base. Replacing
it earlier would clear positions whose blocks have not been laid out yet.

## Build (frozen)

```sh
scripts/build-web.sh      # cargo build --target wasm32 + wasm-bindgen -> web/packages/markview/wasm/
pnpm --dir web build      # esbuild + tsc: package dist and the demo site in web/dist/
pnpm --dir web test       # Playwright
node web/serve.mjs        # serves web/dist on http://127.0.0.1:4173
```

* **esbuild** is the bundler, driven by a `build.mjs` per package/app or a
  single root script. TypeScript is checked with `tsc --noEmit`, and the
  package's `dist/` carries `.d.ts` from `tsc --emitDeclarationOnly`.
* The wasm binary reaches the demo as an **esbuild file asset** (`loader:
  { ".wasm": "file" }`), so the emitted site references a plain URL and needs
  no path tricks at runtime.
* `web/packages/markview/wasm/`, `web/packages/markview/dist/`,
  `web/apps/demo/dist/` and `web/dist/` are generated and gitignored.
* No framework, no CDN, no runtime bare-specifier resolution.

## Demo contract (`apps/demo`)

* `index.html` has `<textarea id="source">` and `<canvas id="view" tabindex="0">`.
* `src/main.ts` reads `globalThis.MV_CONFIG`, attaches a `CanvasReader`, and
  wires the textarea's `input` (debounced at most 120 ms) to `reader.setMarkdown`.
* Only after the reader's first presented frame does the page set
  `window.__markviewReady = true`. On failure it sets `window.__markviewError`
  and shows a banner, and never sets ready.
* The page exposes `window.mv` — the `Markview` **component class** from
  `@markview/web`, never the raw wasm-bindgen handle — plus `window.mvStats()`
  (the parsed `Stats`) and `window.mvReader` (the `CanvasReader`). The tests
  drive the same public surface a host application would, so an escape hatch to
  the wasm handle is not part of the package's surface.
* The visual design of the current demo page is kept; only the wiring moves to
  TypeScript. `style.css` is carried over, not redesigned.

## Playwright acceptance (`web/tests`)

Run with `pnpm --dir web test` against `web/dist`. The harness must:

1. Serve `web/dist` on the fixed port and load `index.html`.
2. Wait for `window.__markviewReady`, failing loudly with captured console and
   page errors **and** `window.__markviewError` — never skip.
3. Assert `window.mvStats().backend === "Gl"` and no page errors.
4. Assert the initial document has `blocks > 0` and `contentHeight > 0`.
5. Type into `#source` and assert the canvas re-renders.
6. Assert `glyphs > 0` and `frames > 0`.
7. Screenshot the canvas and assert real ink, decoding the PNG with `node:zlib`
   only; print the count.
8. Drag-select on the canvas, assert `selectedText()` is a substring of the
   source, then assert `Ctrl+C` puts exactly that text on the clipboard.
9. Assert an edit changes `contentHeight` and reports `reused > 0`.
10. Assert one `LayoutUpdate.step(8)` — from `Markview.beginLayout()` — on a
    replacement with the **same block count as the published document** leaves
    `LayoutUpdate.done` false and does not publish the whole document.
11. Assert non-finite arguments (`LayoutUpdate.step(Infinity)`, `setScroll(NaN)`,
    `scrollBy(NaN)`, `resize(Infinity, Infinity, 2)`) do not poison the handle or
    break the page.
12. Assert the **resumable** property directly: on a many-hundred-block document,
    drive `beginLayout()` + repeated `LayoutUpdate.step(8)` and assert that
    (a) `blocks` increases monotonically and never exceeds the total,
    (b) no step publishes a block it already published (`blocks` strictly
    increases or the layout is done), and
    (c) the wall time of the last step is not materially larger than the first —
    a re-scanning implementation charges the whole prefix into every step, so
    its later steps grow with the document.
13. Assert syntax colors land on the pass that first lays the code out: laying
    out the same document with a fenced block twice must paint identical pixels.
    A block measured before its colors arrived renders uncolored, so a
    difference means the first pass missed them and edited code is one update
    behind.
14. Assert the reader lifecycle: after `destroy()` a pointer event on the
    released canvas must not reach the freed handle (no page errors); a
    superseded `LayoutUpdate` must report `stale`/`done` and must not step or
    finish the pass that replaced it; and `destroy()` must still free the handle
    after the frame loop stopped on an error.
15. Assert the whole document is reachable by scrolling:
    `maxScroll() + (canvasCssHeight - 2 * INSET) >= contentHeight()`. The clip
    region is the canvas box minus both insets, so subtracting the whole canvas
    leaves the last `2 * INSET` logical pixels unreachable.
16. Assert a multi-click (word-grain) drag still yields reading text after a
    publication lands mid-gesture. Its base must be carried onto the new
    revision like the selection; a stale base builds a selection that
    `extract_text` rejects, so the drag would return `""`.
17. Assert a narrower canvas reflows: after shrinking the canvas to 420 CSS px,
    `stats().width` is at most `420 - 2 * MARGIN` and `contentHeight` has grown
    (the text re-wrapped rather than being clipped).
18. Assert a replacement pass keeps the reader's position: from a large scroll
    offset, `beginLayout(sameText)` + `step(0)` + `finish()` leaves `scroll()`
    where it was. Losing it means the one-block prefix was allowed to clamp the
    requested offset.
19. Assert an attached canvas becomes focusable: `attach()` gives an ordinary
    canvas a `tabindex`, `focus()` then lands, `destroy()` removes what it
    added, and a caller-provided `tabindex` survives both.
20. The demo build consumes the package's **built** `dist/index.js`, never its
    sources, so every build already bundles the published entry point. A
    downstream bundler must emit the JavaScript alone and let `init()` find
    `markview_web_bg.wasm` beside it with no arguments; the demo serving from
    `dist/` is the standing proof.
21. Assert a resize that reflows supersedes the layout handle: after a
    `beginLayout()` step, a `resize()` that narrows the column must make the
    handle report `stale`/`done`, and its `step()`/`finish()` must not drive the
    replacement reflow, which `stepPending()` then carries to completion.
22. Assert a layout handle reports its own progress: from a scroll offset past a
    replacement's one-block prefix, `step(0)` leaves `blocks` at zero while the
    previous snapshot's count is still published, and `finish()` then reports the
    whole document.
23. Assert `Stats.selectionLength` tracks the selection: it matches
    `selectedText().length` after `selectAll()` and drops to zero after
    `clearSelection()`. The not-re-extracted-per-read guarantee is a native
    test in `crates/markview-web`, so the browser asserts only the numbers.
24. Assert a DPR-2 canvas wider than the device's texture edge still creates and
    resizes: the backing store never exceeds the edge, keeps the canvas's aspect
    ratio, and the reflow completes without page errors.
25. Assert a wheel event during a pending replacement does not discard the held
    scroll request: after `finish()`, `scroll()` is where the request plus the
    wheel put it, not clamped to the temporary prefix height.
26. Assert a cancelled pass keeps the warm block cache: a full layout after a
    pass cancelled before its first block, and after one cancelled post-step,
    still reports every block reused.
27. Assert `selectionLength` counts UTF-16 code units: for a selection holding
    supplementary-plane characters it equals `selectedText().length`, which is
    larger than the selection's code point count.
28. Assert an attribute-sized canvas keeps its logical box at DPR 2: a bare
    canvas appended with no CSS keeps the same bounding rectangle across frames
    instead of growing with its backing store, the backing store still scales by
    the ratio, and `destroy()` gives the CSS size back without resizing the
    element.
29. Assert a held drag follows a wheel scroll: with the pointer stationary, a
    scroll must re-hit-test it so `selectedText()` extends past its pre-scroll
    range, and releasing keeps that range.
30. Assert repeated cancellations keep the warm block cache: after a
    partially-resumed pass is abandoned and a further pass is cancelled before
    touching anything, a full layout still reports every block reused.
31. Assert the legacy clipboard fallback restores focus: with the async
    clipboard forced to reject, `copy()` leaves `document.activeElement` on the
    canvas so the keyboard bindings keep working.

## Appendix: verified environment facts

* Headless Chromium always finds a WebGL2 context, but **which device backs it
  depends on the launch flags**, not on the machine's capability. With no flags
  it falls back to SwiftShader (CPU). Pointing ANGLE at the platform driver —
  `--use-angle=vulkan --enable-features=Vulkan` — or running headful reaches the
  real device; on this machine that is an Intel Arc (MTL) through Mesa. The
  default project therefore renders on SwiftShader, and
  `MV_GPU=1 pnpm --dir web test` runs the same suite on the real GPU.
  `--enable-unsafe-swiftshader` is a harmless net for the default project.
* `page.locator(canvas).screenshot()` **does** capture WebGL content;
  `canvas.toDataURL()` does **not** (no `preserveDrawingBuffer`).
* The stdlib-only PNG decoder lives in `web/tests/png.mjs`.
* The `wasm-bindgen` CLI 0.2.129 is vendored at
  `.tools/wasm-bindgen-0.2.129/wasm-bindgen`; the crate pins `=0.2.129`.
* The demo emits 16 committed subset faces as host assets; `scripts/check_web_font_coverage.py`
  guards the demo document against tofu. It unions every face, so a style gap
  can still slip through.

## Non-negotiables

* English comments; comments in Markdown syntax (`// \`x\` is borrowed`).
* No new `unsafe` in Rust; no `any` in the package's public types.
* `web/dist/`, `web/*/dist/`, `web/packages/markview/wasm/` and
  `web/node_modules/` are generated: never edit, never commit.
* The native workspace must still build, test and lint.

## Shared document interaction

`markview-selection` owns document hit testing, hover/cursor decisions,
scroll animation and deferred destinations, overflow scrollbar geometry,
selection edge scrolling and gesture release inertia. Desktop chrome and
browser event conversion remain in their hosts. Windows fractional line-wheel
input uses the same eased path as whole detents; the previous packet momentum
algorithm has been removed.

The existing immediate `setScroll` / `scrollBy` API remains compatible.
Direct relative input cancels internal animation before measuring travel from
the displayed offset, while retaining deferred non-animated requests.
An absolute request beyond a prefix remains held and is displayed clamped to
each growing prefix. `scrollToEnd()` queues the final end while layout is
pending; the reader's End key uses it. Subsequent deliberate scrolling or Home
replaces that destination.
The low-level wasm surface adds `scrollInput`, `setScrollMode`, `cursor`,
`pointerLeave`, `cancelPointer` and `setImagesClickable`. `pointerUp` now returns
a JSON activation or `"null"`; the TypeScript facade parses it:

```ts
export type ScrollMode = "external" | "internal";
export type DocumentCursor = "default" | "text" | "pointer";
export type PointerAction = { kind: "document"; reflowed: boolean }
  | { kind: "link" | "image"; target: string; modifiers: Modifiers };

// Additional Markview methods:
scrollToEnd(): void;
setScrollMode(mode: ScrollMode): void;
scrollInput(dx: number, dy: number, kind: "external" | "step"): void;
cursor(): DocumentCursor;
pointerLeave(): void;
cancelPointer(): void;
setImagesClickable(clickable: boolean): void;
pointerUp(x: number, y: number): PointerAction | null;

// Additional CanvasReaderOptions:
scrollMode?: ScrollMode;
onLink?: (target: string, modifiers: Modifiers) => void;
onImage?: (target: string, modifiers: Modifiers) => void;
```

`external` motion applies the host's travel directly and cancels internal
animation. `internal` motion eases wheel input; gesture hosts can use the shared
`Motion` machinery for release inertia. `scrollInput(..., "external")` always
applies external motion; `"step"` follows the selected mode. Inputs are CSS
pixels, positive right/down. The browser converts line and page wheel units
before passing them in. `internal` is the default. Hosts can explicitly choose `external` when
motion is already maintained outside Markview; the demo offers a live mode
switch for comparison. No browser device detection is inferred from fractional deltas.

Hover follows pointer motion, scrolling and layout publication. Pointer exit
clears idle hover; cancellation ends a gesture without activation or erasing
its selected text. Only a click released over the original target activates;
dragging, Shift selection and cancellation suppress activation. Images become
clickable only when the host enables them (`CanvasReader` does so when
`onImage` exists). The reader restores the canvas's original inline cursor on
teardown. The renderer receives hover and wide-block horizontal offsets.

Details summaries toggle their layout state through progressive reflow and
supersede existing `LayoutUpdate` handles. Resize and option changes preserve
expansion; replacing Markdown resets it. Current-document fragment links,
including percent-encoded anchors and footnotes, are handled internally.
An anchor inside closed details expands its enclosing disclosures first.
Unpublished targets wait for layout; deliberate scroll input cancels a waiting
jump; completion without a target ends the wait. External and cross-document
links are emitted through `onLink` with no default browser navigation. Images
use `onImage`; no image viewer is included.

## Asynchronous image resources

`Markview.create(canvas, options?, resources?)` accepts `ResourceOptions`,
independent of typography configuration. `CanvasReaderOptions.resources`
forwards the same configuration. Neither entry point fetches images by default.
The public package exports these types and helpers:

```ts
interface ResourceOptions {
  onResources?: (events: readonly ImageResourceEvent[]) => void;
  onError?: (error: unknown) => void;
}
interface ImagePixels {
  width: number;
  height: number;
  rgba: Uint8Array; // Straight-alpha sRGB RGBA8.
}
interface ImagePriority {
  region: "visible" | "near" | "offscreen" | "unknown";
  distance: number | null; // CSS px; null means position is unknown.
}
interface ImageRequest {
  readonly id: string;
  readonly src: string;
  readonly signal: AbortSignal;
  readonly priority: ImagePriority;
  resolve(pixels: ImagePixels): void;
  reject(message: string): void;
}
type ImageResourceEvent =
  | { kind: "request"; request: ImageRequest }
  | { kind: "priority"; request: ImageRequest };

function decodeImage(
  source: Blob | ArrayBuffer | Uint8Array,
  signal?: AbortSignal,
): Promise<ImagePixels>;
function loadImageUrl(
  request: ImageRequest,
  options?: { baseUrl?: string | URL; requestInit?: RequestInit },
): Promise<void>;
```

Each document replacement emits its full set of Markdown/HTML image sources,
including closed details, in a microtask. Identical original `src` strings are
deduplicated in document order. Request IDs identify the component, document
and source; they are independent of layout passes. The host owns queuing,
throttling, concurrency, retry during a pending request, and caching. It may
retain a request and resolve or reject it later. Returning a Promise from the
callback does not complete a request; asynchronous host code must handle its
own failures and call `reject`.

Requests start with `unknown` priority. Each presented frame reports changed
priorities for outstanding requests through `priority` events, using the same
request object whose getter exposes its current priority. Laid-out occurrences
intersecting the viewport are `visible`; occurrences within one viewport height
vertically are `near`; others are `offscreen`. Distance is the shortest vertical
distance to the viewport, zero for vertical overlap. The nearest occurrence
wins, preferring a visible occurrence. Unpublished or closed content remains
`unknown`. Geometry comes from the newest layout, never a retired document.
Visibility uses the renderer's per-command horizontal offsets and overflow
clipping, so panning a wide table updates image priorities without a reflow.
Priority is a scheduling hint; the component never delays requests on its basis.

`resolve` copies the RGBA view immediately. Positive integer dimensions, exact
RGBA length and the renderer's texture edge are validated; invalid images
become error placeholders. The first completion wins. Results are queued until
`frame`, `stepPending`, or an active `LayoutUpdate.step/finish` submits the batch
and starts one progressive reflow. That reflow supersedes existing layout
handles; drive it with `stepPending`, or let `CanvasReader` do so. `setMarkdown`
and `LayoutUpdate.finish` finish only layout, never wait for image requests.
`stats.pending` continues to describe layout work only.

Resize and typography changes retain requests, image results and details
expansion. Resource reflow retains the newest text, scroll request, and
rebasable selection. Document replacement and destruction abort requests and
ignore late completions. Pixel snapshots remain alive while the old displayed
snapshot still needs them. There is no automatic retry or cross-document cache;
setting Markdown again creates fresh requests, which a host cache can answer.
Without a callback images remain placeholders.

A synchronous callback exception rejects outstanding new requests in that
batch and reports through `resources.onError`, falling back to
`CanvasReaderOptions.onError` or `console.error`. Priority callback exceptions
are reported without stopping the frame loop or failing image requests.

`loadImageUrl` is an opt-in helper, not a component setting. It fetches HTTP(S),
Blob and `data:image/` sources, resolves relative URLs against `baseUrl` or
`document.baseURI`, obeys browser CORS and the supplied fetch options, decodes a
static frame, and calls `resolve/reject`. `decodeImage` preserves typed-array
view bounds, infers `image/svg+xml` for SVG bytes or untyped Blobs, obeys
cancellation and releases temporary browser objects. It
returns pixels without completing a request, so hosts can compose it with
custom storage or authentication. Browser decoding determines supported image
formats; no Rust decoder or worker thread is added.

Text fonts retain the initialization-only `InitOptions.fonts` contract.
Mermaid is an internal rendering computation, not an external image request;
its existing desktop renderer will be integrated with MVaaC separately.
