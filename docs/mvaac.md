# Markview Web components

MVaaC is the framework-independent Web/WASM interface to Markview. The native
application remains a read-only Markdown reader; Web editing belongs to
`@markview/editor`. This iteration is still in progress: instance font sets,
WOFF codecs, the final font helper and SVG completion are the next stages.

## Packages and ownership

| Package | Responsibility |
| --- | --- |
| `@markview/viewer` | Engine, canvas input/frame loop, source geometry, reading events and TOC navigation |
| `@markview/editor` | CodeMirror Markdown editing, split layout and automatic bidirectional source following |
| `@markview/resources` | Explicit browser image transport/decoding, base URL and request configuration |
| `@markview/web` | Deprecated compatibility re-export of viewer and legacy image helpers |

The viewer does not depend on CodeMirror or the resource helper. The resource
helper performs no work until explicitly passed to a viewer. Hosts may supply
`resources.onResources` instead and own transport, caching and concurrency.

## Quick start from built packages

Build the packages as described below before importing workspace entries. Give
the mount container a height; text fonts are explicit host assets, never bundled
in the viewer. The current compatibility font initialization accepts OTF/TTF/TTC
URLs or bytes, and only KaTeX fonts are embedded.

```ts
import { Editor } from "@markview/editor";
import { browserResources } from "@markview/resources";
import wasmUrl from "@markview/viewer/wasm?url"; // Vite asset import

const editor = await Editor.mount(document.getElementById("editor")!, {
  markdown: "# Hello\n\nStart writing here.",
  viewer: {
    initialization: { wasmUrl, fonts: ["/fonts/body.otf", "/fonts/mono.ttf"] },
    resources: browserResources({ baseUrl: new URL("/documents/", location.href) }),
  },
  onChange: ({ markdown, documentVersion }) => console.log(documentVersion, markdown),
});
// Call when the host removes the component.
editor.destroy();
```

For an independent preview, replace the editor import/mount with
`Viewer`/`Viewer.mount` from `@markview/viewer`, and pass the nested `viewer`
options directly. No CodeMirror or frontend framework is involved.

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

Following uses the source position at the top visible line and its fractional
vertical displacement. It follows within long paragraphs/code, not by whole
scroll percentage. Input in either pane takes ownership, cancels pending motion
from the other, and prevents programmatic follow events feeding back. Following
never focuses the other pane or changes its selection. Editor changes map the
previous reading reference through CodeMirror changes. See the
[source, version and TOC reference](mvaac-source-api.md) for pending geometry,
reflow preservation, atomic content and collapsed-content behavior.

The default TOC uses the complete parsed heading list. Hosts can read
`editor.viewer.outline()` and own their own UI, or disable the component's panel.
Heading navigation opens containing disclosures; ordinary source following keeps
them collapsed. New document versions retire prior targets and resource requests.

## Lifecycle and examples

Every mount creates owned DOM and engine state. Multiple instances are supported;
call `destroy()` before removing/replacing an instance. Destruction is idempotent
and stops listeners, frame loops and resources. No filesystem, file manager,
PDF export, framework wrappers, VS Code plugin or Mermaid rendering is supplied
by these packages.

`apps/editor` consumes built public entries and demonstrates editing, both
scroll directions, TOC, themes and a draggable split. `/editor.html` is the
current example; `/index.html` preserves the original low-level reader regression
host. Font-helper composition will be added in the font stage.

## Build and test

```sh
pnpm --dir web install
scripts/build-web.sh
pnpm --dir web build
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web test:packages
pnpm --dir web serve
```

The WASM target and matching wasm-bindgen CLI are described in
[the historical build contract](mvaac-web-demo.md). `web/build.mjs` builds the
viewer, helpers, compatibility entry and editor in dependency order, emits type
declarations, copies WASM, then bundles examples using built entries. Tests
also install actual tarballs into an isolated consumer, typecheck with `skipLibCheck: false`, bundle without source aliases and initialize/mount/destroy in Chromium. The normal suite includes the original reader regressions, built viewer/source navigation, real
CodeMirror scrolling/editing, Chinese composition, resize, TOC and lifecycle.

## Migration

Use `@markview/viewer` for `init`, `Markview`, `LayoutUpdate`, `CanvasReader`,
`Viewer` and their types. Move `decodeImage`/`loadImageUrl` imports to
`@markview/resources`. `@markview/web` re-exports both for existing callers;
it is a compatibility package rather than a separate engine instance.
`packages/markview` is now the viewer's source directory; `packages/web` is the
compatibility directory. The old demo contract is historical and its exclusions
of editing/outlines do not constrain this iteration. Public source/navigation
API lives in this guide and `mvaac-source-api.md`.
