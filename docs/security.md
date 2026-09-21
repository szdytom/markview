# Security and threat model

Markview's product promise is that opening any file is safe. This page states what that promise covers, which parts of the system an untrusted document can reach, what has already been shown to break, and which risks are knowingly accepted. It owns every security-relevant design decision; [Architecture](architecture.md) owns the pipeline, resource boundaries, and snapshot ownership. A page that needs a security fact links here rather than restating it.

Status: revision 3, implemented. Every decision under [Policy decisions](#policy-decisions) and every structural mitigation under [Mitigations](#mitigations) is in the tree, with the code location named next to it. The verification work that is not code — fuzzing, concurrency models, memory measurement — is still open and marked as such.

## Summary

The three findings that drove revision 2 are resolved.

1. **The remotely triggerable abort is fixed.** `Reader::inlines` now draws a depth budget from a shared [`Limits`](#limits) value, exactly as `Reader::blocks` already did, and the 12 KB trigger is a regression test. The same edit removed the structural cause: no recursion or allowance in the pipeline is a locally invented constant any more.
2. **The image-path decision is a policy, not a boundary.** Absolute paths and `file:` URLs are refused; relative paths, including `../`, are allowed. Markview deliberately does not confine image paths to the document directory, because `..` is an ordinary way to reference a neighbouring directory and a local read has no exfiltration channel. The residual — a symlink inside the document directory still resolves anywhere — is recorded under [T8](#t8-symlink-and-time-of-checktime-of-use-races).
3. **Remote image loading is bounded and cannot reach the local network.** A document may fetch at most 128 distinct remote sources per revision; the remainder wait behind a notice strip the reader can act on. Every host is resolved first, private, loopback and link-local addresses are refused, and the surviving address is pinned so a rebind cannot slip past the check. A fetched body is kept in a bounded on-disk cache beside the user configuration, so later opens and `--offline` need no request; the cache adds no second client and no way around the address policy.

## Scope and assumptions

In scope: a user on an unsandboxed desktop opening a Markdown file from an untrusted source, including downloads, mail attachments, extracted archives, generated text, shared folders, a document watched with `pdf --watch`, and a second Markdown file reached by following a link.

Out of scope: physical access, kernel and GPU driver defects considered as defects in themselves (they appear only as [T10](#t10-gpu-and-driver-boundary)), social engineering in which the user installs or approves something, and supply-chain compromise of a dependency (tracked as [T4](#t4-memory-corruption-in-a-dependency)). The PDF exporter is in scope for the same parser and image budgets, and out of scope for conformance of the PDF it writes: a viewer's handling of the bytes is the viewer's boundary. Revision 2 claimed that work was handled with `cargo-deny`, `cargo-audit`, and `cargo-vet`; no such configuration exists in the repository today. That claim was wrong, and the gap is real and open.

One structural fact shapes everything below. Markview executes nothing from a document: the raw HTML subset is deliberately limited to semantics Markdown already expresses, and attributes such as `class` and `style` are never interpreted. There are therefore exactly two channels from document content to an effect outside the process:

- `open::that_detached`, reached from document-controlled links
- image I/O, reached from a document-controlled `img src`

the `pdf` subcommand writes a file to the path the user named on the command line, which is
the user's own choice and never document-controlled. It runs no window, no GPU,
and no link handler; it reuses the same parsing, image loading, and `Limits`
budgets as the reader, so a document cannot make the export reach anywhere the
reader could not. `--watch` only rewrites that same user-named path on later
saves; it adds no destination the document can choose.

Both are now closed to anything a policy has not already named. [T6](#t6-arbitrary-file-opened-by-the-operating-system) routes every document-controlled launch through `src/link.rs`, which is the only allowlist in the program; [T5](#t5-arbitrary-local-file-read) and [T7](#t7-server-side-request-forgery-and-network-beaconing) bound the two image channels. The rest of the system is a pure data-to-geometry pipeline, held to "does not crash, hang, or exhaust memory."

## Assets

| | Asset | Consequence of compromise |
| --- | --- | --- |
| A1 | Process availability | Loss of what the reader is reading, unsaved session state, an interrupted `--watch` session |
| A2 | Host confidentiality | Local file contents read or rendered |
| A3 | Host integrity | Arbitrary code execution |
| A4 | Network identity | Opening a document alone discloses IP address and online activity |
| A5 | Trust in the reader's own interface | Document content impersonating reader chrome or a security prompt |
| A6 | Clipboard and selected text | Injection through the paste path |

## Trust boundaries

```text
   .md bytes ──(B1)──► comrak ──► Document (pure data) ──(B2)──► Layout ──► Scene ──► GPU (B4)
      │                    │                                            ▲
      │                    ├─► fence info ──► syntect ────────────────┘
      │                    ├─► $...$ ───────► ratex ──────────────────┘
      │                    └─► img src ──┐
      │                                  ▼
      └────────────────────────► (B3) image source resolution ──► local file / HTTP / data:
                                           │
   link click ────────────────────► (B5) src/link.rs ──► OS handler

B1 untrusted bytes → pure data        B2 pure data → geometry (no side effects)
B3 document → filesystem and network  B4 process → driver (not fuzzable)
B5 document → OS execution
```

B3 and B5 carry all of the risk. B2 needs only the safety properties listed under [Security invariants](#security-invariants), which are engineering problems rather than policy problems, and which the shared [`Limits`](#limits) value now answers structurally.

## Attacker capabilities

| | Capability | Realistic setting |
| --- | --- | --- |
| K0 | Controls the `.md` bytes | The baseline; always assume it |
| K1 | Also controls other files reachable by relative path | Extracted archive, cloned repository, shared folder |
| K2 | Controls a remote server the document points at | Common |
| K3 | Controls a stylesheet the user loads | Depends on distribution |

K1 is easy to overlook. With the link policy in place it no longer yields code execution on a single click; it still yields a symlink surface under [T8](#t8-symlink-and-time-of-checktime-of-use-races).

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

`inlines` now takes a depth and, at `Limits::inline_depth`, collects the remaining subtree with an iterative walk and keeps it as plain text. Content is never lost, only its styling. The bound is 256, which costs about 128 KB of the 8 MiB stack: it exists to stop pathological input, not to restrict nested documents. `crates/markview-core/src/document/tests.rs` keeps the 12 KB input as a regression test.

### T2: hang or CPU exhaustion

**Bounded.** Four document-controlled hot spots had no work budget. Each now draws one from `Limits`, and each **degrades instead of failing**: the reader keeps the content and loses only the costly presentation.

| Hot spot | Bound |
| --- | --- |
| `crates/markview-core/src/highlight.rs`, syntect | A single line longer than `highlight_line_bytes` (64 KiB) is not handed to the regex engine at all, and only `highlight_bytes` (16 MiB) of code per layout pass is highlighted. The remainder renders uncolored. |
| `crates/markview-core/src/math.rs`, ratex | `math_formula_bytes` (256 KiB) per formula and `math_bytes` (8 MiB) per layout pass. A formula over budget renders through the existing math-error path. |
| `crates/markview-core/src/linebreak.rs`, `greedy` | The greedy fallback now observes the same `linebreak_evaluations` budget as the optimal pass, and always terminates: when the budget runs out it keeps the best candidate found, or advances one unit. |
| `crates/markview-core/src/layout/table.rs`, wide tables | `table_columns` (256), `table_rows` (16384), and `table_cells` (131072) truncate the grid before it is shaped. |

The defaults are chosen so that ordinary documents never reach them; a pasted 10K-character formula is well inside `math_formula_bytes` even when every character is three bytes wide, and a code-heavy document fits inside `highlight_bytes`.

One residual is honest and cannot be closed from here: ratex exposes no work budget, so the bound on a single formula is its byte length. A hostile formula just under 256 KiB can still take a long time. This is recorded under [Accepted and residual risks](#accepted-and-residual-risks) and is the reason the byte limits are the only lever available.

### T3: memory exhaustion

Unchanged. Decoded pixels are capped at 16 million pixels per image, roughly 64 MB of RGBA8, the decoded cache is capped at 256 MB, and four worker threads decode concurrently. The peak is therefore on the order of half a gigabyte before the source text, the comrak arena, and layout are counted. Memory keys hold the raw source string, so a `data:` URI costs roughly its own size in file bytes. This still needs measurement rather than speculation before any limit is chosen.

### T4: memory corruption in a dependency

Unchanged, and open. The workspace forbids `unsafe_code`, so every unsafe operation reachable from a document lives in a dependency: `image` for six raster decoders, `resvg` and `usvg` and `tiny-skia` for SVG and curve rasterization, `swash` and `parley` for font parsing and shaping, `syntect` for highlighting, and `ratex-*` for math.

Worth knowing when allocating effort: the `image` crate is already continuously fuzzed upstream in OSS-Fuzz, so byte-level raster fuzzing would largely repeat that work. The parts that are *not* covered upstream are Markview's own decode paths: the hand-written ICO entry scan in `src/images/decode.rs`, first-frame selection for APNG and animated WebP, the SVG path that rasterizes at a caller-supplied target size, and the premultiplied-to-straight alpha conversion. No OSS-Fuzz project for `resvg` or `usvg` was found, so SVG rendering is the least covered layer in the stack. No dependency-audit tooling is configured in the repository yet.

### T5: arbitrary local file read

**Policy decided.** `src/images/source.rs` refuses an absolute path and a `file:` URL, and treats everything else as a path relative to the document directory. `../` is allowed: it names another relative location, and a document that reads `../../etc/passwd` learns nothing it can report back, because Markview never concatenates file content into a URL.

Severity is bounded by that same absence of an exfiltration channel. The exposure is that a local sensitive file that happens to be an image is rendered, and that the error strings distinguish *missing*, *not a regular file*, and *not an image*, which yields a local existence-and-type oracle.

A canonical prefix check was considered and rejected. It would have to reject `../images/x.png`, which is an ordinary way for a document in a subdirectory to reference a sibling directory, and it buys nothing against the only stakeholder who can act on the read — the person already looking at the screen.

### T6: arbitrary file opened by the operating system

**Mitigated.** This remains the most severe design surface in the repository: with K1 it was code execution from a Markdown link plus one click. `src/link.rs` is now the single policy for what a document-controlled link may do, and no other module calls `open::that_detached` on document content.

A link is exactly one of four things:

| Class | Behavior |
| --- | --- |
| Markdown (`.md`, `.markdown`, `.mdown`) | Parsed in Markview, in a new tab. Never handed to the OS. |
| Inert local file | Handed to the OS with no prompt. The allowlist is `.txt`, the image formats `png jpg jpeg gif webp bmp ico svg avif tiff tif heic`, the fixed-layout documents and e-books `pdf epub mobi azw3 djvu cbz cbr xps oxps`, and common audio and video (`mp3 m4a aac flac wav ogg oga opus wma aiff mid midi mp4 m4v mkv webm mov avi wmv flv mpg mpeg 3gp ogv`). |
| Directory | Handed to the OS with no prompt: a file manager shows its contents, and does not run them. |
| Anything else | An in-app confirmation, defaulting to the safe action. |

The confirmation offers three answers: **Open folder** (the default, which reveals the file in the file manager and runs nothing), **Open anyway** (hands the file to the OS), and **Close**. It displays the canonical absolute path and the extension, never the link's label text, which the document controls. It is modal: while it is open it owns clicks, keys, and scrolling, so nothing behind it can be operated by accident.

The confirmation covers `.html`, `.ps`, `.eps`, `.swf`, `.chm`, `.jar`, `.rtf`, archives, and every executable, script, and installer type — `.desktop`, `.AppImage`, `.run`, `.lnk`, `.url`, `.bat`, `.cmd`, `.ps1`, `.vbs`, `.hta`, `.reg`, `.inf`, `.msc`, `.scr`, `.com`, `.pif`, `.cpl`, `.msi`, `.command`, `.app`, `.scpt`, `.workflow`, `.action`, `.dmg`, and any file with an executable bit. `.html` and `.svg` are the two allowlisted-adjacent cases that are knowingly dangerous; `.html` is deliberately **not** allowlisted.

Two mitigating facts remain true from revision 2. The extension test is applied to the canonicalized path, so a symlink named `note.txt` pointing at `payload.desktop` is classified as a `.desktop` file. And the other two `open::that_detached` call sites, in `src/app/interaction.rs`, are UI-initiated, opening the stylesheet directory and the settings file, and are not document-controlled.

The residual is the one revision 2 already named: user confirmation is the least reliable control in the chain, and it is now the *only* control for the executable types. That departure from revision 2's class D, which refused them outright, was taken deliberately and is recorded below.

### T7: server-side request forgery and network beaconing

**Mitigated.** Remote images remain enabled by default, but the magnitude is now bounded in three ways, all in `src/images/`.

1. **A per-revision cap.** At most `MAX_REMOTE_SOURCES` (128) distinct remote sources are requested. The remainder are not requested at all; they render as placeholders with the reason.
2. **A notice strip.** When anything is deferred, a strip below the tab bar reports it and offers **Dismiss** and **Load all**. Both answers are per tab and per content revision: Load all lifts the cap for the tab that asked, never for another document, and a reload asks again. The strip is informational, not a dialog.
3. **A private-address policy.** `pinned_client` in `src/images/net.rs` resolves the host itself, refuses any address that is loopback, private, link-local, carrier-grade NAT, unspecified, documentation, multicast, or broadcast, and then pins the surviving addresses with `resolve_to_addrs`, so the client cannot re-resolve behind the check. Redirects are followed manually, at most five hops, and every hop repeats the resolution and the check before a connection is made. It is the only HTTP client in the program.

The same client serves the one other remote resource a user can name: a font family a stylesheet declares under `[[font-family]]`. A stylesheet is a user-installed artifact, not document content, so its URLs carry the user's own trust; nothing fetches them on open, on install, or in `ss validate`. A job starts only when the reader asks for one on the Fonts page or runs `markview fonts download`, and is refused under `--offline`.

A download streams to disk rather than into memory, so the image client's fifteen-second whole-request limit would cut a large archive off; the download path therefore has its own async client with the same address policy, no whole-request limit, and a sixty-second silence limit instead. A family's sources are ordered by a one-time measurement of each distinct host's latency, so the declared order only breaks ties; each source may download a set of files or extract one archive, whose container is recognized from its own bytes. Extraction never writes outside the download directory, never takes a directory, symlink or hard link, and is bounded at 2 GiB and 4096 members; a font file is capped at 64 MiB, an archive at 2 GiB, and a declared `sha256` must match before the file is renamed into place.

Consequences that remain:

- A beacon still tells an attacker when a document was opened, and a unique URL per copy identifies which copy — up to the cap, and only after the reader has been shown the notice. A cached body is not fetched again, so a later open of the same document may disclose nothing at all.
- SSRF against internal services is blocked by the address policy unless the reader explicitly lifts the cap, and even then the policy still applies.
- Long background activity is bounded by the cap and the per-request timeouts (15 s total, 5 s to connect).
- A fetched body outlives the document in the image cache, whose header records the URL. That is an on-disk record of what the reader fetched, readable by anything that can read the user's files; it holds only bytes the reader's own documents caused to be fetched, is bounded at 128 MiB, and is deleted by removing the cache directory.

The unconditional private-address refusal breaks the legitimate case of a local document referencing a localhost service. Since Markview cannot tell whether a document is trustworthy, that case is refused and explained in the placeholder text. See [Accepted and residual risks](#accepted-and-residual-risks).

### T8: symlink and time-of-check/time-of-use races

**Accepted.** With containment removed from the image policy, a symlink inside the document directory can point anywhere and Markview will read through it. This is deliberate: the containment check that would have blocked it also blocked `../`, and it protects against an adversary who can already write to the document directory. On top of that, the image staleness stamp is still `(len, mtime)`, and a writer can preserve both.

### T9: concurrency state-machine races

Unchanged, and open. The application runs the layout worker, four image loaders, a file watcher, and the UI. `Worker` and `Images` each carry their own generation and ticket counters, and `Images::poll` briefly holds the decoded-pixels and demand locks together, which is a lock-order obligation that nothing currently documents or enforces. Coverage-guided fuzzing cannot reach these; they need a model such as `loom` or `shuttle`.

### T10: GPU and driver boundary

Unchanged. Malformed geometry reaches wgpu as validation errors, device loss, or driver defects. This layer has no useful feedback signal for a coverage-guided fuzzer, so the only economic approach is a headless smoke test on a software adapter that opens documents and renders frames without asserting on pixels. Raster images are resized to stay within the 8192-pixel texture dimension, while SVG rasterization is only bounded by a 16-million-pixel check, which is an asymmetry to note but not a defect by itself.

### T11: interface impersonation

Unchanged. The HTML subset interprets no `class` or `style`, so document content cannot adopt reader styling. Headings, link text, and image alt text remain attacker-controlled, which is a low risk recorded here so that it is not re-litigated. It is a P3 non-goal. The confirmation modal deliberately shows the canonical path rather than the link label, which is the one place where this risk could have been amplified.

## The shared `Limits` value

Revision 2's diagnosis was that the gap was structural: each bound was invented locally, so the ones that were forgotten are exactly where the failures were. That diagnosis is now the implementation.

`crates/markview-core/src/limits.rs` defines one `Limits` value from which every depth, iteration, and byte allowance is drawn. `document::parse` uses the default, and layout reads it from `LayoutOptions::limits`, so styling, math, highlighting, table layout, and line breaking all take their budget from the same place.

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

## Security invariants

| | Invariant | Status |
| --- | --- | --- |
| I1 | No input at or below the accepted size aborts, panics, hangs, or exhausts memory | T1 fixed with a regression test; T2 bounded; T3 still unmeasured |
| I2 | Every recursion has an explicit depth bound whose violation is a recoverable error | Satisfied: `Limits::inline_depth` and `Limits::block_depth`, both degrading to text |
| I3 | Decoded pixel totals, cache residency, and concurrent decodes are bounded per document | Partially: per-image and cache bounds exist, totals do not |
| I4 | Document content produces no effect outside the process unless the user confirms an action whose target the document cannot forge | Partially: T6 confirms with the canonical path, but confirmation is the only gate for executable types |
| I5 | Every filesystem path is confined to the document directory subtree | **Deliberately not enforced.** Relative-only is the policy; see [T5](#t5-arbitrary-local-file-read) |
| I6 | Rendering the same input is deterministic across runs and processes | Satisfied for image selection: `Prepared::images` is a `BTreeMap`, so the sole-image caption path is deterministic |
| I7 | Every output geometry value is finite and bounded in magnitude | Unverified; the stylesheet validates only finiteness and sign, so a finite but absurd size such as `1e30` passes |
| I8 | The only URL Markview opens comes from a single scheme allowlist, with no second path | Satisfied: `src/link.rs` is the only policy, and `openable_link` no longer exists as a second one |
| I9 | The number of remote requests and total bytes triggered by one document is bounded | Satisfied: 128 distinct sources per revision, 32 MiB per body, a private-address policy on every hop, and a 128 MiB disk cache with LRU eviction |
| I10 | Any path handed to the OS has had its executability judged for that platform and confirmed by the user | Partially: the inert allowlist is judged, everything else reaches the confirmation |

## Existing defenses

Worth keeping: `unsafe_code` forbidden workspace-wide; ratex pinned to an exact version; per-image pixel, byte, and cache bounds; the `--offline` switch; the `data:image/` prefix check; the SVG image-href resolver disabled; the texture-dimension clamp for raster images; the single link policy in `src/link.rs`; the single pinned HTTP client in `src/images/net.rs`; the bounded image cache in `src/images/cache.rs`; the per-revision remote cap; the explicit, verified, user-triggered font download in `src/fonts.rs`; and the shared `Limits` value with its tests.

## Policy decisions

### T5: image paths

Decision: only relative image paths are allowed. Absolute paths and `file:` URLs are refused.

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

### T6: local links

Decision: Markdown opens in the app; an inert allowlist is handed to the OS; a directory is handed to the OS; everything else requires confirmation.

Implementation: `src/link.rs`, with the modal in `src/app/chrome/modal.rs` and the dispatch in `src/app/pointer.rs`.

The allowlist is a closed, reviewed list, not a category. Additions are a security change:

- **Opens in Markview**: `.md`, `.markdown`, `.mdown`.
- **Handed to the OS**: `.txt`; `png jpg jpeg gif webp bmp ico svg avif tiff tif heic`; `pdf epub mobi azw3 djvu cbz cbr xps oxps`; `mp3 m4a aac flac wav ogg oga opus wma aiff mid midi`; `mp4 m4v mkv webm mov avi wmv flv mpg mpeg 3gp ogv`.
- **Directories**: handed to the OS.
- **Everything else**: the three-button confirmation. This includes `.html`, `.htm`, `.xhtml`, `.ps`, `.eps`, `.swf`, `.chm`, `.jar`, `.rtf`, archives, and all executable, script, and installer types.

The confirmation must be blocking, default to the safe action, offer no "remember this choice", and display the canonical absolute path rather than the link's label text.

### T7: network access

Decision: remote images remain enabled by default, with a per-revision cap, a notice strip, and an unconditional private-address policy.

Implementation: `src/images.rs` (cap, notice state), `src/images/net.rs` (resolution, pinning, and the streaming download client), `src/images/cache.rs` (bounded disk cache) and `src/fonts.rs` (the explicit font download); the strip is drawn from `src/app/chrome.rs` and the catalogue from `src/app/chrome/fonts.rs`, with `markview fonts` in `src/app/fonts_command.rs`.

1. At most 128 distinct remote sources are requested per document revision. The remainder render as placeholders naming the reason, so a headless `render` or `smoke-test` run cannot block on them.
2. The notice strip appears below the tab bar and offers Dismiss and Load all. Both answers belong to the tab and content revision they were chosen in: opening another document shows its own notice and starts capped again, and so does a reload. The exemption travels with the layout request instead of a shared flag, so it cannot leak into the next document laid out.
3. Loopback, private, link-local, carrier-grade NAT, unspecified, documentation, multicast, and broadcast addresses are refused on the initial URL and on every redirect hop, after resolution and before connecting.
4. A fetched body is stored under `cache/images` beside `settings.toml`, keyed by a hash of its absolute URL, bounded at 128 MiB with least-recently-used eviction, and installed by rename so a partial body is never served. A fresh entry needs no request; a stale one revalidates with the stored `ETag`/`Last-Modified`. `--offline` never calls the client but serves a cached body whether or not it is fresh, deliberately overriding `no-cache` and `must-revalidate` because there is no network to revalidate against.
5. A font family a stylesheet declares under `[[font-family]]` is fetched only by an explicit action on the reader's Fonts page or by `markview fonts download`. Both use one client with the same resolution, pinning and address policy, but streaming to a file: no whole-request limit, a 5 s connect limit and a 60 s silence limit, and a `User-Agent` naming the reader, because a mirror may refuse an anonymous client. HTTPS is preferred; plain `http` is accepted because a mirror may serve only that, and the cost is transport privacy for a URL the user chose. A family's sources are mirrors ordered by a one-time latency measurement per host — the declared order only breaks ties — each a set of files or one archive; the container is recognized from its own leading bytes, extraction refuses directories, symlinks, hard links and any path outside the download directory, and is bounded at 2 GiB and 4096 members. A font file is capped at 64 MiB, an archive at 2 GiB, a declared `sha256` must match, and the body is verified as a font (every table record inside it, the tables a drawable face needs — outlines, or a color emoji face's `CBDT`/`sbix` bitmap strikes — and a nonempty character map) before it is renamed into place, so a partial transfer is never registered. A failed source's own files are removed before the next mirror runs. Nothing is cached by the image cache; the verified files in `fonts/` are their own cache. `--offline` refuses the job with a message. The directory is a personal resource: only the reader's own layout loads it, while diagnostic modes, `--ignore-system-fonts`, and the reader's export jobs keep the configured set, so a download cannot change a reproducible export.

## Accepted and residual risks

| Risk | Why accepted |
| --- | --- |
| A4 under T7: remote images are fetched by default, so opening a document discloses the reader's address | Product decision; now bounded to 128 requests per revision, shown in a notice strip, and still disableable with `--offline` |
| T7: a fetched image is cached on disk with its absolute URL | The cache sits under the user's configuration directory, is bounded at 128 MiB with LRU eviction, holds only what the reader's own documents fetched, and is deleted by removing the directory |
| T6: executable types are confirmed rather than refused | Product decision. A mis-click on a confirmed `.desktop` file is still code execution; "Open folder" is the default so the safe answer is also the easiest |
| T6: `.svg` is allowlisted and a browser may execute script when it opens a `file://` SVG as a top-level document | Product decision; `.svg` is a common image and the risk is milder than `.html`, which is not allowlisted |
| T6: `.txt` and the document/audio/video allowlist are handed to the OS without a prompt | The handlers are viewers; the risk is the same class as an image decoder bug, which is already accepted under T4 |
| T6: links are not confined to the document directory | A clicked `.md` link may open any local Markdown file. It is a click-gated read with no exfiltration channel, and confining it would break ordinary relative links |
| T5: no containment, so a symlink inside the document directory can escape | Requires a local adversary with write access to the document directory; recorded under T8 |
| T8: forgeable image staleness stamp | As above; the reader only re-reads a file it already read |
| T2: ratex has no work budget, so a formula just under 256 KiB can still be slow | The only available lever is byte length; lowering it would reject formulas users legitimately write |
| T9: unverified lock-ordering obligation in `Images::poll` | To be covered by a concurrency model rather than by review |
| T10: driver defects | Out of scope as defects in themselves; covered only by a headless smoke test |
| T11: interface impersonation through headings, link text, and alt text | The HTML subset interprets no `class` or `style`, so the practical surface is small, and the confirmation shows the path instead of the label |
| No exfiltration channel for T5 | Markview never places file content into a URL, so a local read cannot be reported back to a remote party |

## Mitigations

Structural, in order of value:

1. ~~One `Limits` value threaded through parsing, styling, math, highlighting, and layout.~~ **Done**: `crates/markview-core/src/limits.rs`.
2. ~~Recursion replaced by an explicit stack, or given a budget, wherever it relies on input size.~~ **Done** for `Reader::inlines`; the greedy line breaker now has the same budget as the optimal pass.
3. A pinned font for layout instead of loading system fonts, which is both a reproducibility requirement for testing and a determinism requirement for I6. **Open.**
4. ~~The single URL allowlist for I8, and the containment check for I5.~~ **Done** for I8 in `src/link.rs`; I5 was deliberately dropped, see [T5](#t5-arbitrary-local-file-read).

Engineering, in order of cost:

| Step | Cost | Covers | Status |
| --- | --- | --- | --- |
| Run the test suite under `cargo careful` | Minutes | Undefined behavior in dependencies that the standard library can detect | Open |
| Dependency audit tooling (`cargo-deny`, `cargo-audit`, `cargo-vet`) | Minutes | T4 supply chain | Open, and previously misreported as done |
| ASAN-instrumented fuzzing of decode, SVG, and shaping | Hours | T4 | Open |
| `loom` or `shuttle` models of `Worker` and `Images` | Days | T9 | Open |
| `kani` proofs for the integer and slice logic in `html`, image-source resolution, and the link allowlist | Days | Boundary errors in I8 and the T6 classes. Not applicable to the float-heavy line breaker, where Kani's support is poor | Open |
| `miri` over the dependency-free logic modules | Days | Requires extracting those modules, since the font stack is FFI | Open |

## Verification plan

Each invariant maps to a harness with an oracle, rather than to "no crash". Crash-only fuzzing is weak here: a 15-day run of 30 billion inputs against a CommonMark parser produced 594 new-interesting inputs and no bugs, which is the expected outcome for a pure parser under byte-level mutation. The value is in the deeper stages and in the oracles.

| Target | Harness | Oracle | Status |
| --- | --- | --- | --- |
| I1, I2 | Parse and layout, plus a dedicated line-breaker harness over unit vectors | No abort, no panic, a recorded recursion bound, a time budget | Regression tests exist for the 12 KB abort and for greedy termination; fuzzing open |
| I3, I9 | Layout with a synthetic image snapshot; decode in isolation | A counting global allocator asserting peak allocation against input length; a request counter | Request counting is unit-tested; the allocator oracle is open |
| I4, I8 | Source resolution over `(link, document path)` | The T6 class tables | Implemented as `src/link.rs` tests |
| I5 | Source resolution over `(src, document path)` | The relative-only predicate | Implemented as `src/images/tests.rs` tests |
| I6 | Layout twice in one process and across processes | Field-by-field equality after serialization | Open |
| I7 | Layout over generated numeric options | Finiteness and magnitude bounds on every geometry value | Open |
| T4 | Decode, SVG rasterization, and shaping | ASAN cleanliness | Open |
| T2 | Highlight, math, and line breaking, each with a timeout | Wall-clock budget per input | Budget unit tests exist; timeout harness open |
| T7 | Address policy and cap | An address class table and a per-revision counter | Implemented as `src/images/tests.rs` tests |
| Differential | Cached versus uncached layout; progressive prefix versus final snapshot | Field-by-field equality. Both properties are already asserted in unit tests and should be enforced during fuzzing | Partially implemented |

Input generation should be structured rather than byte-level. Comrak exposes an `arbitrary` feature that derives `Arbitrary` for its option types, which makes randomized configuration free, and the comrak repository already carries a fuzz suite whose targets include a complexity-focused one and a `sourcepos`-focused one that exercises the same positions `Reader::range` depends on. See [Appendix B](#appendix-b-existing-fuzzing-assets).

## Non-goals

- Making a hostile document safe to *act on*. Only the reader is hardened, not the user's judgement.
- Confining image and link paths to the document directory. Relative-only is the boundary; the residual is recorded under [T8](#t8-symlink-and-time-of-checktime-of-use-races).
- Defending against a local attacker who can already write to the document directory, beyond the recorded residuals.
- Treating driver defects, or wgpu validation failures caused by driver behavior rather than by Markview's inputs, as findings.
- Restricting which content a document may *display*. Styling attributes are not interpreted, and impersonation is a recorded P3 risk.

## Appendix A: confirmed findings

**T1 — stack overflow in `Reader::inlines`.** Reproduced outside the application, on a thread with the same 8 MiB stack size that `Worker` requests in `src/worker.rs`, and now fixed.

```sh
python3 -c "n=6000; print('*'*n + 'a' + '*'*n, end='')" > nested.md
```

The file is 12001 bytes. Observations against the current tree:

| Input | Result |
| --- | --- |
| 8001 bytes (`n = 4000`) | Parses, one block |
| 12001 bytes (`n = 6000`) | Aborts the debug test binary; rendered by the fixed tree |
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

## Appendix B: existing fuzzing assets

Nothing off the shelf generates Markdown input in Rust, but the following are worth reusing rather than rebuilding.

| Asset | Use |
| --- | --- |
| [comrak's own fuzz suite](https://github.com/kivikakk/comrak/blob/master/fuzz/Cargo.toml) | Nine targets covering parse, CommonMark, GFM, source positions, footnotes, all-options, CLI defaults, and a complexity-focused target. Markview depends on the same version, so it adapts with a path change. |
| comrak's `arbitrary` feature | Derives `Arbitrary` for the option types, so randomized configuration needs no generator. |
| [pulldown-cmark's `pandoc` target](https://github.com/pulldown-cmark/pulldown-cmark/blob/master/fuzz/fuzz_targets/pandoc.rs) | A template for differential fuzzing. The applicable differential here is comrak's HTML output against Markview's block extraction. |
| [tree-crasher](https://github.com/langston-barrett/tree-crasher) with [tree-sitter-markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown) | Grammar-aware mutation without instrumentation. tree-crasher ships C, CSS, JavaScript, Regex, Rust, SQL, TypeScript, HTML, OpenSCAD, and Ruby, but not Markdown; the grammar exists, so adding a `tree-crasher-markdown` crate is small. Its HTML front end applies to the raw HTML subset directly. |
| [codec-corpus](https://docs.rs/codec-corpus) | Image test corpora, including PngSuite, as a development dependency. |
| [OSS-Fuzz: cmark](https://github.com/google/oss-fuzz/tree/master/projects/cmark) and [md4c](https://github.com/google/oss-fuzz/tree/master/projects/md4c) | Reference engineering for Markdown fuzzing, including corpus and dictionary layout. |
| [OSS-Fuzz: image-rs](https://github.com/google/oss-fuzz/tree/master/projects/image-rs) | Confirms the `image` crate is fuzzed upstream; do not duplicate it. |
| [librsvg's OSS-Fuzz work](https://gitlab.gnome.org/GNOME/librsvg/-/work_items/1096) | SVG seed corpus and a render-focused target. No equivalent project was found for `resvg` or `usvg`. |
| CommonMark and GFM spec suites, KaTeX test cases | Seeds for parsing and math. Correctness corpora, not crash corpora; pair them with generated extremes. |
| [bolero](https://github.com/camshaft/bolero) | One harness that runs as a coverage-guided fuzzer, a property test, or a Kani proof. |
| [aretext's report](https://devnonsense.com/posts/aretext-markdown-fuzz-test/) | The expectation-setting data point cited under [Verification plan](#verification-plan). |
