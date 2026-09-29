# VS Code plugin requirements (Path B)

Status: draft, awaiting approval. This document states what must hold. How each
requirement is verified is the reviewer's call; see [Review gate](#review-gate).

## Architecture

An extension activates a long-lived, windowless Markview process and displays
its output in a VS Code webview. The native engine keeps every typography
decision; the webview is a display surface. The WebView exception in
[AGENTS.md](../AGENTS.md) governs this mode.

```text
VS Code window
├── extension host ── stdio ──> markview serve --headless
│                                    │ layout + offscreen render
│                                    ▼
└── webview  <──── tiles, block map, text layer ────┘
```

Requests and ordinary responses use JSON lines. A tile response is a JSON header
with `encoding: "png"` and `bytes`, immediately followed by exactly that many
PNG bytes, with no trailing delimiter. The shared client handles arbitrary pipe
chunk boundaries and forwards a `Uint8Array`; the webview decodes a Blob URL and
revokes it after decoding. Preview tiles never use base64. A configured engine
using the old tile protocol is refused with an update instruction.

The plugin instance is private: it never discovers, attaches to, or shares
state with a Markview the user launched.

## INV — invariants

Violating any of these is a defect regardless of behavior.

| ID | Requirement |
|:--|:--|
| INV-1 | Line breaking, microtypography, shaping, math, image handling, and stylesheet resolution happen in `markview-core` only. No second layout implementation may exist in the extension. |
| INV-2 | Text reaches the screen as pixels the engine rasterized. The webview text layer is a transparent overlay for selection and source mapping, and never an input to measurement or positioning. |
| INV-3 | The editor owns the document text. The server exposes no method that mutates document text. |
| INV-4 | Plugin mode reads and writes no existing Markview user state: not `settings.toml`, not the user stylesheet directory, not the user font directory. All state lives under the extension's storage. |
| INV-5 | The extension works when Markview is not separately installed. |
| INV-6 | The webview owns no document, no layout, and no command surface. |

## ENG — engine side

| ID | Requirement |
|:--|:--|
| ENG-1 | `serve` runs a windowless server that needs no display and no `winit` event loop. |
| ENG-2 | A document's text can come from the client instead of the filesystem, and an unsaved buffer renders exactly as its bytes would from disk. |
| ENG-3 | A document keeps a path or base directory, so relative images and relative links resolve for a buffer that has never been saved. |
| ENG-4 | File observation is suspended while the client owns the text and restored when the client reports a save, so a save never reverts the preview to stale content. |
| ENG-5 | A tile request returns an encoded image for a given viewport rectangle at a given scroll offset, cropped and scaled as requested. |
| ENG-6 | The server publishes a whole-document block map `{id, source_start, source_end, y, height}`, growing as layout progresses. |
| ENG-7 | The server publishes a text layer over rendered regions, carrying per-cluster geometry, text, and source range. |
| ENG-8 | Two mappings exist and are separately correct: a rendered point maps to a byte offset, and a byte offset maps to a rendered vertical position. Neither is defined as the inverse of the other. |
| ENG-9 | Source ranges reach `TextCluster`, extending the block-level `PlacedBlock.source`, so a position inside a block resolves to a range inside that block. |
| ENG-10 | Appearance is set by the client. Plugin mode reads neither the host operating system's theme nor any user configuration. |
| ENG-11 | Configuration applies per document or view rather than per process, so two documents under different settings render differently from one server. |
| ENG-12 | One `--state-dir` relocates the font directory, the stylesheet directory, and the image cache together. |
| ENG-13 | The process terminates when stdin reaches end of file, so an extension host that crashes or is killed cannot orphan it. |
| ENG-14 | Export is triggerable over the protocol and produces the same output as the existing CLI path for the same input. |
| ENG-15 | Math errors, degradation, and deferred remote images are reported to the client. |
| ENG-16 | An export names its format and its template: the server renders PDF or PNG, layers a template the client names or the rules it holds, and lists the templates it can name. |

## EXT — extension side

| ID | Requirement |
|:--|:--|
| EXT-1 | The server starts lazily on first use and is reused across documents for the life of a VS Code window. |
| EXT-2 | A webview panel shows the document, opens from the command palette and a distinct icon fixed in the Markdown editor title toolbar, and can sit beside the source. An open preview follows the active Markdown document without stealing focus; non-Markdown editors leave it unchanged. |
| EXT-3 | Scrolling is virtualized: the scrollable extent matches the document height and only the visible band is fetched. |
| EXT-4 | Edits to an unsaved buffer update the preview, coalesced so a burst of keystrokes converges on the latest state. |
| EXT-5 | Scroll synchronization is bidirectional, moves the view without moving the caret, and does not oscillate between the two surfaces. |
| EXT-6 | Native drag selection, clipboard copy, and in-panel find work over the rendered content. |
| EXT-7 | Clicking in the preview reveals the corresponding source range in the editor. |
| EXT-8 | Every setting comes from VS Code configuration, resolved per document, including workspace-folder and language-scoped layers. Preview and default export share `markview.template`, falling back to `markviewExport.template` only when the former is unset; an explicit empty value disables the fallback. |
| EXT-9 | Commands live in the command palette, menus, and user keybindings. The preview surface binds no keys of its own. |
| EXT-10 | Appearance follows the editor's color theme when no template is configured; an explicit template retains its own palette. |
| EXT-11 | Local Markdown links open in the editor and remote links open in the browser. The preview opens no tabs of its own. |
| EXT-12 | Export commands run and reveal the result. |
| EXT-13 | The reader exports the document as PDF or PNG under a template of their choosing: one of the engine's own, or a `.mvss.toml` of their own that never has to be installed. |

## NFR — non-functional

| ID | Requirement |
|:--|:--|
| NFR-1 | An edit reaches the visible preview fast enough to read as live. The measurement boundary is the keystroke to the updated pixels, including coalescing, transport, encoding, decoding, and paint. |
| NFR-2 | A screenful of preview costs few enough bytes to stay off the critical path. The boundary includes every message the client needs to paint that screenful. |
| NFR-3 | Typing does not move the reader's place in the document. |
| NFR-4 | No server process survives the VS Code session that owns it, under graceful quit and under a kill of the host. |
| NFR-5 | The cost of starting the engine is paid once per window, not once per document. |
| NFR-6 | Six targets ship: `win32-x64`, `win32-arm64`, `linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`. |
| NFR-7 | The engine's Linux runtime requirements are documented, including any system library and the oldest supported glibc. |

## EXC — excluded from v1

| ID | Exclusion |
|:--|:--|
| EXC-1 | `vscode.dev` and browser-based editors: no extension host and no native process. |
| EXC-2 | Remote-SSH and dev containers: rendering needs a GPU where the server runs. |
| EXC-3 | Editing in the preview. The editor remains the only editing surface. |
| EXC-4 | Window placement, grouping, and chrome: this architecture owns no window. |

## TST — verification surface

| ID | Requirement |
|:--|:--|
| TST-1 | Layout, parsing, and the protocol state machine are testable from text alone, with no display and no GPU. Anything that rasterizes or reads back a GPU texture is not part of that set. |
| TST-2 | Deterministic runs use offline mode and a fixed font set, matching the existing diagnostic modes. |
| TST-3 | A stylesheet change that only affects colors reports no reflow, and one that affects geometry reports a reflow. |
| TST-4 | Where a behavior is only observable in a running VS Code, it is verified by launching a real VS Code instance and driving the extension programmatically, not asserted from source alone. |

## Review gate

The gold standard is whether a requirement is fully satisfied, and the reviewer
decides what evidence establishes that. A requirement is done when a review by
`gpt-6-astra` at medium reasoning effort returns `verdict: "approve"`.

```sh
echo "<review prompt naming the requirement IDs>" | scripts/review-gate/review.sh
```

Exit status 0 approves, 1 rejects and prints findings, 2 means the review did not
usably run. A rejection returns the work to the implementer; an approval is the
only thing that closes a requirement.

Where a requirement is only observable in a running editor, the reviewer
launches a real VS Code instance and drives the extension programmatically.
Screen-driven review is not available here: `computer_use` is a Desktop-app
capability and is not exposed to `codex exec`, and `screencapture` fails for
want of the macOS Screen Recording grant (`could not create image from
display`). If that grant is given to the terminal later, screenshot review can
be added, but nothing in this plan may depend on it.

## Progress

The records below describe the original preview implementation. The port onto
main `9f61e25` plus the published export extension is in progress; affected
requirements need fresh tests and review before this branch is accepted.
The independently published export subset retains its history in
[export-plugin-requirements.md](export-plugin-requirements.md).


| Port scope | Result |
|:--|:--|
| EXT-5, NFR-4, TST-4 test-harness follow-up | **Approve, round 1**, 2026-09-29 (`review-host-cleanup.log`). Display-pixel scroll bounds, recorded Linux desktop handoffs and bounded process-group cleanup. Lifecycle tests and installed regressions pass; full-suite first-frame failure remains open. |
| INV-1..6, EXT-1/2/8/9/10/12/13, NFR-4, TST-1/2/4 consolidation | **Approve, round 2 of completed reviews**, 2026-09-29 (`review-unify-4.log`). Round 1 found following retained the dead engine after restart; the listener now uses the replacement session. The reviewer independently reran installed regressions. Final full/reload logs (`unify-recovery-full.log`, `unify-recovery-reload.log`) and 821 CPU / 13 GPU tests pass. Linux CI and NFR-1/NFR-6 closure remain separate. |
| EXT-1/2/4/8, TST-4 active-editor follow-up | **Approve, round 1**, 2026-09-29 (`review-follow.log`). One preview follows actual Markdown tab changes without stealing focus, with scoped settings and latest-request protection; non-Markdown and closed-panel behavior remain stable. Full suite (`follow-full-7.log`), installed regressions and actual reload pass. Creation focus and early-ready races were fixed; background-edit/link tests were aligned with following semantics. |
| EXT-2, EXT-8/10, INV-1..4, NFR-3/4, TST-4 reload/background follow-up | **Approve, round 1**, 2026-09-29 (`review-restore.log`). Serializer restores editor buffers, scroll and panel group without awaiting webview delivery; native tile metadata colors the outer spacing with the committed frame. Actual installed-window reload, installed full suite, 798 CPU and 13 GPU tests pass. |
| ENG-5, INV-2, EXT-2..4, NFR-2; TST-4 transport follow-up | **Approve, round 1**, 2026-09-29 (`review-binary.log`). Raw PNG frames and typed-array/Blob display replace base64. 798 workspace tests, 12 GPU tests, framing tests, final full-host and both installed packages pass; fixed PNG bytes match the baseline with 24.99% less wire data. The old string-identity assertion and export whitelist omission were fixed before review; intermittent resize evidence stays open. |
| TST-1, TST-2, TST-3, ENG-10 | **Approve, round 2**, 2026-09-27. Round 1 caught stale Mermaid images and missing diagram reflow; round 2 verified refreshed pixels, preserved palette geometry and published size changes. 794 default tests and 11 server GPU tests pass; clippy passes. |
| EXT-8, EXT-9 | **Approve, round 1**, 2026-09-27. `review-settings.log` verifies document-scoped configuration and host-owned commands against the completed installed-VSIX integration run. |
| NFR-7 | **Approve, round 1**, 2026-09-27. `review-platform.log` verifies Linux dependencies and the glibc 2.35 baseline on both architectures. NFR-6 remains open until all six packages build. |
| INV-1..6, ENG-1..16 | **Approve, round 2**, 2026-09-27. Round 1 found source maps lost when adjacent text merged and entity clusters colliding in the overlay. Round 2 independently probed the fixes and ran 40 CPU protocol tests; `review-engine-2.log` approves the port invariants and engine requirements. |
| EXT-10..13 | **Approve, round 2**, 2026-09-27. Round 1 found relative templates resolved at the filesystem root. Round 2 verifies document-relative paths, workspace fallback, theme/link behavior and export using `installed-preview-2.log`, GPU tests and native export parity. |
| EXT-1..5, NFR-2..5 | **Approve, round 2**, 2026-09-27. Round 1 found edits lost while initial open was awaiting the engine. Round 2 independently reproduces the fix and approves the completed installed-VSIX/lifecycle evidence in `review-preview-2.log`. |
| EXT-6, EXT-7, TST-4 | **Approve, round 3**, 2026-09-27. Round 1 rejected injected find matches; round 2 found duplicate overlay matches after selection and block-start navigation inside tall blocks. Round 3 approves Chromium find next/previous/wrap, row-level navigation and host clipboard tests in `installed-preview-4.log`. The workbench query field itself remains undriven. |

Requirements closed by an approving review, in order. A later change that
touches one of these reopens it.

| Requirement | Verdict | Notes |
|:--|:--|:--|
| ENG-1, ENG-2, ENG-13 | approve | `serve` + block map, 2026-09-21. ENG-2 needed two rounds: the first missed the image pipeline, the second the syntax-highlighting settle. |
| ENG-3, ENG-12 | approve | Link classification and `--state-dir`, 2026-09-21, first attempt. |
| ENG-5 | approve | Tile rendering, 2026-09-21. Three rounds: margins were re-inset per tile, parameters were clamped rather than refused, then a present-but-wrongly-typed parameter still fell back to a default. |
| ENG-6 | approve | Progressive block map, 2026-09-21. Three rounds: the map used `start`/`end` rather than the specified `source_start`/`source_end`, the default `settle` path left nothing to grow, and a tile absorbed the rasters it settled without telling the client. |
| ENG-8 | approve | The two mappings, 2026-09-21, after three reviews, none of which the tests caught. The text layer first published unclipped rectangles, so a point in the blank margin beside a wide block resolved to a character the reader cannot see; clipping them then removed the only per-row position for the bytes they covered. It now publishes `clusters` clipped for hit testing and `rows` unclipped for every byte that is drawn. |
| EXT-1 | approve | One engine per window, 2026-09-21, first attempt, established in a real VS Code: nothing runs before the first preview, two documents leave exactly one engine, closing the panel keeps it, and the window's end leaves none. |
| NFR-5 | approve | Starting the engine once per window, 2026-09-21, after the earlier review rightly rejected the engine-side probe. The same real-editor run establishes it. |
| NFR-2 | approve (engine part) | A screenful costs 444 KB of measured bytes, inside the 500 KB target: 191 KB of PNG, 163 KB of text layer, 27 KB of block map. The independent count of complete responses was 427,641 bytes. |
| NFR-4 | approve | No orphaned sessions, 2026-09-21, first attempt. scripts/serve_lifecycle.py runs three cases — a closed pipe, a host killed outright, and an empty pipe — and checks that no session survives each. |
| ENG-14 | approve | Protocol export, 2026-09-21, after two reviews. The first pass wrote the client's bytes to a temporary and let the reader derive its title from that temporary's name, so a headingless document carried the temporary into its metadata and differed from the command line. |
| ENG-4 | approve | Observation and saves, 2026-09-21, after three reviews. Two were bugs in `saved` itself — it resolved resources through the stored directory instead of the document path, and it read a new document without taking its newly named resources again. The first pass also misread the requirement: never observing the file is not the same as suspending observation and restoring it. The regression the approval left owed — a newly named image settling after an observed reload — is now in `serve::tests::a_save_restores_observation_of_the_file`. |
| ENG-10 | approve | Client-set appearance, 2026-09-21, after two reviews. A theme changed nothing until it was installed into the effective stylesheet rather than the renderer's fallback, and a served session still listed the reader's stylesheet directory for a named style. |
| ENG-15 | approve | Diagnostics, 2026-09-21, first attempt: `degraded`, `math_errors`, and `deferred` travel with every published state. |
| ENG-11 | approve | Per-document settings, 2026-09-21, after two reviews. Both leaks were places that still read session options: the re-layout a settling image triggers, and a tile's centring. |
| ENG-9, ENG-7 | approve | Source ranges and the text layer, 2026-09-21, after six reviews. Each round found a real defect: absolute offsets in a content-keyed cache, a panic on a multi-character entity, two decoders that disagreed about entities, a byte search that matched a fence's language or a container's marker, a blank literal line that consumed no source line, and three separate holes in the cache key (inline spelling, inline placement, container placement). |
| ENG-2 | approve (again) | Re-reviewed with ENG-6. A served SVG shown larger than its intrinsic size was drawn from an unsettled raster until the render path's second pass was mirrored. |
| EXT-5, NFR-3 | approve | Scrolling that follows the reader, and a place that survives typing, 2026-09-21, after nine reviews between them. Each round found a real defect: a block-granularity sync that could not see a line inside a block; a byte-versus-code-unit mix-up; a hold that dropped a real scroll; a source-distance dead zone that swallowed short lines; a block-start fallback that lost a position deep inside a block; a search that could not find a line in a block with uneven lines; an anchor read across an await while the webview's temporary move replaced it; an edit that crossed the anchor without moving it; and finally three rounds on the reveal that answers a scroll — an answer that moves nothing, an answer identified by arrival time, and an answer identified by a byte that was still on screen. Positions are now resolved through the engine to the row they are drawn at, and the panel only reveals a byte the editor is not already showing, so every reveal it makes is one the editor answers. |
| ENG-16, EXT-13 | approve | Export by template, 2026-09-22, after three reviews, all of them real. The first found that picking "None" in the template picker fell back to the configured template, and that the setting was read without the exported document's scope, so a `[markdown]` or folder override never reached it. The second found that a templated PNG export left its stylesheet installed on the session's shared renderer, so the next preview tile was drawn in the export's appearance. The test now draws a band before and after such an export and asserts the pixels are unchanged. |
| EXT-4 | approve | Live buffer updates, 2026-09-21, after two reviews. The first found that five of the six edits in the burst were refused by the editor and that one update opened the document twice; the second approved it with six applied edits, seven buffer changes and one layout. |
| EXT-2, EXT-3 | approve | The panel and virtualized scrolling, 2026-09-21, after three reviews in a real VS Code. The first found a preview that stayed hidden behind another tab and bands that accumulated and were refetched; the second found a previously previewed document's change listener outliving the switch, so editing it blanked the panel now showing another document and replaced its extent. The panel now reveals itself when asked for, keeps one tile per band and drops the ones it has scrolled past, asks for every band the viewport shows rather than only the one its offset falls in, drops tile answers whose document id is not the one on screen, and watches one document at a time. |
| EXT-2, EXT-3, INV-2, TST-4 | approve | Repaint continuity, 2026-09-29, round 1 (`.work/logs/review-repaint.log`): retain previous pixels until the visible replacement decodes, atomically publish bands, guard stale interactions, and clear pixels on document switches. Final full suite (`repaint-full-3.log`) and installed-VSIX regressions (`repaint-installed.log`) passed with clean shutdown. NFR-1 remains open. |
| EXT-2, EXT-3, INV-2, TST-4 | approve | Demo audit follow-up, 2026-09-27, round 1: override VS Code image max-height and request physical pixels at display density. Completed `.work/logs/demo-full-suite-3.log` verifies visible CSS bounds and DPR 2 rasters; `.work/logs/review-demo.log` approves. Manual preview and PDF/PNG export succeeded in the installed host. |
| EXT-2, EXT-8, EXT-10, EXT-13; ENG-5, ENG-10; INV-1..4 | approve | Shared templates and fixed toolbar icon, 2026-09-27, round 2 (`.work/logs/review-template-2.log`). Round 1 found invalid configured preview templates blocked explicit exports; export now uses an independent short-lived document. Native workspace tests, 12 GPU tests, installed-package regressions (`template-installed-2.log`) and final full suite (`template-extension-4.log`) passed. The fixed icon opened a Mondrian preview in the installed host. |
| INV-1..5, ENG-16, EXT-8, EXT-12, EXT-13, NFR-4, TST-4 (export-only package scope) | approve | 2026-09-26, first completed review of `editors/vscode-export`; an earlier connection attempt did not run. The installed darwin-arm64 VSIX exports dirty buffers with bundled/custom templates and scoped defaults, explicitly bypasses defaults for None, and leaves no engine after graceful close. Picker sequencing uses mocked dialogs; actual dialog driving and host-kill behavior were not re-established. This does not close the full preview extension's outstanding requirements. |
| EXT-8, EXT-9, EXT-12, EXT-13, TST-4 (export-only context menu) | approve | 2026-09-26, first review of 0.1.1. Native UI driving verified both Markdown editor context-menu entries go directly to Save and export under the workspace template. Installed-VSIX tests verify scoped defaults and dirty buffers; a command regression verifies the menu's URI wins over a different active editor. Full-preview requirements remain pending. |



## Not yet established

Requirements that no engine probe can settle, to be established by driving a
real editor, which `editors/vscode/run-tests.sh` does: it loads the extension
into the VS Code installed on the machine and runs `test/run.js` inside a real
extension host. See [State of play](#state-of-play) for what is left.

## State of play

Markview4vsc 0.2.0 consolidates preview and export under the published
`Stevvven.markview-export` identity. Branch `feat/markview4vsc` is rebased on
main `9f61e25`; upstream PR is [#24](https://github.com/szdytom/markview/pull/24).
The second completed consolidation review approves (`review-unify-4.log`),
after fixing the stale engine captured by active-editor following. Final local
full, installed recovery and actual reload suites pass, as do 821 CPU tests,
13 GPU tests, Clippy, rustfmt, TypeScript, picker/framing tests and actionlint.
The reviewer independently reran installed regressions with engine replacement.
All six VSIX targets built in [the initial packaging run](https://github.com/szdytom/markview/actions/runs/36535563760).
Three-OS Rust checks pass. Linux real-host CI exposed an asynchronous viewport
assertion and layout-versus-physical-pixel tolerance; both tests were corrected
without removing their source alignment, no-bounce or anchor checks. Updated
The earlier local full suite passed (`unify-physical-pixel.log`). The latest hosted
run failed another layout-unit scroll assertion and hung after Edge inherited
its output. The test now uses a display-pixel bound, records Linux desktop
handoffs, and runs the editor with bounded process-group cleanup. Lifecycle
checks and installed regressions pass (`unify-host-lifecycle.log`,
`unify-host-regressions.log`); fresh full runs reproduce the known first-frame
stall (`unify-host-cleanup.log`, `unify-host-cleanup-2.log`) and now exit promptly.
The test-only follow-up passed review round 1 (`review-host-cleanup.log`);
Linux CI then exited promptly on an edit-anchor assertion (`unify-host-ci.log`).
At the user's request, CI now sets `MARKVIEW_SKIP_SCROLL_SYNC=1` to defer the
EXT-5 synchronization/edit-anchor group and `scroll-top.js`; cases remain
available when the flag is unset. A passing reduced run does not approve EXT-5.
Preview rendering, fit/resize, links, export and lifecycle checks remain enabled.
The reduced installed-VSIX suite passes with clean shutdown
(`unify-deferred-scroll.log`); hosted CI is pending.
Marketplace publication is explicitly deferred.


Active Markdown editor following is implemented, including serialized opens
and stale-response rejection. The final full real-editor suite passes
(`follow-full-7.log`): dirty buffers, per-folder settings, focus, panel reuse,
non-Markdown retention, rapid switching and closed-panel behavior are covered.
Installed regressions (`follow-installed.log`) and actual window reload
(`follow-reload.log`) also pass with clean engine shutdown. Round 1 approved
the focused follow-up (`review-follow.log`).

Window-reload restoration and template-colored outer spacing are implemented;
the installed ordinary-window reload test passed (`restore-real-reload-final.log`),
including dirty buffers, reading position, editor group, template background and
later edits. The installed full suite (`restore-installed-full.log`), 798 CPU
tests and 13 GPU tests also pass. Round 1 approved the focused follow-up
(`review-restore.log`). Older panels with no saved URI need reopening once after
upgrading; newly opened panels persist the information needed for restoration. Saved webview state
contains only the document URI and reading position; the editor still owns text.
The engine sends its resolved opaque page background with each tile.

Binary preview transport now uses raw PNG frames and typed arrays. The fixed
1200×800 sample preserves the exact PNG SHA-256 while reducing a tile response
from 308,793 to 231,625 bytes (24.99%). Workspace tests (798), GPU tests (12),
framing tests (3), render parity, installed-preview regressions, the final full
host suite (`binary-full-3.log`) and installed export (`binary-export-2.log`) pass.
The first completed review approved this transport follow-up (`review-binary.log`);
fit/scroll reviews, NFR-1 and six-platform distribution remain open. These are transport figures,
not end-to-end keyboard latency or zero-copy claims.

Preview repaint follow-up retains the previous pixels until all replacement visible
bands decode, then publishes them together. The final full extension suite
(`repaint-full-3.log`) and installed VSIX regressions (`repaint-installed.log`)
passed, including delayed two-band replacement and clean engine shutdown. The first
completed review approved this repaint follow-up (`review-repaint.log`); NFR-1
remains open.

The full Path B implementation now lives in `editors/vscode` on
`feat/markview4vsc`, rebased on main `9f61e25`. Markview4vsc retains the published
`Stevvven.markview-export` identity and legacy export settings/commands. Preview
and export share one native process and the client in `editors/shared`; the
standalone export package has been retired. The consolidation review approved its second completed round.

Fit-to-window display now keeps a fixed 16px outer inset and shrinks narrow panes without native reflow, with the full
suite and installed-package tests green. Its review is pending due to connection failures.

Continuous scrolling was updated on 2026-09-27 and passed the full extension suite
and installed-package regressions. EXT-5 remains reopened pending a review verdict.

Implemented and approved in the fresh port reviews (NFR-1 and NFR-6 remain open):

- Preview and default export share document-scoped templates, with legacy export-setting
  fallback and explicit palettes preserved. A distinct M document icon opens preview
  from the Markdown toolbar. Round 2 approved independent explicit exports.
- Demo audit fixed collapsed tile images and Retina blur. Real-webview bounds/density
  regressions and the completed full suite passed; review approved the follow-up.
- Preview, virtualization, source mapping, editing and scroll synchronization;
  document-scoped settings, theme following, links and template export.
  EXT-8/9 passed the first fresh review against the installed package;
  EXT-10..13 and all engine/invariant requirements passed round 2.
- Fixed-font/offline server fixtures with eleven GPU tests separated from the
  default suite. The first port review found stale Mermaid appearance; round 2 approved the fix
  that refreshes pixels, preserves palette-only geometry and publishes reflow.
- Compact rendered-text requests for find. The host no longer fetches the entire
  document's cluster geometry or sends the source buffer to the webview.
- A host-owned Copy Preview Selection command, exercised through VS Code's real
  clipboard API. Native find now uses Chromium `window.find`, matching VS Code’s
  implementation, with real-webview next/previous/wrap regression coverage.
  The workbench input field itself has not been driven. EXT-6/7 and TST-4 passed round 3.
- Local Apple Silicon VSIX packaging and a six-target artifact workflow. The
  other five platform builds have not run for this port. Linux runtime
  documentation passed NFR-7 review; the build matrix alone does not close NFR-6.

## Known defects and missing evidence

- Some earlier consolidation runs stalled at the first painted frame (`unify-scroll-await.log`, `unify-recovery.log`). The final full and installed regressions pass, and 30 repeated open/switch/close cycles did not reproduce it (`unify-open-cycles.log`). Tile rejection/decode diagnostics are retained; the intermittent root cause is not established or claimed fixed.

- Binary transport follow-up: one full-host run (`binary-full-2.log`) saw a ~47px
  layout-coordinate resize-anchor shift. Installed regressions subsequently passed
  unchanged; the separate fit/scroll review remains open.

- **16px inset follow-up (2026-09-27):** locally installed; actual four-edge dimensions, resize, density, selection and pointer mapping passed installed-VSIX regressions (`inset-installed.log`). Full suite passed (`inset-full-5.log`) with clean shutdown. Review gate could not connect (`review-inset.log`, exit 2), so approval remains pending.
- An expanded 800-paragraph PNG export fixture failed both with and without the inset (`inset-full-2.log`, `inset-baseline.log`); the cause remains unconfirmed. Final validation restores the original 400-paragraph fixture and bounds virtualization requests per scroll instead of assuming a particular number of screens.

- **EXT-2/3/5/6/7 follow-up (2026-09-27):** shrink-to-fit display is implemented and locally installed. It preserves native layout, display-density rasterization, scaled selection/click/drag coordinates, scrolling and resize anchors. Final full suite (`fit-full-8.log`) and installed-package regressions (`fit-installed-6.log`) passed with clean engine shutdown. Review remains pending: the gate could not connect (`review-fit.log`, exit 2); no approve verdict was obtained.

- **EXT-5 reopened (2026-09-27):** continuous scroll following now coalesces updates at 50 ms, removes the 120 px dead zone and already-visible-source skip, and uses native row positions with top/gap handling. Full real-editor tests (`scroll-follow-full-2.log`) and installed-VSIX regressions (`scroll-follow-installed-2.log`) passed, including caret stability, no-op reveals, stale mappings and clean shutdown. The local update is installed. Review remains pending: connection timeouts prevented the gate from running (`review-scroll-follow.log`, exit 2); no approve verdict was obtained.

- **NFR-1** remains open: the test host refuses `type`/`default:type` commands even
  with the source editor active. Automated timing measures `TextEditor.edit` to
  decoded visible tiles after a paint frame, including the host acknowledgment;
  it does not include keyboard dispatch or physical display scanout.
- Before compact find transport, 1 MB edit-to-paint median was 1,780 ms; afterward
  it was 357 ms (seven samples, 775 ms maximum). These are diagnostic results,
  not approval of NFR-1. The completed installed-VSIX run measured medians of 80/88/340 ms at
  10 KB/100 KB/1 MB, with respective maxima of 90/122/709 ms.
- Six platform packages need distribution evidence. A local macOS artifact
  alone does not close **NFR-6**. Native find input-field driving remains a
  manual check; the Chromium search/navigation path is now exercised.
- Fresh reviews caught relative template resolution, merged source-map gaps,
  entity-cluster collisions and edits lost during initial open. Fixes and
  regression tests are implemented. Engine/invariant and EXT-10..13 reviews
  approved round 2; live-preview round 2 and input round 3 also passed. The completed
  installed-preview suite includes find after selection, wrapping, deep
  paragraph matches, entity copying and initial-open replay.
- No port requirement is closed until its own review gate returns approve.

## Open questions

On 2026-09-29 the user authorized consolidation, CI, a PR from their personal
fork into `szdytom/markview` main, and a local extension update. Marketplace
publication is explicitly deferred. The tag-triggered publishing workflow is
prepared but no release tag or Marketplace upload is part of this change.

## Port verification

Run `cargo test --workspace --locked --all-targets` for the non-GPU set.
The server's eleven raster/readback tests are explicitly ignored in that set;
run them on a GPU with `cargo test --locked -p markview --lib serve::tests -- --ignored --test-threads=1`.
Server unit fixtures and subprocess protocol tests use committed font subsets
and offline mode. An invalid-tile request that checks the device limit belongs
to the GPU set too. Existing renderer GPU tests retain their own ignore markers.

The extension's latency test applies VS Code's `TextEditor.edit` and waits
for the matching document version's visible tiles to decode and pass two
animation frames. Its interval includes the return message to the host. This
is an application paint-frame measurement, not a physical display scanout probe.
