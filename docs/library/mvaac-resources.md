# Asynchronous image resources

This page defines the current low-level image resource protocol. See
[Web components](mvaac.md#images-and-host-transport) for helper usage and host transport.

`Markview.create(canvas, options?, resources?)` accepts `ResourceOptions`,
independent of typography configuration. `CanvasReaderOptions.resources`
forwards the same configuration. Neither entry point fetches images by default.
`@markview/viewer` exports the resource types; `@markview/resources` exports
the decoding and transport helpers:

Use the exported TypeScript types for exact signatures:
[resource types](../../web/packages/markview/src/resources.ts) and
[transport helpers](../../web/packages/resources/src).
An `ImageRequest` supplies an ID, source string, abort signal, current priority
and `resolve`/`reject` completion methods. Pixel data is straight-alpha sRGB RGBA8;
priority distances use CSS pixels. The event discriminant is `request` or `priority`.

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
