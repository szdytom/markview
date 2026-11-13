# Historical security investigation

> Archived from `37f22d622d41d5409321b8db7cf9f36b38b0d5a1`. This report preserves earlier findings, estimates and implementation descriptions. Some assertions (including workspace-wide unsafe-code prohibition) no longer describe the tree. Use the [current policy](../security.md) and [verification guide](../security-verification.md) for new work.

This page records threat details, implementation controls, and historical findings. [Security and threat model](../security.md) owns policy and accepted risks; [Security verification](../security-verification.md) owns verification status and follow-up work. Asset and capability IDs refer to the model in the main document.

## Threat catalog

| ID | Threat | Capability | Impact | Priority | Status |
| --- | --- | --- | --- | --- | --- |
| T1 | Process abort from unbounded recursion | K0 | A1 | **P0** | Fixed |
| T2 | Hang or CPU exhaustion | K0 | A1 | **P0** | Bounded |
| T3 | Memory exhaustion | K0, K2 | A1 | P1 | Open, needs measurement |
| T4 | Memory corruption in a dependency | K0, K1 | A3 | **P0** | Open, verification only |
| T5 | Local reads beyond document authority | K0 | A2 | P1 | Relative-path and Untrusted resource gates enforced |
| T6 | Arbitrary file opened by the OS | K0 + one click | A3 | **P0** | Mitigated, confirmation remains the weakest control |
| T7 | Network tracking and cache history | K0, K2 | A4 | P1 | Accepted privacy risk |
| T8 | Symlink and time-of-check/time-of-use races | K1 | A2, A1 | P2 | Accepted |
| T9 | Concurrency state-machine races | K0 triggers | A1 | P2 | Open, needs a model |
| T10 | GPU and driver boundary | K0 | A1, A3 | P1 | Separate track |
| T11 | Interface impersonation | K0 | A5 | P3 | Accepted |
| T12 | Unauthorized requests to local services | K0, K2 | A7 | P1 | Document modes, destination pinning, and scoped authorization enforced |
| T13 | Sensitive data sent beyond reading authority | K0, K2 | A2 | P1 | URL credentials and proxies refused; request-capture coverage exists |

### T1: process abort from unbounded recursion

**Fixed.** `Reader::inlines` in `crates/markview-commonmark/src/parse.rs` used to recurse once per inline AST node with no depth budget, while the sibling `Reader::blocks` capped recursion. A 12 KB file of nested emphasis aborted the process with a stack overflow on the 8 MiB stack the layout worker requests, and `--watch` or following a link could reach it a second time.

`inlines` now takes a depth and, at `Limits::inline_depth`, collects the remaining subtree with an iterative walk and keeps it as plain text. Content is never lost, only its styling. The bound is 256; the original investigation estimated about 128 KB of the 8 MiB stack: it exists to stop pathological input, not to restrict nested documents. `crates/markview-commonmark/src/tests.rs` keeps the 12 KB input as a regression test.

### T2: hang or CPU exhaustion

**Bounded.** Four document-controlled hot spots had no work budget. Each draws one from `Limits`. Highlighting falls back to uncolored text, math uses its error path, line breaking advances with the best available candidate, and tables truncate their grid.

| Hot spot | Bound |
| --- | --- |
| `crates/markview-layout/src/highlight.rs`, syntect | A single line longer than `highlight_line_bytes` (64 KiB) is not handed to the regex engine at all, and only `highlight_bytes` (16 MiB) of code per layout pass is highlighted. The remainder renders uncolored. |
| `crates/markview-layout/src/math.rs`, ratex | `math_formula_bytes` (256 KiB) per formula and `math_bytes` (8 MiB) per layout pass. A formula over budget renders through the existing math-error path. |
| `crates/markview-layout/src/linebreak.rs`, `greedy` | The greedy fallback now observes the same `linebreak_evaluations` budget as the optimal pass, and always terminates: when the budget runs out it keeps the best candidate found, or advances one unit. |
| `crates/markview-layout/src/layout/table.rs`, wide tables | `table_columns` (256), `table_rows` (16384), and `table_cells` (131072) truncate the grid before it is shaped. |

Shaping also explicitly disables Parley's intermediate line-height ceiling.
The six-byte `shaping-raw-size-hang` fuzz input derives a finite font size of
`3.0887546e38`, which overflows line height to infinity; Parley 0.11.1's default
`f32::MAX` ceiling then makes its convenience loop yield forever without
consuming the next cluster. Ordinary Markdown cannot set a font size: HTML
styles are ignored and front matter remains source. A user-selected MVSS rule
can reach it because validation accepts any finite positive `size` (for
example, `size=1.716e37` for `p` hangs on `ab` before the fix). The shared shaper
now sets the height ceiling to infinity before breaking lines. Regression
coverage is in `crates/markview-layout/tests/shaping.rs`; this ensures termination,
while extreme sizes can still overflow geometry.

The defaults are chosen so that ordinary documents never reach them; a pasted 10K-character formula is well inside `math_formula_bytes` even when every character is three bytes wide, and a code-heavy document fits inside `highlight_bytes`.

One residual is honest and cannot be closed from here: ratex exposes no work budget, so the bound on a single formula is its byte length. A hostile formula just under 256 KiB can still take a long time. This is recorded under [Accepted and residual risks](../security.md#accepted-and-residual-risks) and is the reason the byte limits are the only lever available.

### T3: memory exhaustion

Decoded pixels are capped at 16 million pixels per image, roughly 64 MB of RGBA8, the decoded cache is capped at 256 MB, and four worker threads decode concurrently. The peak is therefore on the order of half a gigabyte before the source text, the comrak arena, and layout are counted. Memory keys hold the raw source string, so a `data:` URI costs roughly its own size in file bytes. This is an estimate, not a measured peak; see [verification status](../security-verification.md#remaining-evidence).

### T4: memory corruption in a dependency

The workspace forbids `unsafe_code`, so every unsafe operation reachable from a document lives in a dependency: `image` for six raster decoders, `resvg` and `usvg` and `tiny-skia` for SVG and curve rasterization, `swash` and `parley` for font parsing and shaping, `syntect` for highlighting, and `ratex-*` for math.

Verification priorities for these dependencies are tracked under [dependency coverage](../security-verification.md#remaining-evidence).

### T5: arbitrary local file read

Relative image paths can render sensitive local images and expose existence or file-type information through errors. The [document modes](../security.md#document-trust-modes) allow supported reads by default for Trusted local files, while Untrusted clipboard and web content need authorization for the identified local resource. Temporary staging must not supply local-read authority. The source resolver gates local reads before opening files; clipboard relative images require an explicit file selection and web relative images resolve against the page URL. Directory containment remains deliberately absent for authorized reads; see the [image-path policy](../security.md#image-paths) and [resolution details](#image-source-resolution).

### T6: arbitrary file opened by the operating system

A document-controlled link can reach an OS handler after a click. Markdown stays in the reader; other local targets follow [local-link classification](#local-link-classification). Confirmation is the remaining gate for non-allowlisted types and can be approved by mistake. The [policy and accepted risks](../security.md#links-and-os-handlers) explain that tradeoff.

### T7: network tracking and cache history

**Accepted privacy risk.** Remote images can disclose the reader's address and opening activity, including a tracking URL authored directly in Markdown. A default source cap limits automatic activity; it is not a tracking-consent mechanism. Load all changes only that cap. Fresh cached bodies can avoid a request; stale bodies may be revalidated, and cached URLs leave a local history. Offline mode prevents network requests. This is separate from unauthorized local-service access (T12) and disclosure of sensitive data (T13). See [network implementation](#network-and-font-downloads) and the [network policy](../security.md#network-access-and-caching).

### T8: symlink and time-of-check/time-of-use races

**Accepted within authorized local reads.** With containment removed from the image policy, a symlink inside the document directory can point anywhere and Markview will read through it. This permits neighbouring image directories and `../`. That default authority belongs to Trusted documents; Untrusted local reads require an identified resource grant before image contents are read. The gate precedes image-content reads. The image staleness stamp is still `(len, mtime)`, and a writer can preserve both.

### T9: concurrency state-machine races

The application runs the layout worker, four image loaders, a file watcher, and the UI. `Worker` and `Images` each carry their own generation and ticket counters, and `Images::poll` briefly holds the decoded-pixels and demand locks together, which is a lock-order obligation that nothing currently documents or enforces. The proposed concurrency models are tracked in [verification](../security-verification.md#remaining-evidence).

### T10: GPU and driver boundary

Malformed geometry reaches wgpu as validation errors, device loss, or driver defects. The original verification proposal uses a headless smoke test on a software adapter that opens documents and renders frames without asserting on pixels; it does not establish driver safety. Raster images are resized to stay within the 8192-pixel texture dimension, while SVG rasterization is only bounded by a 16-million-pixel check, which is an asymmetry to note but not a defect by itself.

### T11: interface impersonation

The HTML subset interprets no `class` or `style`, so document content cannot adopt reader styling. Headings, link text, and image alt text remain attacker-controlled, which is a low risk recorded here so that it is not re-litigated. It is a P3 non-goal. The confirmation modal deliberately shows the canonical path rather than the link label, which is the one place where this risk could have been amplified.

### T12: unauthorized requests to local services

A document can name `http://localhost:PORT/action` as an image source without controlling that server. If a request is sent, an operation may happen even when the response cannot decode as an image. Redirects and DNS answers can also move a nominally public resource to a local service. This is a client-side request and authorization threat; ordinary NAS images or an explicitly opened intranet page are not attacks in themselves.

`src/net.rs` checks every resolved address against the receiving `Security` context and pins direct connections. The [policy](../security.md#target-authorization-policy) allows ordinary LAN, VPN, and localhost images for Trusted local documents and requires origin- and address-class-scoped grants for Untrusted sessions. An explicitly opened local page remains Untrusted with authority for that target; every redirect checks the receiving context again. Link-local destinations require a grant in either mode. Automatic HTTP/SOCKS proxies are disabled. Trusted defaults or a grant accept the risk that a GET has side effects.

### T13: sensitive data sent beyond reading authority

The confidentiality boundary is whether document input can cause local file contents, ambient credentials, or a fetched response to be sent to another destination. Supported relative image reads and passive cross-origin image display do not by themselves establish that information flow. An authored tracking URL belongs to T7; borrowing host authority to disclose a secret belongs here.

Documents have no scripts or response-reading API. Local image loading does not compose remote URLs from file contents, SVG image-href loading is disabled, and the HTTP client disables automatic referrers and has no cookie store enabled. URL userinfo is refused on every hop, browser unsafe ports are blocked, and proxies are disabled. Request-capture tests check that GET requests carry no cookies, authorization, proxy authorization, or referrers. These controls do not establish browser sandbox equivalence. A server accepting other users' documents must separately prevent SSRF and isolate its own secrets; see [scope and assumptions](../security.md#scope-and-assumptions).

## Resource limits

`crates/markview-commonmark/src/limits.rs` owns the default parsing depth and HTML scope budgets. `crates/markview-layout/src/limits.rs` owns the work and byte allowances read from `LayoutOptions::limits` by math, highlighting, table layout and line breaking. The split preserves every default allowance.

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

### Document origin and mode

The [Trusted and Untrusted modes](../security.md#document-trust-modes) are represented by the `Trust` and `Origin` enums in `src/security.rs`. `src/app/interaction.rs` writes pasted text to a temporary file before opening it, while a web article is lowered into a semantic document held in its tab. A local path therefore cannot identify a Trusted entry.

Origin and mode travel with the reading session into navigation, image jobs, cache access, session restoration, and PDF export. Local Markdown links inherit the initiating mode; an Untrusted local link also needs read authorization, and web loads always enter Untrusted mode. Tab reuse includes origin identity; staging does not promote trust. Grants bind to the document source digest and are dropped on changed content and restoration. Target grants authorize resources without changing document mode. Verification belongs to the [follow-up plan](../security-verification.md#regression-checklist).

### Image source resolution

Implementation: `src/images/source.rs`.

An image carries an `ImageSource`: either `Resource(ResourceRef { address, base })`, where `base` is `ResourceBase::Document` or `ResourceBase::Vault`, or `Diagram(Diagram { language, source })`. The web front end stores each image as a `ResourceRef::document` whose address is already absolute against the page URL; a Mermaid block is a `Diagram`, not a resource. The document model performs no resolution, authorization or cache-key construction.

The resolver receives the document security context. `resolve` turns the semantic source into `Source::{File, Http, Data, Diagram}`, and `key` builds the application cache key used by image scheduling and `ImageSnapshot::keys`. A resource key starts with the address, prefixed by `vault:` for `ResourceBase::Vault`; in an Untrusted context it also hashes the resolution base (document path or workspace) together with `Security::cache_key()`. A diagram key is its language and source. Two pages therefore cannot share a relative or vault-relative resource merely because the address text matches, and changed authority cannot reuse an old entry.

Web relative URLs use the page URL; clipboard relative sources need user-selected files. Local documents use the path rules below, with `Security::check_file` before content reads. Explicitly selected clipboard files are reader-chosen resources; authored absolute or `file:` image sources remain refused.

```text
1. Reject an empty address.
2. Parse it as a URL. Accept http and https only. A data: URI is accepted here;
   content fetch still requires the data:image/ prefix and its size cap. There
   is no file: branch. A diagram never reaches this resolver.
3. Otherwise treat it as a path:
   a. Percent-decode.
   b. Reject if it is absolute, if its first component is a root, or, on
      Windows, if it is a prefix (C:foo) or rooted without a drive (\foo).
   c. Join it to the resource base: the document directory, or the workspace
      for ResourceBase::Vault. Canonicalize when the target exists, for alias
      deduplication and the identified permission target.
   d. Require Trusted mode or a grant for that file before reading it.
4. Do not confine the result to the document directory. `..` is allowed.
```

### Local-link classification

Implementation: `src/link.rs`, with the modal in `src/app/chrome/modal.rs` and the dispatch in `src/app/pointer.rs`.

The allowlist is a closed, reviewed list, not a category. Additions are a security change:

- **Opens in Markview**: `.md`, `.markdown`, `.mdown`.
- **Handed to the OS**: `.txt`; `png jpg jpeg gif webp bmp ico svg avif tiff tif heic`; `pdf epub mobi azw3 djvu cbz cbr xps oxps`; `mp3 m4a aac flac wav ogg oga opus wma aiff mid midi`; `mp4 m4v mkv webm mov avi wmv flv mpg mpeg 3gp ogv`.
- **Directories**: handed to the OS.
- **Everything else**: the three-button confirmation. This includes `.html`, `.htm`, `.xhtml`, `.ps`, `.eps`, `.swf`, `.chm`, `.jar`, `.rtf`, archives, and non-allowlisted executable, script, and installer extensions.

The modal offers **Open folder** (the default), **Open anyway**, and **Close**, and owns clicks, keys, and scrolling while open. See the [confirmation policy](../security.md#links-and-os-handlers).

`src/link.rs` canonicalizes existing targets before testing their extensions, so `note.txt` pointing to `payload.desktop` is classified as `.desktop`. If canonicalization fails, it retains the supplied path. Classification uses the extension, not Unix executable permission bits; an allowlisted extension is not made confirmation-only by setting an executable bit. HTTP, HTTPS, and mailto URLs go to the OS; `file:` URLs enter local classification, and other schemes are refused.

The document-mode policy requires authorization for an Untrusted link to local Markdown and preserves its Untrusted mode; Trusted local Markdown links retain Trusted mode. OS-handler allowlisting and confirmation remain the same in both modes.

### Network and font downloads

Implementation: `src/images.rs` (cap, notice state), `src/net.rs` (resolution, pinning, and the streaming download client), `src/web_page.rs` (explicit article fetch, UTF-8 decode and origin binding) with `crates/markview-webpage` (DOM extraction and semantic lowering; see [Web page extraction](webpage-extraction.md)), `src/images/cache.rs` (bounded disk cache) and `src/fonts.rs` (the explicit font download); the strip is drawn from `src/app/chrome.rs` and the catalogue from `src/app/font_panel/view.rs`, with `markview fonts` in `src/app/fonts_command.rs`.

`src/security.rs` supplies mode and target authority. Local files opened directly are Trusted, pasted text and web articles are Untrusted, and explicit requests authorize the identified origin and resolved address classes for their job.

1. By default, at most 128 distinct remote sources are admitted per document revision; Load all lifts this cap. The remainder render as placeholders naming the reason, so a headless `render` or `smoke` run cannot block on them.
2. The notice strip appears below the tab bar and offers Dismiss and Load all. Both answers belong to the tab and content revision they were chosen in: opening another document shows its own notice and starts capped again, and so does a reload. The exemption travels with the layout request instead of a shared flag, so it cannot leak into the next document laid out.
3. Image requests allow at most five redirect hops, 32 MiB per body, 15 s per hop and 5 s to connect. Resolved destinations are classified as Public, Private, Loopback, LinkLocal, or Invalid, including IPv4-mapped IPv6. Public destinations are allowed in both modes; Trusted local documents also allow Private (including carrier-grade NAT) and Loopback. Other local requests require exact origin/class grants. Invalid destinations, URL credentials, and browser unsafe ports are refused. Load all does not grant authority. The client disables proxies and pins only the checked addresses.
4. A fetched body is stored under `cache/images` beside `settings.toml`, keyed by the absolute URL and a SHA-256 partition of the receiving security context, bounded at 128 MiB with least-recently-used eviction, and installed by rename so a partial body is never served. Authority changes cannot reuse decoded pixels or another cache partition. A fresh entry needs no HTTP request; online reads still check DNS authorization; a stale one revalidates with the stored `ETag`/`Last-Modified`. `--offline` never calls the client but serves a cached body whether or not it is fresh, deliberately overriding `no-cache` and `must-revalidate` because there is no network to revalidate against.
5. A font family a stylesheet declares under `[[font-family]]` is fetched only by an explicit action on the reader's Fonts page or by `markview fonts download`. Both use a streaming async client with the same resolution and pinning as the image client, but authority scoped to each explicitly selected source or probe. Local redirects outside that origin require separate authority and therefore fail in the noninteractive job. Transfers stream to a file with no whole-request limit, a 5 s connect limit and a 60 s silence limit, and a `User-Agent` naming the reader, because a mirror may refuse an anonymous client. HTTPS is preferred; plain `http` is accepted because a mirror may serve only that, and the cost is transport privacy for a URL the user chose. A family's sources are mirrors ordered by a one-time latency measurement per host — the declared order only breaks ties — each a set of files or one archive; the container is recognized from its own leading bytes, extraction refuses directories, symlinks, hard links and any path outside the download directory, and is bounded at 2 GiB and 4096 members. A font file is capped at 64 MiB, an archive at 2 GiB, a declared `sha256` must match, and the body is verified as a font (every table record inside it, the tables a drawable face needs — outlines, or a color emoji face's `CBDT`/`sbix` bitmap strikes — and a nonempty character map) before it is renamed into place, so a partial transfer is never registered. A failed source's own files are removed before the next mirror runs. Nothing is cached by the image cache; the verified files in `fonts/` are their own cache. `--offline` refuses the job with a message. The directory is a personal resource: the reader, its export jobs, and the drawing subcommands load it by default, while the measurement modes and `--ignore-system-fonts` keep the configured set, so a download cannot change a pinned export.

Web-page loads use the same client, the document byte limit, and a 30 s download deadline. Extraction accepts UTF-8 HTML and limits parsed elements to 100,000 before lowering the article into a semantic document. `--offline` refuses web-page loads. No extracted article scripts execute.

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
