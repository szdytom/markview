# Security reference

This page records threat details, implementation controls, and historical findings. [Security and threat model](security.md) owns policy and accepted risks; [Security verification](security-verification.md) owns verification status and follow-up work. Asset and capability IDs refer to the model in the main document.

## Threat catalog

| ID | Threat | Capability | Impact | Priority | Status |
| --- | --- | --- | --- | --- | --- |
| T1 | Process abort from unbounded recursion | K0 | A1 | **P0** | Fixed |
| T2 | Hang or CPU exhaustion | K0 | A1 | **P0** | Bounded |
| T3 | Memory exhaustion | K0, K2 | A1 | P1 | Open, needs measurement |
| T4 | Memory corruption in a dependency | K0, K1 | A3 | **P0** | Open, verification only |
| T5 | Arbitrary local file read | K0 | A2 | P1 | Policy decided |
| T6 | Arbitrary file opened by the OS | K0 + one click | A3 | **P0** | Mitigated, confirmation remains the weakest control |
| T7 | SSRF and network beaconing | K2 | A4 | P1 | Mitigated |
| T8 | Symlink and time-of-check/time-of-use races | K1 | A2, A1 | P2 | Accepted |
| T9 | Concurrency state-machine races | K0 triggers | A1 | P2 | Open, needs a model |
| T10 | GPU and driver boundary | K0 | A1, A3 | P1 | Separate track |
| T11 | Interface impersonation | K0 | A5 | P3 | Accepted |

### T1: process abort from unbounded recursion

**Fixed.** `Reader::inlines` in `crates/markview-core/src/document/parse.rs` used to recurse once per inline AST node with no depth budget, while the sibling `Reader::blocks` capped recursion. A 12 KB file of nested emphasis aborted the process with a stack overflow on the 8 MiB stack the layout worker requests, and `--watch` or following a link could reach it a second time.

`inlines` now takes a depth and, at `Limits::inline_depth`, collects the remaining subtree with an iterative walk and keeps it as plain text. Content is never lost, only its styling. The bound is 256; the original investigation estimated about 128 KB of the 8 MiB stack: it exists to stop pathological input, not to restrict nested documents. `crates/markview-core/src/document/tests.rs` keeps the 12 KB input as a regression test.

### T2: hang or CPU exhaustion

**Bounded.** Four document-controlled hot spots had no work budget. Each draws one from `Limits`. Highlighting falls back to uncolored text, math uses its error path, line breaking advances with the best available candidate, and tables truncate their grid.

| Hot spot | Bound |
| --- | --- |
| `crates/markview-core/src/highlight.rs`, syntect | A single line longer than `highlight_line_bytes` (64 KiB) is not handed to the regex engine at all, and only `highlight_bytes` (16 MiB) of code per layout pass is highlighted. The remainder renders uncolored. |
| `crates/markview-core/src/math.rs`, ratex | `math_formula_bytes` (256 KiB) per formula and `math_bytes` (8 MiB) per layout pass. A formula over budget renders through the existing math-error path. |
| `crates/markview-core/src/linebreak.rs`, `greedy` | The greedy fallback now observes the same `linebreak_evaluations` budget as the optimal pass, and always terminates: when the budget runs out it keeps the best candidate found, or advances one unit. |
| `crates/markview-core/src/layout/table.rs`, wide tables | `table_columns` (256), `table_rows` (16384), and `table_cells` (131072) truncate the grid before it is shaped. |

Shaping also explicitly disables Parley's intermediate line-height ceiling.
The six-byte `shaping-raw-size-hang` fuzz input derives a finite font size of
`3.0887546e38`, which overflows line height to infinity; Parley 0.11.1's default
`f32::MAX` ceiling then makes its convenience loop yield forever without
consuming the next cluster. Ordinary Markdown cannot set a font size: HTML
styles are ignored and front matter remains source. A user-selected MVSS rule
can reach it because validation accepts any finite positive `size` (for
example, `size=1.716e37` for `p` hangs on `ab` before the fix). The shared shaper
now sets the height ceiling to infinity before breaking lines. Regression
coverage is in `crates/markview-core/tests/shaping.rs`; this ensures termination,
while extreme sizes can still overflow geometry.

The defaults are chosen so that ordinary documents never reach them; a pasted 10K-character formula is well inside `math_formula_bytes` even when every character is three bytes wide, and a code-heavy document fits inside `highlight_bytes`.

One residual is honest and cannot be closed from here: ratex exposes no work budget, so the bound on a single formula is its byte length. A hostile formula just under 256 KiB can still take a long time. This is recorded under [Accepted and residual risks](security.md#accepted-and-residual-risks) and is the reason the byte limits are the only lever available.

### T3: memory exhaustion

Decoded pixels are capped at 16 million pixels per image, roughly 64 MB of RGBA8, the decoded cache is capped at 256 MB, and four worker threads decode concurrently. The peak is therefore on the order of half a gigabyte before the source text, the comrak arena, and layout are counted. Memory keys hold the raw source string, so a `data:` URI costs roughly its own size in file bytes. This is an estimate, not a measured peak; see [verification status](security-verification.md#security-invariants).

### T4: memory corruption in a dependency

The workspace forbids `unsafe_code`, so every unsafe operation reachable from a document lives in a dependency: `image` for six raster decoders, `resvg` and `usvg` and `tiny-skia` for SVG and curve rasterization, `swash` and `parley` for font parsing and shaping, `syntect` for highlighting, and `ratex-*` for math.

Verification priorities for these dependencies are tracked under [dependency coverage](security-verification.md#dependency-coverage).

### T5: arbitrary local file read

Relative image paths can render sensitive local images and expose existence or file-type information through errors. Directory containment is deliberately absent; see the [image-path policy](security.md#image-paths) and [resolution details](#image-source-resolution).

### T6: arbitrary file opened by the operating system

A document-controlled link can reach an OS handler after a click. Markdown stays in the reader; other local targets follow [local-link classification](#local-link-classification). Confirmation is the remaining gate for non-allowlisted types and can be approved by mistake. The [policy and accepted risks](security.md#links-and-os-handlers) explain that tradeoff.

### T7: server-side request forgery and network beaconing

Remote images can disclose the reader's address and opening activity. A default source cap limits automatic activity, while DNS checks, pinned addresses, and redirect checks block access to non-public destinations. Load all changes only the source cap. The notice does not precede requests within the cap. Fresh cached bodies can avoid a request; stale bodies may be revalidated. See [network implementation](#network-and-font-downloads) and the [network policy](security.md#network-access-and-caching).

### T8: symlink and time-of-check/time-of-use races

**Accepted.** With containment removed from the image policy, a symlink inside the document directory can point anywhere and Markview will read through it. This is deliberate: the containment check that would have blocked it also blocked `../`, and it protects against an adversary who can already write to the document directory. On top of that, the image staleness stamp is still `(len, mtime)`, and a writer can preserve both.

### T9: concurrency state-machine races

The application runs the layout worker, four image loaders, a file watcher, and the UI. `Worker` and `Images` each carry their own generation and ticket counters, and `Images::poll` briefly holds the decoded-pixels and demand locks together, which is a lock-order obligation that nothing currently documents or enforces. The proposed concurrency models are tracked in [verification](security-verification.md#mitigations).

### T10: GPU and driver boundary

Malformed geometry reaches wgpu as validation errors, device loss, or driver defects. The original verification proposal uses a headless smoke test on a software adapter that opens documents and renders frames without asserting on pixels; it does not establish driver safety. Raster images are resized to stay within the 8192-pixel texture dimension, while SVG rasterization is only bounded by a 16-million-pixel check, which is an asymmetry to note but not a defect by itself.

### T11: interface impersonation

The HTML subset interprets no `class` or `style`, so document content cannot adopt reader styling. Headings, link text, and image alt text remain attacker-controlled, which is a low risk recorded here so that it is not re-litigated. It is a P3 non-goal. The confirmation modal deliberately shows the canonical path rather than the link label, which is the one place where this risk could have been amplified.

## Resource limits

`crates/markview-core/src/limits.rs` defines one `Limits` value for core parsing and layout depth, iteration, and byte allowances. `document::parse` uses the default, and layout reads it from `LayoutOptions::limits`, so styling, math, highlighting, table layout, and line breaking all take their budget from the same place.

| Field | Default | Governs |
| --- | --- | --- |
| `inline_depth` | 256 | Inline AST recursion in `Reader::inlines` |
| `block_depth` | 256 | Block nesting retained as structure in `Reader::blocks` |
| `linebreak_evaluations` | 2,000,000 | Candidate breaks per paragraph, optimal and greedy |
| `highlight_line_bytes` | 64 KiB | Longest line handed to the syntax highlighter |
| `highlight_bytes` | 16 MiB | Total code highlighted per layout pass |
| `math_formula_bytes` | 256 KiB | Largest single formula laid out |
| `math_bytes` | 8 MiB | Total formula bytes laid out per layout pass |
| `table_columns` | 256 | Columns retained from a document table |
| `table_rows` | 16,384 | Rows retained from a document table |
| `table_cells` | 131,072 | Cells retained from a document table |

The values are compile-time defaults and are not exposed to `settings.toml` or the command line: a user could raise them and reintroduce exactly the hangs they exist to prevent.

## Existing defenses

Controls include: `unsafe_code` forbidden workspace-wide; ratex pinned to an exact version; per-image pixel, byte, and cache bounds; the `--offline` switch; the `data:image/` prefix check; the SVG image-href resolver disabled; the texture-dimension clamp for raster images; the single link policy in `src/link.rs`; the shared pinned-address HTTP policy in `src/net.rs`; the bounded image cache in `src/images/cache.rs`; the per-revision remote cap; the explicit, verified, user-triggered font download in `src/fonts.rs`; and the shared `Limits` value with its tests.

## Boundary implementation

### Image source resolution

Implementation: `src/images/source.rs`.

```text
1. Reject an empty src.
2. Parse as a URL. Accept http and https only. Keep data:image/ with its size
   cap. There is no file: branch.
3. Otherwise treat as a path:
   a. Percent-decode.
   b. Reject if it is absolute, if its first component is a root, or, on
      Windows, if it is a prefix (C:foo) or rooted without a drive (\foo).
   c. Join it to the document directory. Canonicalize when the target exists,
      for alias deduplication only.
4. Do not confine the result to the document directory. `..` is allowed.
```

### Local-link classification

Implementation: `src/link.rs`, with the modal in `src/app/chrome/modal.rs` and the dispatch in `src/app/pointer.rs`.

The allowlist is a closed, reviewed list, not a category. Additions are a security change:

- **Opens in Markview**: `.md`, `.markdown`, `.mdown`.
- **Handed to the OS**: `.txt`; `png jpg jpeg gif webp bmp ico svg avif tiff tif heic`; `pdf epub mobi azw3 djvu cbz cbr xps oxps`; `mp3 m4a aac flac wav ogg oga opus wma aiff mid midi`; `mp4 m4v mkv webm mov avi wmv flv mpg mpeg 3gp ogv`.
- **Directories**: handed to the OS.
- **Everything else**: the three-button confirmation. This includes `.html`, `.htm`, `.xhtml`, `.ps`, `.eps`, `.swf`, `.chm`, `.jar`, `.rtf`, archives, and non-allowlisted executable, script, and installer extensions.

The modal offers **Open folder** (the default), **Open anyway**, and **Close**, and owns clicks, keys, and scrolling while open. See the [confirmation policy](security.md#links-and-os-handlers).

`src/link.rs` canonicalizes existing targets before testing their extensions, so `note.txt` pointing to `payload.desktop` is classified as `.desktop`. If canonicalization fails, it retains the supplied path. Classification uses the extension, not Unix executable permission bits; an allowlisted extension is not made confirmation-only by setting an executable bit. HTTP, HTTPS, and mailto URLs go to the OS; `file:` URLs enter local classification, and other schemes are refused.

### Network and font downloads

Implementation: `src/images.rs` (cap, notice state), `src/net.rs` (resolution, pinning, and the streaming download client), `src/images/cache.rs` (bounded disk cache) and `src/fonts.rs` (the explicit font download); the strip is drawn from `src/app/chrome.rs` and the catalogue from `src/app/font_panel/view.rs`, with `markview fonts` in `src/app/fonts_command.rs`.

1. By default, at most 128 distinct remote sources are admitted per document revision; Load all lifts this cap. The remainder render as placeholders naming the reason, so a headless `render` or `smoke-test` run cannot block on them.
2. The notice strip appears below the tab bar and offers Dismiss and Load all. Both answers belong to the tab and content revision they were chosen in: opening another document shows its own notice and starts capped again, and so does a reload. The exemption travels with the layout request instead of a shared flag, so it cannot leak into the next document laid out.
3. Image requests allow at most five redirect hops, 32 MiB per body, 15 s total and 5 s to connect. Loopback, private, link-local, carrier-grade NAT, unspecified, documentation, multicast, and broadcast addresses are refused on the initial URL and on every redirect hop, after resolution and before connecting. The Load all exemption never bypasses this policy.
4. A fetched body is stored under `cache/images` beside `settings.toml`, keyed by a hash of its absolute URL, bounded at 128 MiB with least-recently-used eviction, and installed by rename so a partial body is never served. A fresh entry needs no request; a stale one revalidates with the stored `ETag`/`Last-Modified`. `--offline` never calls the client but serves a cached body whether or not it is fresh, deliberately overriding `no-cache` and `must-revalidate` because there is no network to revalidate against.
5. A font family a stylesheet declares under `[[font-family]]` is fetched only by an explicit action on the reader's Fonts page or by `markview fonts download`. Both use a streaming async client with the same resolution, pinning and address policy as the image client. Transfers stream to a file with no whole-request limit, a 5 s connect limit and a 60 s silence limit, and a `User-Agent` naming the reader, because a mirror may refuse an anonymous client. HTTPS is preferred; plain `http` is accepted because a mirror may serve only that, and the cost is transport privacy for a URL the user chose. A family's sources are mirrors ordered by a one-time latency measurement per host — the declared order only breaks ties — each a set of files or one archive; the container is recognized from its own leading bytes, extraction refuses directories, symlinks, hard links and any path outside the download directory, and is bounded at 2 GiB and 4096 members. A font file is capped at 64 MiB, an archive at 2 GiB, a declared `sha256` must match, and the body is verified as a font (every table record inside it, the tables a drawable face needs — outlines, or a color emoji face's `CBDT`/`sbix` bitmap strikes — and a nonempty character map) before it is renamed into place, so a partial transfer is never registered. A failed source's own files are removed before the next mirror runs. Nothing is cached by the image cache; the verified files in `fonts/` are their own cache. `--offline` refuses the job with a message. The directory is a personal resource: the reader, its export jobs, and the drawing subcommands load it by default, while the measurement modes and `--ignore-system-fonts` keep the configured set, so a download cannot change a pinned export.

## Historical findings

**T1 — stack overflow in `Reader::inlines`.** Reproduced outside the application, on a thread with the same 8 MiB stack size that `Worker` requests in `src/worker.rs`, and now fixed.

```sh
python3 -c "n=6000; print('*'*n + 'a' + '*'*n, end='')" > nested.md
```

The file is 12001 bytes. The following observations were recorded during the original investigation and fix; they are not measurements rerun for this documentation reorganization:

| Input | Result |
| --- | --- |
| 8001 bytes (`n = 4000`) | Parses, one block |
| 12001 bytes (`n = 6000`) | Aborted the pre-fix debug test binary; rendered by the fixed tree |
| Release binary, `n = 30000` (60001 bytes) | Aborted before the fix; rendered now |
| Release binary, `n = 120000` (240001 bytes) | Aborted before the fix; rendered now |
| Comrak alone, 8 MiB stack, 50000 levels | Parses; the AST is built iteratively |
| Comrak alone, nested emphasis, 40 KB | AST depth 10002 |

The threshold depends on the frame size, so a debug build aborts at 12 KB while
a release build reaches about 60 KB before the same failure; the fix is a bound,
not a moved threshold, and it holds at both. The recursion was Markview's, not
the parser's, and the trigger was small enough to arrive inside an ordinary
document. It now has a depth budget matching the one `Reader::blocks` already
has, and the input is a regression test that aborts the unfixed tree.
