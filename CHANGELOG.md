# Changelog

All notable changes to this project are documented in this file.

For new entries:

- Use dense lists: each item stays on one line.
- No soft breaks.
- Entries are grouped under sections such as `Added`, `Changed`, and `Fixed`.

Historical entries keep their existing format.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!--
cargo-dist takes the H2 section whose version matches the tag as the GitHub
Release notes, and its heading as the release title. Keep one H2 per release,
at the same level, without `[brackets]`.
-->

## Unreleased

### Added

- Include Android mobile mode and smallest width in a single About and copied-diagnostics line.

- Add MV4A (MarkView as a Android App), sharing desktop settings, tabs, fonts, image caching and export, with Android document access, touch adaptation and emulator integration tests; the installed app is named Markview.

- Pan a Windows precision touchpad through Direct Manipulation, so the finger's contact and the OS's own inertia move the page as one motion; `MARKVIEW_NO_DM` keeps the wheel paths. The pan yields to scrollbar, selection and viewer drags, and focus loss, resize, reload, a panel opening or a tab switch abandon it mid-glide.
- Document the Direct Manipulation touchpad pan in the architecture document, and add the design's terms, **contact** and **native inertia**, to the scrolling glossary.
- Add MVaaC instance-local MVSS registration and dynamic selection by ID, with cached parsing and reserved `bundled:` theme IDs.
- Add configurable text and inline background edges with local-em baseline shifts, shared by reader and PDF typography.
- Integrate tabs into platform-style window chrome, with a persisted System, macOS, Windows or Linux layout choice and native window movement and resizing.
- Add connected rounded tabs alongside underlined tabs, selectable in general settings without document reflow.

### Changed

- Hide the scroll-speed setting on Android phones and tablets.

- Refine the phone tab drawer with left-aligned filenames, subtle selection markers, quiet open controls and a theme-colored Markview icon.

- Use `sw600dp` to keep phones in portrait with a left tab drawer and hide their tab-style setting; tablets retain desktop tabs and rotation.

- Move tab style from MVSS to the `tab-style` user setting, defaulting to `underline` (formerly `classic`).
- Default text layout edges to cap-height/baseline, allowing line heights and page breaks to follow the new typography policy.
- Balance macOS traffic-light margins and deepen light and builtin tab-strip backgrounds, matching inactive tabs to the strip.
- Strengthen light-theme hover feedback, unify right-side toolbar and window-button spacing without a separator, and add Linux chevron and diamond controls.
- Choose window layout from a dropdown, matching the language selector.
- Share caption input routing across viewer and reader modes, keep layout queries free of native-window mutations, and cache tab-strip extents.

### Changed

- Batch a pan's sub-pixel travel until it adds up to a whole physical pixel, and flush the residue on release: the OS inertia tail stops redrawing the page for motion the display cannot show, while the page still lands exactly where the viewport does.

### Fixed

- Support touchscreen dragging and edge resizing of custom window frames on Linux, using the touch contact's native serial on Wayland.
- Hide desktop window-layout and single-instance options on Android and ignore their saved runtime values.
- Make settings fill the app content area on narrow screens, keeping centered dialogs on wider screens.
- Preserve reading positions inside large images when returning to cached tabs, retaining bounded intrinsic sizes while releasing inactive pixels.

- Reload images when returning to cached tabs, including after closing the active tab.
- Keep custom window controls interactive above the image viewer and support caption-button taps without triggering covered controls.
- Make unused tab-strip space draggable on Windows and Wayland, sharing tab bounds with native Windows caption hit testing.
- Disable unused Adwaita window decorations so Wayland does not parse desktop button layouts or warn about `icon` buttons.
- Show the pointer cursor over custom window buttons, including Windows maximize-hover Snap Layouts, while preserving drag and resize cursors.
- Pin the page while the OS inertia coasts over a touchpad contact that never left the pad, so a hand that rests on the glass holds the page still.
- Count a touchpad pan's first and last frames in full: the travel the first content update already carries pans instead of grounding the gesture, and the final update reaches the seam even when it shares the release's batch.
- Keep an open image viewer out of the touchpad pan: its contacts stay on the wheel paths, which zoom the viewer, instead of scrolling the page behind it.
- Claim the touchpad pointer through the window's pointer hit-test, the only message that can begin a touchpad gesture; the touch events winit delivers never carry the pad's contacts.
- Keep the viewport's own ready-reset cycle out of the fold, so releasing a pan parks the content without throwing the page back by everything the hand had travelled.
- Invalidate MVaaC cached geometry when stylesheet selection changes font definitions, including when restoring the default theme.
- Raise inline code beside Chinese text by 0.08em (#6), join mixed-font code backgrounds, and keep vertical background padding independent of line height.
- Keep the same external spacing between Chinese prose and inline code containing Chinese or Latin text.
- Keep glyphs, inline backgrounds and decorations with their text row during PDF pagination, preventing compact spacing or large padding from duplicating text.
- Keep blank text rows safe when the font collection is empty, including the browser's initial font configuration.
- Check image-viewer redraws independently of caption drawing order across all window layouts.

## 0.1.11 - 2026-10-03

**Highlights**:

- Start a search with `/` and jump straight to the next match.
- Refined touchpad and wheel scrolling, without stalls or lurches.
- New setting to open files from a second launch in tabs of the existing window.
- Correct CJK bold rendering in the reader and PDF export.
- More predictable triple-click and drag selection.
- Continued robustness and security hardening.

### Changed

- Pin Comrak to upstream revision `4127507`, which includes all of Markview's merged parser fixes, in the reader and fuzz harness.
- Expand MVaaC demo fonts with variable Latin weights and italics plus Chinese medium/semibold faces, using npmmirror with a jsDelivr fallback.
- Use version-pinned CDN WOFF2 faces for MVaaC demo Latin and Simplified Chinese text, and persist font downloads across reloads with Cache Storage.
- Remove the MVaaC demo’s redundant reading hint.
- Redesign the MVaaC demo with restrained typesetting, square controls, a centered width-limited workspace and a compact header with a GitHub link.
- Load the MVaaC SPA’s Noto fonts from version-pinned CDN URLs and keep tiny subset fonts in the regression harness only.
- Show font download counts, received bytes or percentages and renderer/page preparation stages during MVaaC SPA startup.

### Added

- Start an empty document search with `/` while reading, ignoring selected text and preserving literal slash input in text fields.
- Expand the `fuzz/` harness with three document-structure oracles (reference resolution, source-range content, `<details>` structure), a structural PDF readback oracle, a LaTeX-aware `math` mutator, and pinned-slot campaign scripts.
- Prepare WinGet package manifests, installation checks and stable-release update submissions for `szdytom.Markview`; releases keep moving while first-time registration is pending.
- Add an opt-in single-instance setting that opens files from subsequent launches in tabs of the existing reader window.
- Link macOS app bundles and Linux AppImages in release downloads; list the Windows MSI before the portable ZIP and include macOS installation and quarantine instructions.

### Fixed

- Keep inline SVG decoding and disclosure source navigation compatible with container-prefix and footnote parsing fixes.
- Keep a covering font family when the requested weight is unavailable, selecting its closest available weight instead of skipping to the next candidate.
- Start a paused wheel stream's new travel at packet arrival and decay speed across stalled frames, preventing jumps when rendering resumes (#3).
- Compare canonical paths in single-instance regression tests, fixing Windows path assertions and macOS directory-notification timeouts.
- Clear cancelled wheel momentum targets and start paused or reversed packet streams from the displayed position, preventing jumps and motion against new input.
- Restore the high-resolution wheel stream's packet momentum dropped by the interaction refactor, so a Windows touchpad's inertia rides across the gaps between packets again (#3).
- Spend the speed a paused wheel stream leaves behind, whose stale carry sent the page on the old way and made a reversal answer nothing (#3).
- Continue a wheel stream through the flushes a batchy touchpad delivers after a silence and the whole detents it rounds out mid-gesture, instead of killing the momentum into a dead stop and a lurch (#3).
- Still the page within a beat once its packets go quiet, by lifting the lead and dying at a faster quiet rate, so a hand that halts on the pad no longer leaves a long coast (#3).
- Cap the chase and adapt the lead to packet spacing, let a late flush keep the stream's decaying speed instead of claiming one from its silence-long distance, and believe a packet's own rate no faster than the chase — a flush lands as a dense burst of large packets whose rates name speeds no finger reached — so all of them arrive as glides instead of lunges and freezes (#3).
- Make the syntax-highlight cache regression test independent of background worker scheduling.
- Keep single-instance authentication tokens private, bound IPC requests and reject forwarding after the event loop closes.
- Forward macOS desktop-opened documents before a secondary instance exits and register surviving independent windows when enabling single-instance mode.
- Run campaign blocks in libFuzzer fork mode with `-keep_seed`, so a known crash no longer ends the block before its time budget and the fork startup merge stays off the clock.
- Signal the whole process group when `rsscap.sh` passes its RSS ceiling, so a heavy descendant cannot outlive the wrapper.
- Block in `slot.sh --wait` until a slot frees instead of failing immediately like `--try`, and keep the holder record when a lock is opened.
- Create the campaign artifact, slot and guard-log directories before they are first used, so a fresh checkout can start and check a campaign.
- Match each reference text run to the reference span that contains it in the `refdef` oracle, so a link resolving to a destination its definition contradicts is caught.
- Stop the `refdef` definition scan from reporting list-, quote- and raw-HTML-contained definitions as parser crashes, and never invent a definition row.
- Preserve the order of explicit `mathprobe` probe sizes instead of duplicating and reversing them.
- Normalize footnote labels before numbering, so a disclosure body reusing a document label references the one note instead of allocating a second number.
- Ignore `<details>` inside code spans and fenced code when deciding whether a prefix ends in an open HTML block, restoring the opening-viewport fast path.
- Preserve paragraph source ranges before tables with CRLF line endings by updating the Comrak pin.
- Keep indented code block source ranges stable across trailing blank lines by updating the Comrak pin.
- Keep `<details>` footnotes once at the document's end, including cross-disclosure references; defer incomplete disclosures and unstable note numbering in prefixes, and assign heading anchors in final reading order.
- Give adjacent disclosures their own source ranges, retaining original offsets through multiline HTML tags inside lists and block quotes.
- Complete syntax highlighting synchronously in layout differential fuzzing to avoid reporting background color updates as cache divergence.
- Keep preceding blocks unchanged when an unused link reference definition is appended, with correct Comrak table source positions and consistent `<details>` body termination.
- Pin Comrak to our front-matter line-count fix to prevent a `<details>` parsing panic on UTF-8 documents with lone carriage returns, and keep closing-tag ranges correct after HTML normalization.
- Unify the MVaaC demos into a reading/editing SPA with shared documents, an embedded component guide, file opening, downloads and a compact responsive workspace.
- Extract editor-independent scroll anchors, input ownership and versioned request cancellation into `@markview/scroll-sync`.
- Render inline SVG as static host-decoded images and reject unsupported external SVG dependencies in browser resources.
- Add per-instance shared font sets, an explicit font loading/cache package and optional WOFF/WOFF2 decoding in official WASM builds.
- Add reusable viewer/editor/resource packages, a CodeMirror split editor with automatic source following, configurable TOC/layout and a built-package example.
- Add versioned MVaaC source geometry, complete TOC and a container-mounted viewer with progressive navigation and reading-position events.

- Center the MVaaC editor divider grip in horizontal and vertical layouts, including hosts with a global border-box reset.
- Keep MVaaC disclosure summaries in their intended font and spacing by supplying their required 600 weight.
- Evict MVaaC demo font cache batches that fail validation so malformed HTTP 200 responses cannot prevent startup recovery.
- Preserve CR source-line offsets and restrict text-free collapsed-container scroll anchors to their visible opening line.
- Exclude relocated footnotes from MVaaC scroll anchors so early definitions cannot displace main prose synchronization.
- Batch MVaaC scroll anchors in Rust and retain progressive-layout prefixes instead of repeating quadratic source queries on every revision.
- Rebuild MVaaC editor scroll following around continuous, reversible position maps with shared endpoints and gesture ownership to prevent jumps and feedback.
- Keep editor-to-preview scrolling continuous across wrapped image source and multiline SVG, including adjacent blank lines.
- Parse adjacent SVG elements iteratively and preserve normalized code and math text extending beyond inline SVG boundaries.
- Preserve Unicode source ranges in adjacent quoted disclosures, atomic image geometry and source navigation through horizontally panned content.
- Retain source reading targets while progressive reflow has not yet published enough content to position them.
- Preserve original source ranges inside HTML disclosures and code-line offsets across CRLF input.
- Preserve symlinked settings resources and fade loading/error feedback when previewing the document.
- Load settings styles and font catalogues in the background with immediate loading feedback, cached refreshes and retry controls.
- Exclude automatic CJK/Latin spacing from the start and end of selection highlights.
- Limit triple-click selection and dragging to the current paragraph or table cell inside lists, block quotes and other containers.
- Preserve CJK bold in reader and PDF themes by treating the preferred Medium face as a minimum weight instead of an absolute override.
- Ignore the pointer position reports the macOS backend repeats before every wheel event, which pulled an open list's highlight back to the hovered option between notches.

## 0.1.10 - 2026-10-01

This is a hardening release: hostile-input fuzzing removed panics in parsing, incremental reparse, download date/`max-age` handling and raw HTML scanning, and shaping no longer hangs on an extreme font size. Desktop readers also get a full-size image viewer, wheel-scrolled option lists and per-role font selection, while the WebAssembly front end stays in-progress groundwork.

### Added

- Add host-managed asynchronous MVaaC image requests with viewport priorities, cancellable RGBA delivery, progressive reflow and opt-in browser URL/decode helpers.
- Keep an open option list inside the panel that owns it and cap its width at the panel's, so a list never overflows the window.
- Scroll an open option list with the wheel: wherever the pointer rests, one notch of travel moves the highlight one option, a trackpad's pixel travel accumulates to that notch, and a list longer than its window draws its own scrollbar.
- Open a full-size viewer over any image: click to open, wheel or glide to zoom about the pointer, drag to pan, click or Escape to close.
- Add `markview-selection`, the pointer-driven selection machine taken out of `InteractionState`, so a second front end can drive the same grains, click counting and drag rules.
- Cover the selection paths that had no test: what a copy takes, the drag-past-edge auto-scroll tick, the deferred `Ctrl+A`, and carrying a selection across an update.
- Let a host with no filesystem supply its own faces as bytes through `FontConfig::from_faces`, so the shaper can be driven from a front end that has no directory to scan.
- Add `markview-web`, a `wasm-bindgen` front end that lays Markdown out with the engine and paints it into a `<canvas>`, with pointer selection, clipboard copy and layout budgeted across frames.
- Add the MVaaC canvas demo under `web/`: a textarea feeds Markdown to the WebAssembly module, with a Playwright acceptance suite that checks rendering, incremental re-layout and copy.
- Add `scripts/check_web_font_coverage.py`, which reports the characters of a document that none of the bundled subset faces can draw.
- Add `ProgressiveLayout`, a layout pass that suspends between blocks and resumes without re-measuring what it already laid out, so a front end can bound each step by a time budget.
- Add the `@markview/web` TypeScript package and a pnpm monorepo under `web/`, bundling the WebAssembly front end into a reusable component with a progressive layout handle and a very thin demo app.
- Choose the family of each font role — serif, sans-serif, monospace and the same three for Han text — from a **Set fonts** step beside the Fonts page's catalogue filters, one chooser row per role: a pick reflows the document at once and persists as a per-role `fontdef` override, and the Han rows appear only for the `cjk-type` variant in force, offering only the families that cover Han text.
- A `cargo-fuzz` harness for the library crates (`fuzz/`, not part of the root workspace): eleven targets covering parse, incremental reparse, MVSS, layout, progressive layout, math, highlight, shaping, fonts, PDF export, and render geometry, with structure-aware mutators, calibrated per-input wall and allocation budgets, and spec-derived seed corpora. A `fuzz`-only `prewarm_highlight` API lets the harness pay the syntax-highlight warm-up before its input budgets start.

### Changed

- Simplify MVaaC builds with pnpm setup/build commands, a cross-platform Python WASM builder and a TypeScript-only rebuild path.
- Point the Comrak patch at upstream again, now carrying the fence-offset fix and the multiline inline source-position fix.
- Align publishable web package versions with Markview and synchronize them during releases.
- Load MVaaC text fonts from host URLs or bytes during initialization; the esbuild demo emits separate font assets, while KaTeX stays embedded in wasm.
- Share document hover, cursor, scrolling and gesture inertia between desktop and Web; Web adds host activation callbacks, anchors, details and selectable scroll motion ownership, defaulting to internal wheel easing.
- Route Windows fractional wheel events through ordinary wheel easing, removing the touchpad-specific packet momentum path pending a new adaptation.
- Share one cancellable native download service and CPU budget across images and fonts, with owned progress events and transactional installation.
- Version pixel lookups and frame demand, move eviction outside cache locks, and wake layout through a shared latest-request mailbox.
- Bound background CPU work by queue size and retained inputs; cancel obsolete highlight epochs and wake owners after completion.
- Own diagram face metrics under one lock and share reclaimable font names; reject chooser selections from retired catalogues.
- Drive `layout_progressive` from the resumable pass, so the engine has one block loop instead of two and a cancelled prefix costs nothing to resume.
- Split the font module into validation, directory scanning and diagram selection, so a front end reuses the parts it has and leaves the rest behind.
- Put directory scanning behind a `font-directories` feature that is off by default, so a build for a host without a filesystem leaves every `std::fs` call out of the binary.
- Focus security documentation on policy and the threat model, with implementation details and verification work in separate references.
- Name the loop generically in `handle_user_event` and the new `tick`, so the event and timer paths run without a window server.
- Separate the reading text a copy takes from the clipboard write that stores it.
- Take a `SurfaceSource` in `markview-render` instead of a window, so the renderer never names a windowing toolkit and a canvas front end can drive it the way the desktop window does.
- Take the library crates' clock from `web-time` instead of `std::time`, because `Instant::now` panics on `wasm32-unknown-unknown`; on every other target it is the same type re-exported.
- Put texture readback and GPU waits behind a `readback` feature that is off by default, because both block the calling thread and a browser has no second thread to wait on.
- Declare wgpu's `webgl` backend for `wasm32-unknown-unknown`, so `markview-render` builds for the Web; `winit` leaves its dependency tree.

### Fixed

- Prevent malicious HTTP dates and overflowing `max-age` values from panicking during downloads or redirect cache calculations.
- Take multiline inline source positions from the block rather than the inline, so formulas and code spans keep the same range across differing indents, containers and line endings.
- Keep the line ending that closes an incremental parse window, so a multiline formula or code span at the window's edge gets the same source range as in a full parse.
- Prevent shaping from hanging when an extreme font size overflows Parley's line height, including sizes supplied by a valid MVSS rule.
- Keep thematic-break source ranges on their marker line, so trailing blank lines no longer change block IDs or make incremental and full parses disagree.
- A raw HTML attribute scan no longer splits a multi-byte space that stripping a leading `/` exposed (`<details /\u{a0}open>`), which panicked on an 18-byte document.
- A merged inline's source range only ever grows, so the out-of-order positions comrak reports after a link reference definition can no longer invert a range.
- Treat a `[x]: y` line that continues a paragraph as text, not as a reference definition, so a prefix parse or a `<details>` body no longer resolves a link the document never had.
- A prefix parse no longer cuts through YAML front matter or an open raw HTML block, and keeps the line ending a trailing list marker needs.
- Remove deprecated TypeScript `baseUrl` options and resolve web package aliases relative to each configuration file.
- Apply horizontal overflow transforms and clipping to MVaaC image priorities, and infer SVG MIME types for byte decoding.
- Keep scanned glyph fallback choices independent of preceding characters, reuse fallback faces when caches fill, and preserve the cached text length limit.
- Preserve Web selections and held drag bases until a replacement reflow has laid out their blocks.
- Honor a corrected MVaaC wasm URL on retry when an early font failure leaves the previous binary request pending and that request later fails.
- Preserve the displayed Web scroll offset when changing motion mode or pressing the pointer during an animation over incomplete layout.
- Toggle Web disclosures using the document behind the published prefix, and stop internal scroll animation when external input pans a wide block.
- Apply external scroll travel from the displayed animation offset, preserve absolute requests through growing Web layouts, and defer End navigation until the final document height is known.
- Move image cache writes and eviction off the shared I/O runtime so disk work does not stall transfers or cancellation.
- Cancel pending save selections during shutdown, keep highlight preparation linear, and finish queued file creation before cancellation cleanup.
- Persist restoring the last font role to Default by removing its saved override.
- Offer Han font choosers only for definitions resolved by the selected CJK variant.
- Draw open language menus once to preserve translucent backgrounds and avoid redundant shaping.
- Fix image viewer activation without selectable text, capture covered controls' input, and cancel or latch drags correctly across focus loss and pointer movement.
- Refresh open image viewers after decoding and keep very thin images within the window's fit bounds.
- Cover the WebAssembly front end's publication bookkeeping and selection-length cache with native tests, replacing the Playwright wall-clock timing assertion that could flake on a loaded runner.
- Keep the last completed pass's block geometry when a later pass is cancelled, so typing past a document no longer flushes the warm cache.
- Keep a block's completed-pass membership separate from the pass that last used it, so a pass that resumes geometry and is then abandoned cannot cost the warm cache.
- Let a selection stop between the letters of a ligature, so `ff` and `fi` can be picked apart instead of only taken whole, while a single grapheme is still never parted.
- Re-derive the selection when the view scrolls under a held press, so scrolling with the wheel extends what the pointer covers.
- Take a formula or a drawn image whole, since its source text is not what is displayed, instead of selecting an arbitrary slice of it.
- Mark the selected part of a ligature from the glyph's own advance, so a horizontally scrolled line never lights the letter that is still onscreen in place of the one selected.
- Keep selection painting linear in the paragraph length by scanning only the cluster's own grapheme boundaries.
- Ask a WebGL2 device only for the limits it has, so the browser backend gets past `request_device` instead of failing it on the desktop storage-buffer and compute defaults.
- Color a document's code blocks without spawning a thread where the target has none, since `thread::spawn` panics on `wasm32-unknown-unknown` and every fenced block went through it.
- Source ranges after a lone carriage return: comrak ends lines at `\r` as well as `\n`, and the line-offset table followed only `\n`, shifting every range after the break (and desynchronizing the incremental parser).
- The incremental parser's line and blank-line model now follows the parser's: a lone carriage return breaks a line, so a list marker hidden after a mid-line carriage return can no longer take the fast path with a window cut through the list's range.
- An indented `</details>` closing tag no longer drags the element's source range past the tag, which could index out of bounds.
- The incremental parser falls back to a full parse when its change window starts on a mid-document byte-order mark, which the parser only strips at the start of a document.
- Code blocks and HTML source split their lines on the parser's line structure (a lone carriage return ends a line), so a carriage return inside a code line can no longer reach the shaper inside a run.
- Border fitting on a box with a non-positive extent no longer runs a `0 / denormal` scale that produced `NaN` border widths.
- A quoted `<details>` body and the reference definitions a prefix parse appends now split their lines on the parser's line structure, so a lone carriage return no longer quotes a body line twice or hides a `[x]: ...` definition from the prefix.
- Draw a cluster no configured face covers from a scan of the whole collection's character maps before handing it to the platform, so rare symbols like the long double arrow render instead of tofu even where the per-script fallback knows no family, and the fallback warning now means the glyph is genuinely absent.

## 0.1.9 - 2026-09-29

This release adds in-document text search with a bottom search bar, MVSS `table.border_collapse` modes, and artist-template font downloads through international mirrors. It also brings a reusable single-line text input with IME support and fixes a long tail of scrolling, PDF export and font-loading issues.

### Added

- Offer artist-template font downloads through international CTAN and TUNA mirrors, with matching SHA-256 pins and template fonts preferred over system alternatives.

- Add MVSS `table.border_collapse` modes to share cell borders or retain independent, optionally rounded cell borders.

- Find text throughout a document with a bottom search bar, Unicode case folding, whole-word matching, disclosure navigation and independent match highlights.

- Reusable single-line text input with selection, clipboard, undo/redo and IME support; PDF exports can use a custom title for each document session.

### Changed

- Replace the Qi Baishi, Van Gogh and Mondrian export layouts with updated MVSS designs, preserving their bundled IDs.

- Prefill document search with the selected text when pressing Ctrl+F (Cmd+F on macOS).

- Close the search bar when switching documents while retaining each document’s query and match options.

- Submit document searches immediately without debounce, scan a compact text index and receive results without rebuilding highlight indexes on the UI thread.

### Fixed

- Keep a high-resolution wheel stream moving across the gaps between its packets: a Windows touchpad hands its inertia over as a few large packets, each arriving a quarter of a second after the motion it describes, so the reader now carries a speed between them instead of easing every packet from a standstill, which made a fast two-finger scroll crawl and then lurch. Whole-detent wheels and stream events are untouched.

- Keep PDF code-block text selectable with glyph-level mappings; leave copied whitespace reconstruction to the viewer.

- Compress PDF images with bounded loading and rayon instead of retaining all decoded pixels; abort export with a source-specific error if an image resource is missing.

- Pin table border integration tests to committed fonts so they pass with system fonts hidden in Linux CI.

- Keep screen geometry and theme flags out of PDF commands, restrict `--scroll` to `render`, and reject reader arguments before subcommands.

- Update screenshot callers for the render-only `--scroll` option and abort capture when the reader exits or fails to initialize.

- Keep table grid borders at a single thickness in reader and PDF output by default, and avoid scanline rounding of square GPU borders.

- Wake waiting font transfers when the transfer gate is poisoned, even with warning logging disabled.

- Recover poisoned runtime caches and gracefully degrade failed font and worker state instead of cascading panics.

- Reveal the correct search anchor in RTL text and preserve visible highlights around multiline image placeholders.

- Coalesce IME cursor-area updates and skip unchanged rectangles to prevent Wayland request feedback during text input.

- Use canonical document paths in search tests so tab restoration works on Windows and macOS temporary directories.

- Pin fonts in app, image, diagram, font catalogue and GPU tests, including SVG decoding, benchmark warmup and a committed CJK Medium face; Linux CI now hides system fonts.

- A font test flaking under parallel `cargo test` by relying on a global cache slot surviving LRU eviction; it now asserts on the collection it built itself.

## 0.1.8 - 2026-09-24

This release adds compiled-in Chinese and Japanese interface translations, improves font loading, and fixes front matter layout and macOS file opening.

### Added

- Draw the whole interface in Simplified Chinese — both panels, the reader's chrome, the status line, the notice strip, the confirmation and the fonts page — following the system language or the new Interface language setting under a new Interface section, with every label compiled in from `assets/locales`.
- Translate the interface into Traditional Chinese and Japanese as well, so a `zh-Hant`, `zh-TW`, `zh-HK` or `ja` locale gets its own text instead of the Simplified or English fallback.
- Offer the interface languages in a list the row's own control opens, so the chooser grows with every language the build carries instead of with every row's width.
- Make that list own input like a confirmation: it takes the pointer, wheel, touch and keys while it is open, returns focus to its chooser when it closes, and closes when its row leaves the page.
- Carry the status line, the footer's warnings, the export panel's own buttons and the format names in its save dialog in the same compiled-in text, so no label is left behind in English.

### Changed

- Draw YAML front matter as a collapsed `Frontmatter` disclosure whose body is the `yaml` code block, dropping the YAML parser and its dependency so no alias or nesting expands under the reader; exports draw none of it.
- Move Scroll speed out of Reading layout into the Interface section, beside the language it shares a device-level concern with.
- Resolve the system language once instead of per frame, so drawing a label costs no allocation.
- Refresh the measured performance baselines, charts and comparison tables against the current build, and drop SuperGoodViewer from the reader and PDF tables.

### Fixed

- Use the personal font download directory in PDF and PNG exports and in the `pdf`, `render` and `smoke-test` subcommands, so an export matches the reader's display; `--ignore-system-fonts` still pins the run.
- Put the front matter's label and visibility options in its layout cache key, so switching the interface language or hiding the front matter for an export updates its disclosure instead of reusing stale geometry.
- Ask for a relayout when the interface language changes, so the front matter's label follows it instead of waiting for the next edit.
- Map each font file into the shaper instead of reading it whole, so a drawing run that loads the download directory pays for the tables a document draws rather than for every face on disk.
- Open the Markdown files the macOS desktop hands the reader — a double-click in Finder, an "Open with" choice, or a file dropped on the app's icon — which arrived as an Apple Event the bundle advertised but nothing answered, so the desktop reported that the reader could not open the format and started it with an empty window; every file of one event becomes its own document.

## 0.1.7 - 2026-09-23

This patch release adds touch and trackpad gestures, captured dragging, inertial scrolling, and larger control hit areas.

### Added

- Support touch taps, captured dragging and inertial scrolling, with larger control hit areas and shared touchpad gesture handling.

### Fixed

- Keep fast trackpad scrolling continuous when fingers lift before motion settles, avoiding a pause followed by a sudden long scroll.

## 0.1.6 - 2026-09-23

This release renders YAML front matter, adds an About tab and collapsible table-of-contents sections, brings the green 8-bit reader theme, and reworks the Styles and Fonts settings.

### Added

- Add compact, right-aligned stacked-square SVG buttons in the Contents header to expand or collapse all outline sections.
- Add the 8-bit reader theme with green CRT colors, Fusion Pixel typography throughout the document and UI.
- Collapse and expand nested table-of-contents sections with per-tab state and keyboard navigation that skips hidden headings.

- Add a compact About tab in Settings with a centered application icon, the standard project description, a clickable project link and one-click copying of diagnostic information: version, build date, OS, active WGPU backend, and commit.

- Read `---` fenced YAML front matter: a flat mapping renders as a two-column table with a bold key column, anything nested renders as a `yaml` code block, and `front_matter` joins the MVSS vocabulary as the frame around either shape — with `table`/`cell` or `code_block`/`label` composing into one rendering, and `show = false` hiding the block while its metadata stays parsed. A document that opens with an unclosed `---` is unchanged.

### Changed

- Prefer upright LXGW WenKai for Simplified Chinese emphasis in reader and PDF themes, offer it through GitHub, SourceForge and archlinuxcn (Europe/TUNA) font downloads, and fall back through system Kai fonts (`regularscript[cjk]`) to synthetic italics.

- Reuse segmented settings controls for the Fonts status filter, with shared borders and consistent selection and focus highlights.

- Rework the Styles settings with clearer priorities: numbered badges for enabled styles, and arrows hidden at the ends of the order.
- Rework the Fonts settings: drop the source filter, rename the states to Missing / Downloaded / In System, split the bulk action into Download Missing and Download All, and show sources, download progress and empty-state guidance.
- Consolidate panel navigation and font UI ownership, share bounded HTTP transport, and decouple PDF requests from CLI launch options.
- Clarify paired performance checks under any consistent power mode; refresh absolute README metrics only during releases.
- Compare readers and PDF pipelines against SuperGoodViewer 1.0.8, whose mathematics fixtures now render and whose new `sgv export` command joins the PDF table.
- Refresh the README size, memory and first-frame figures to the current build.

### Fixed

- Draw shared borders in segmented settings controls only once, preserving selection and keyboard focus highlights.

- Update font download counts as each file finishes, accumulate transferred bytes correctly, and show a continuous progress bar using each active transfer’s byte fraction with clearer details and an SVG cancel control.

- Draw the increase/decrease controls with SVG icons instead of font glyphs, avoiding minus-sign font fallback warnings.
- Keep the bulk font-download buttons hoverable, and silently do nothing when no downloads are needed.
- Align the Styles summary and separator with Generic settings while keeping the tabs clear on short panels, align style text, center the priority badges and row controls, and hide unavailable arrows.
- Build the downloadable-font catalogue when the Fonts or Styles page opens instead of during window startup, removing a system-font collection build from the first frame and about 8 MiB from the reader.
- Name the requested resource in shared HTTP transport errors, so a failed font download no longer reports an image error.
- Fold a ligature's continuation clusters into the cluster that draws the glyph, so its selection highlight, hit testing and copy range cover the whole ligature instead of leaving a gap over half of it.

## 0.1.5 - 2026-09-22

This release adds a downloadable font catalogue, a table-of-contents drawer, collapsible `<details>` blocks, and theme-colored Mermaid diagrams.

### Added

- Let a stylesheet draw Mermaid diagrams in its own colors with a `[mermaid]` table: a built-in preset plus any of the renderer's fields, overlaid field by field as `[page]` is. `font_family` names `fontdef` ids the way a rule's `font` does. Diagrams measure and draw with the reader's own faces — the theme's list, then the body's Han faces for a cluster the list cannot draw, downloads and `--fonts` included — so a Chinese label keeps the reader's regional face. The bundled dark reader themes use it now, so they no longer show diagrams on white paper.
- Add a persisted `scroll-speed` preference (0.5×–2×, in Settings) that scales every wheel notch and arrow step.
- Add a table-of-contents drawer (`Ctrl+B`, or the toolbar's outline button) that lists a document's headings, highlights the reading position, and jumps to a heading through the ordinary anchor path; it is an overlay, so the document keeps scrolling and selecting behind it, while an open panel or confirmation keeps input precedence over it. A heading near the end scrolls to the top too, using the blank tail the other scroll paths already reach.
- Declare downloadable font families with `[[font-family]]`, independent of `fontdef`: a family lists the names it may already have (any of them being installed skips the download), optional description, SPDX license and homepage, and one or more mirror sources. A source is either a set of direct files or an archive whose container is recognized from its own bytes and whose members are picked by `**`-style patterns, with an optional `sha256` and a 2 GiB archive cap; a failed mirror is removed before the next one runs. `fontdef.urls` is gone with it.
- Give the settings panel three tabs — Generic, Styles and Fonts — and put the whole font catalogue on the Fonts tab: every family the builtin recommendations and the catalogued stylesheets declare, with its name, description, license, size and owner, filters by source and state, one download and one cancel per family, and an **Open fonts folder** button. Styles no longer mention fonts at all.
- Add `markview fonts list|download|path|verify`, and move the diagnostic modes to subcommands (`render`, `pdf`, `bench`, `latency`, `smoke-test`) parsed with `clap`; `markview FILE` still opens the reader, and the old `--render`-style flags now name the subcommand that replaced them.
- Recommend Noto in the reader itself: the builtin stylesheet declares Noto Serif, Noto Sans, Noto Sans Mono, Noto Serif CJK SC and Noto Sans CJK SC, each mirroring the official GitHub release archive with jsDelivr, the canonical ctan.org redirector and the Tsinghua CTAN mirror, and downloading every static weight its mirrors publish—the nine Latin weights with their italics, seven per Chinese subset.
- Offer Noto Color Emoji as a downloadable family, mirrored from GitHub, jsDelivr, ctan.org and the Tsinghua CTAN mirror.
- Accept a color bitmap face as a font download: `CBDT`/`CBLC` or `sbix` strikes count beside the outline tables, since the rasterizer already draws them.
- Offer Fira Code as a downloadable family, from the upstream variable release and from Arch's `ttf-fira-code` package on archlinux.org and the Tsinghua mirror.
- Cache network images on disk beneath the configuration directory: honor `Cache-Control`/`Expires`, revalidate stale entries, hold at most 128 MiB with LRU eviction, and serve a cached body under `--offline`.
- Render a raw `<details>` block as a collapsible element: clicking its summary toggles a Markdown body (nesting and `open` supported), `details` and `summary` join the MVSS vocabulary, and both exports show every body expanded.
- Render a `mermaid` fenced block as a diagram: the library runs on the image workers, its SVG and pixels are cached per source, and the result appears in the window, `--render`, `--pdf` and `--smoke-test`, while a broken diagram keeps the image placeholder and `--offline` still renders local diagrams.
- Measure per-frame scroll pacing in `--bench` over a cold, warm and prewarmed pass: prepare and total percentiles, frames over the 120 Hz and 60 Hz budgets, glyphs rasterized in and out of the frame, and atlas pressure, including for a document with no geometry.
- Support independent box edges and corners, heading markers, letter spacing, position conditions and pagination hints in reader and PDF output.
- Add paper-edge rules under `page.header` and `page.footer`, with symmetric `rule_width`/`rule_color` decorations for PDF and PNG exports.

### Changed

- Redesign the Settings header with a persistent title and clickable Generic, Styles and Fonts tabs.
- Draw the Styles page's order buttons as borderless vector arrows, matching the rest of the chrome instead of text glyphs.
- Replace the Styles and Fonts pages' paging arrows with a scrolling list that shares the panel's wheel and scrollbar, clipping each row to the visible band.
- Prepare Mermaid fonts and SVGs on the image workers, allowing the first layout to publish before a cold font collection is ready.
- Load restricted SVG fonts only from configured Mermaid, SVG generic, and selected CJK `fontdef` candidates, avoiding full system-font materialization on first open.

- Keep a diagram's parsed source across a theme change, so a new `[mermaid]` table redraws the diagrams already on screen without parsing them again.
- Honor the system's per-axis lines- and characters-per-notch on Windows and count a Linux wheel detent as the usual three lines instead of one.
- Ease discrete scroll requests and wheel notches for 120–400 ms: Page Up/Down, `Space`, `Home`/`End`, the arrow steps, a click on the scrollbar track and a `#heading` jump, with a reversing wheel taking over from the displayed offset, while a thumb drag stays immediate and ends a running animation.
- Expand the `<details>` elements framing a heading a `#anchor` link or outline entry names, so a jump into a collapsed body reaches it instead of reporting it missing.
- Hold a wheel gesture to the axis its first few moments chose, separated by the boundary the platform reports or by a pause where it reports none and inheriting nothing from the gesture before it, so a diagonal trackpad gesture no longer stops the page or slips a wide formula away, and a sideways gesture over no wide block still scrolls by the vertical motion it carries.
- Prepare the screenful below in idle frames, leaving the visible frame's image demand alone, so no scroll frame rasterizes a glyph once a document is open: the worst such frame on a 100 KiB CJK document falls from 2.6 ms to 0.6 ms.
- Reuse the PDF writer's font and stylesheet caches across a watch session's rebuilds instead of rebuilding them on every export.
- Export PDFs about 40% faster: transparent fills are no longer written, an inline run's background is one rectangle instead of one per cluster, and a formula's glyphs leave as runs rather than one text object each.
- Compare readers against SuperGoodViewer in the README tables, adding its open time and a resident-memory table for all three readers.
- Measure each reader's resident memory and record readers that fail to render a fixture, instead of timing their compile-error window.

### Fixed

- Close the table of contents when clicking outside its drawer.
- Make platform and image-cache tests portable across macOS, Linux and Windows.
- Fix `<details>` parsing and interaction edge cases: tags that share a block and elements nested in the opening block still match, a summary stays with its own element, a quoted body is not quoted twice, references and footnotes defined outside the element resolve, identical elements toggle independently, and adjacent elements keep their content and nesting budget.
- Reject pathologically nested Mermaid labels and render the diagram on a stack sized for the worst case the source cap allows, so a fence that passes the size bounds can no longer abort the reader.
- Keep a Mermaid diagram's fence source out of the reading text, so selecting the figure no longer copies the source and a loading or failed diagram copies its placeholder message instead.
- Remove soft line breaks from Chinese Markdown prose to avoid inserting spaces.
- Name the download client with a `User-Agent`, so a mirror like Tsinghua's Arch repository answers instead of refusing an anonymous request.
- Name `Noto Serif SC` and `Noto Sans SC` in the bundled Chinese `fontdef` lists, so the subset faces a download installs are actually shaped instead of being skipped.

## 0.1.4 - 2026-09-19

This release adds stylesheet discovery and validation, expands theme and typography support, and improves reader previews.

### Fixed

- Avoid spurious font fallback warnings from UI headings requesting unavailable weight 600 faces.

### Added

- `markview ss list` lists bundled and installed stylesheets.

- Optional CJK Medium UI overlay and GPU weight comparisons, with documented exact-weight fallback behavior.

- Monochrome, Qi Baishi, Van Gogh and Mondrian PDF themes, plus `theme = "none"` for uncolored code.

- MVSS `targets` declares UI/PDF destinations, filters theme selectors and rejects incompatible use.

- Celadon, Blueprint and Rosewood reader themes, a theme preview fixture, and an MVSS authoring skill.

- Benchmark reports separate GPU preparation/submission from blocking completion
  to diagnose tail latency without changing the end-to-end timing scope.

- Settings offer an eye button that fades the panel for live document previews.
- `markview ss validate FILE.mvss.toml` parses a stylesheet in place and
  reports its version and rule count, so a draft can be checked before install.
- The `markview-icon` `icon!` macro parses an SVG at compile time into a
  unit-box vector buffer, so the reader keeps its UI icons as editable files
  with no SVG parser in the binary.
- The reader exports the open document to PDF or a whole-document PNG from a
  toolbar button or `Ctrl+E`, with its own text size, indent, paper, margins,
  stylesheet sequence and PNG scale under `[export]` in `settings.toml`,
  independent of the reading view.
- The export panel's `Export and Watch…` action keeps rewriting the same file
  whenever the document is saved.
- The README sets the same text through a browser engine and through Markview at
  one measure, and `scripts/render_typography_comparison.py` reproduces the
  figure.
- The README also times opening a document against MarkText, with
  `scripts/compare_readers.py`, and one document to one PDF against Typst,
  Chromium and XeLaTeX, with `scripts/compare_pdf_engines.py`.

### Changed

- Prefer Medium (500) CJK faces throughout bundled themes, with inherited-weight fallback; refresh README screenshots.

- Enlarge Print page headers, footers and page numbers to 0.75em.

- Refresh Light/Dark palettes and move shared style defaults into the hidden, lowest-priority `builtin` sheet.
- Reorganize the MVSS guide with theme recipes, cascade rules and a validation workflow.

- Stylesheet panels use a matching vector arrow for the Back action.
- Redesigned reader chrome with flat, square controls and coordinated light/dark themes;
  shared, grouped settings and export panels scroll without shrinking their controls.

- The toolbar's Open and Settings buttons, and every panel's close button, are
  vector icons drawn from those buffers instead of text labels.
- The reader's export opens the written file with the operating system, and the
  export icon points out of its tray instead of into it.
- The export panel refuses to open without a document, and its header names the
  document and the resulting measure instead of repeating the rows.
- PDF export is set at 12 pt body text by default, on the command line and in
  the reader's export panel.

### Fixed

- Keep all three toolbar buttons visible while a panel or confirmation is open.
- Buttons now show distinct hover and pressed fills, including selected choices;
  keyboard-only focus replaces the border without stacking extra outlines.

## 0.1.3 - 2026-09-18

Themeable list markers and code chips, a watching PDF export, and exports that
pin their own fonts.

### Added

- An MVSS font candidate takes `synthetic_italic = true`, which shears an
  upright face by 14° when the family has no italic of its own. The bundled
  styles use it for CJK emphasis, so Chinese and Japanese text now slants
  instead of falling back.
- `--pdf … --watch` re-exports whenever the document or a local image it
  references changes, until the session is stopped. Rebuilds reuse the previous
  parse, block layout and decoded images; an unchanged save is skipped.
- An MVSS `fontdef` takes `emoji = true`, which marks the family as the face for
  Emoji text; the bundled reader and print styles use it.
- MVSS takes `align` and `shape` on list markers: `align` places a bullet,
  number or checkbox left, centered or right in its column, and `shape` draws a
  bullet as a disc, square, triangle, diamond, plus or minus. A `shape` list is
  cycled by bullet nesting depth; ordered levels do not advance it.
- MVSS takes `numbering` and `align` on `enum`, so a theme can format ordered
  numbers (`a)`, `I.`, `一、`, `①`) with Typst's numbering patterns and place
  them independently of bullets. The marker column grows to the widest number
  a list renders.
- MVSS takes `border_width`, `radius`, and `accent` on `task_marker`, so a
  theme can thicken, round and fill a task checkbox; a completed box fills with
  `accent` and draws its check in `color`. The bundled styles do.
- `--fonts DIR` (repeatable) adds a directory of font files, and
  `--ignore-system-fonts` shapes with those directories alone, so an export no
  longer depends on the fonts the machine happens to have installed.
- MVSS takes `padding` on `["code"]`, which insets an inline code chip: the
  horizontal sides widen the run and push its neighbours, and the vertical
  sides make the chip taller. The bundled styles pad code, which used to touch
  the text around it.

### Changed

- Bullets and task checkboxes are drawn shapes rather than text, so they are no
  longer selectable or copied; the bundled styles center them in their column.
- Inline code breaks between any two characters inside a run, free at a word
  edge and at a small penalty inside a word, so a long identifier wraps instead
  of overflowing its block. The run's edges keep ordinary break rules, so a
  following comma or closing bracket never starts a line.

### Fixed

- A task checkbox is drawn as an antialiased box with its check centered
  inside, like a list marker. The interior used to resolve transparent and the
  check used to sit outside the box, so a pending task read as a solid square.
- Emoji style is uniform again: a text family that happens to hold an Emoji
  symbol—Noto Sans CJK covering `⚠️`, say—no longer beats the configured Emoji
  face, which used to mix monochrome and color Emoji in one document.
- A blockquote's bar is centered on the text it frames: the quote's box no
  longer absorbs the outer spacing of its first and last child, which left the
  bar hanging far below a quote.
- Syntax colors no longer leak: a code comment painted every line after it in
  the comment color, because syntect was handed lines without their terminator.
- PDF export drew the CJK and emoji inside a formula's `\text{…}` group with
  the document's fonts instead of dropping them.
- A PDF's text map names every character of a ligature: an `fi` ligature used
  to extract as `f`, losing the `i` from copied or searched text.
- A PDF names every glyph of an ordered-list number, which used to leave a
  replacement character after the number when the text was copied.
- `--pdf --watch` recognizes equivalent spellings of one path, including a
  `..` over a symlinked directory such as macOS's `/var`.

## 0.1.2 - 2026-09-18

Paper export, faster first frames, and a smaller idle footprint.

### Added

- `--pdf FILE --output out.pdf` exports the document to paper: vector text with
  subset fonts, page breaking with widow and orphan control, page furniture, and
  link annotations.
- `--paper`, `--landscape`, `--margin`, `--header*`, and `--footer*` configure
  the page; a bundled `print` stylesheet supplies the defaults, and `--style`
  layers on top of it.
- `--title`, `--author`, `--subject`, `--keywords`, `--language`, and
  `--creator` fill the PDF information dictionary; a title defaults to the
  document's first heading, and unset fields stay out.
- MVSS adds a `[page]` table for paper, margins, and the header and footer
  slots, plus the `page`, `page_header`, `page_footer`, and `page_number`
  conditions.
- `scripts/compare_pdf_render.py` holds the PDF export and the GPU render of
  one document to the same content bands, alignment and profile overlap.
- Footnote references are clickable: a reference moves to its note, and the
  note's number moves back to the citation it was opened from.
- Consecutive footnote references share one bracket pair, as in `[1,2]`, and
  only the numbers stay click targets.
- MVSS adds a `footnote_ref` condition styling footnote references and the
  note's number.
- Math accepts LaTeX `\(...\)` and `\[...\]` delimiters alongside dollar signs.
- `settings.toml` takes a `[justification]` table bounding word spacing and
  letter spacing, so character-level justification can be tuned or turned off.
- `settings.toml` takes a `codeblock-wrap` boolean, also exposed under
  **Settings**, that hard-wraps code block lines at the reading column. The
  `--render` and `--smoke-test` image exports enable it by default.
- A CJK curly quote may start or end a line, as the full-width brackets already
  could, so a quoted phrase no longer glues a CJK run together.
- The Windows MSI registers Markview for `.md`, `.markdown`, and `.mdown`, so
  the reader joins **Open with** and **Default apps**; Windows 10 and 11 still
  ask the user to confirm the handoff once.
- `--bench-latency` measures process-entry first-frame latency, edit-to-refresh
  latency after a small on-disk edit, and the RSS trend across reloads.
- The latency and memory analysis documents the three targets, the responsible
  code, and ranked optimization points; stress fixtures and aggregation scripts
  make its scaling results repeatable.

### Fixed

- PDF pagination preserves multiline headings, includes line gaps in widow control,
  and fits tall images together with their leading space.
- PDF links resolve percent-encoded anchors and keep their hitboxes inside the
  printed text area.
- Heading anchors count suffixes in constant time, so a document that repeats a
  heading no longer parses quadratically (a 1 MiB repeated-heading file parses
  about 3.5× faster).
- A small edit no longer drops part of a list or paragraph: the incremental
  parser recognizes empty list items and Markdown's own blank-line rules
  (Unicode spaces are content, not blank lines).
- Reading counts are cached per content identity and sent with every complete
  update, so a second document with identical content still fills its footer.
- Closing the last tab releases the worker's parsed document, decoded images and
  layout caches, so an idle reader keeps nothing from the document it closed.
- `--render` and `--pdf` wait for the syntax highlighting pass, so exported
  code keeps its colors instead of only the text.
- The window sets its Wayland application ID, so a desktop with
  `markview.desktop` installed shows the Markview icon and groups the window
  with it instead of falling back to a placeholder.
- Compressing a line now moves the glyph with the blank half it spends, so an
  opening CJK bracket no longer overlaps the character after it.
- A quote break keeps the neighbouring prohibition, so a closing quote no longer
  hands a full stop to the next line and an opening quote no longer strands an
  opening bracket on the last.
- A tab-indented fenced code block inside a list no longer gains a leading space.
- A footnote's number is set at the note body's size and baseline in a column
  shared by every note, instead of floating above the text as a superscript.
- The diagnostic renders and benchmarks set CJK text in the configured face
  again: they never selected a `[cjk]` variant, so every Han cluster was drawn
  in a system fallback face. `--cjk-type` now names one on the command line.
- The Windows reader is linked for the Windows subsystem, so opening it no
  longer puts a console window on screen. A run with a command line attaches to
  the console it was launched from, and output with nowhere to go is dropped
  instead of panicking.
- Installing the Windows MSI over an already installed copy of the same version
  replaces it instead of leaving both registered, which a rebuild of a released
  version used to produce.

### Changed

- The minimum supported Rust version is 1.92, which the PDF backend requires.
- Opening a large file paints its first viewport from a bounded prefix parse
  instead of waiting for the whole file: a 4 MiB document shows its first
  readable frame in ~75 ms instead of ~175 ms, and reference definitions or
  footnotes later in the file still resolve in that first frame.
- A small edit re-parses only the block it changed, when the document is plain
  text, and reuses every other block. The reader's reading counts are computed
  on the layout worker rather than the event loop; together these cut a 1 MiB
  edit's time to the refreshed frame by about a third.
- The block cache survives passes and invalidates per block: a localized edit
  re-lays out only the changed block instead of the whole document, and the
  256-entry/100k-draw cap is gone.
- Syntax colors invalidate only the code blocks that gained them, instead of
  clearing the whole layout cache, and the highlight cache no longer clears
  itself at 256 entries. Both remove a permanent re-layout for code-heavy
  documents.
- The file-watch quiet window is 10 ms (was 30 ms) with a 40 ms ceiling, so an
  edit reaches the screen sooner while a save burst is still coalesced.
- System fonts are discovered once per process, and the scan runs on the worker
  while the window and renderer initialize, cutting the native first readable
  frame by about 12–14 ms.
- Justification spends word spaces and letter spacing first, then shares the
  remaining slack evenly, so a CJK line closes to the full measure instead of
  stretching one gap. Word spacing now follows Typst's two-thirds to
  three-halves limits.
- CJK punctuation gives back the blank half of its em box at a line start or
  end, following the convention the `cjk-type` setting names, and Han text
  gains a quarter em against Latin.
- A closing mark hangs part of its advance into the end margin, a hyphen is
  cheaper in the middle of a word than near either edge, and a last line that
  slightly overflows is compressed instead of wrapped.
- A paragraph is reflowed to avoid stranding a single word on its last line.
- An explicit `<br>` justifies the line it ends, while a hard break of two
  trailing spaces does not.
- Packaging builds run only when a packaging input changes, in a `Packages`
  workflow that no longer gates merges.
- Diagnostics go through `log` and `env_logger` as `LEVEL message` lines. The
  window logs at `warn` and the diagnostic modes at `debug`; `RUST_LOG` overrides
  both.
- Layout tests shape with pinned Noto subsets instead of host fonts, so the
  suite no longer passes on macOS and fails on Linux or Windows.
- The `comrak` patch points at upstream again, which now carries the fenced
  block offset fix the personal fork had supplied.

## 0.1.1 - 2026-09-16

A security release. Untrusted documents can no longer abort the process, read
arbitrary local images, or reach the local network.

### Security

- A Markdown file of deeply nested emphasis no longer aborts the process: every
  recursion and work allowance now comes from one shared `Limits` value, and
  pathological code blocks, formulas, tables, and paragraphs degrade instead of
  hanging.
- Image sources are relative to the document only. Absolute paths and `file:`
  URLs are refused, while `../` continues to work.
- Local links follow one policy. Markdown opens in the app, a reviewed inert
  allowlist (text, images, fixed-layout documents, audio, video) and directories
  go to the operating system, and everything else — including `.html` and every
  executable, script, or installer type — asks for confirmation first, defaulting
  to revealing the file in the file manager.
- Remote images are capped at 128 distinct sources per document revision, with a
  notice strip offering Dismiss and Load all. Both answers apply to the tab and
  revision they were chosen in, so opening another document shows its own notice
  and starts capped again.
  Loopback, private, and link-local addresses are refused after resolution and
  before connecting, and the resolved address is pinned so a rebind cannot
  bypass the check.

### Fixed

- Pasting Markdown whose first sentence ends in a multi-byte terminator, such as
  the CJK `。`, no longer panics: the tab title is cut on a character boundary.
- A failed or pending image typesets its placeholder through the paragraph
  engine, so the message wraps, justifies, and hyphenates like body text and
  fills the image box before its last line is elided. Previously the whole
  message was shortened to one line, so a box of any size rarely showed it.

### Documentation

- `docs/security.md` is revision 3: the implemented decisions, the shared
  `Limits` defaults, and the risks that remain accepted are recorded, and the
  open verification work is separated from it.

## 0.1.0 - 2026-09-16

An early development release. Expect breaking changes.

### Added

#### Rendering

- CommonMark headings, paragraphs, block quotes, lists, and emphasis, including
  CJK-friendly emphasis that closes next to CJK text.
- GFM tables and task lists, footnotes, GitHub-style alerts, links, and the raw
  HTML equivalents that Markdown documents use.
- Inline and display math rendered natively, with parsing diagnostics shown in
  the document.
- Asynchronous syntax highlighting for code blocks.
- Images from local files, `file:`, `http(s):` and `data:` URIs, in PNG, JPEG,
  GIF, WebP, BMP, ICO and SVG. An image alone in its block is centered, animated
  images show their first frame, and `--offline` blocks the network.

#### Typography

- Publication-quality paragraph layout with hyphenation and justification, plus
  a greedy comparison mode (`--greedy`).
- Optional paragraph indent, from Settings or `settings.toml`: the opening line
  of a prose paragraph indents, lists indent as a whole, and table cells and
  footnotes stay flush.
- Locale-aware font selection, including CJK fallback.
- Progressive layout, so a usable first frame appears before the whole document
  is laid out.
- Word counting by dictionary instead of by whitespace.

#### Reading

- Multi-document tabs, with drag-to-reorder, labels that shrink to keep the first
  two characters visible, wheel scrolling over an overflowing tab bar, and
  auto-scroll when dragging near either edge.
- Close the active tab with `Ctrl+W`, its × button, or the middle mouse button.
- `Ctrl+V` opens clipboard text that looks like Markdown in a new tab, titled
  from its first heading or sentence.
- Select by drag, by word on double click, or by block on triple click; `Ctrl+A`
  selects the document and `Ctrl+C` copies.
- Draggable scrollbars sized from the stylesheet, and horizontal scrolling for
  wide code blocks, tables, and formulas on hover or with `Shift`+wheel.
- Links open with the operating system's default handler. Other `.md` files open
  in a new tab, middle click opens them in the background and reuses an existing
  tab, and `#heading` fragments move to that heading, in the current document or
  in the `.md` file they name.
- Documents are watched for changes and re-rendered while you read them.
- The end of a document can be scrolled two thirds of a page above the window
  bottom.

#### Stylesheets and settings

- Built-in light and dark styles.
- MVSS `.mvss.toml` stylesheets with composable conditions, installed with
  `markview ss install FILE.mvss.toml` and selected with `--style ID`.
- A settings panel (`Ctrl+,`) backed by a live-reloading `settings.toml`.
- Command-line control over theme, alignment, hyphenation, type size, reading
  column, paragraph indent, window size, and scroll offset.

#### Diagnostics

- `--render FILE --output preview.png` renders through the real GPU pipeline
  offscreen, `--bench FILE` reports layout metrics, and `--smoke-test FILE`
  captures a window.

#### Packaging

- Release archives and installers: a `.tar.gz` and a shell installer on Linux, a
  `.zip`, a PowerShell installer and an `.msi` on Windows, and a zipped `.app`
  bundle on macOS, alongside a `.deb` and an AppImage.
- Application icons generated from the source SVGs by
  `cargo run -p xtask -- icons`, then installed into the window and into the
  Windows executable resources.
- Archives ship `THIRD_PARTY.md` and `licenses/`, and the Debian package and the
  macOS bundle carry a generated third-party notice.
- Every package is built and unpacked on every push, so a broken installer fails
  before a release can publish.

#### Documentation

- Guides for the architecture, the performance model, the security and threat
  model, development, stylesheets, and packaging under `docs/`.
