# Internal API property testing

Property tests run as ordinary Rust unit tests using Proptest. Keep them in a
`#[cfg(test)] mod proptests` next to their owning API; longer modules use
`module/proptests.rs`. Private access comes from the test child module, without
expanding production visibility. The dependency is development-only in sixteen
workspace packages, with `std` enabled and the default fork, timeout and bit-set features
disabled. Proptest 1.12 requires Rust 1.88, below the workspace's 1.96 MSRV.
The icon parser is also included by `xtask`; its property module is attached
at the icon crate root so that reuse does not import a development dependency
or change test-module resolution in the including crate.

## Run and reproduce

From the repository root:

```sh
cargo test --locked -p markview-geometry -p markview-document -p markview-layout -p markview-ui-core -p markview-interaction -p markview-commonmark -p markview-i18n -p markview -p markview-web -p markview-webpage -p markview-icon -p markview-render -p markview-pdf -p markview-platform -p markview-image --lib proptests::
PROPTEST_CASES=4096 cargo test --locked -p markview-geometry -p markview-document -p markview-layout -p markview-ui-core -p markview-interaction -p markview-commonmark -p markview-i18n -p markview -p markview-web -p markview-webpage -p markview-icon -p markview-render -p markview-pdf -p markview-platform -p markview-image --lib proptests::
cargo test --locked -p xtask proptests::
PROPTEST_CASES=4096 cargo test --locked -p xtask proptests::
```

`xtask` is a binary package and runs separately from the fourteen libraries.
Use a longer test name or one `-p` argument to isolate a failure. Native build
prerequisites are in the [development guide](development.md). These local API
fixtures need no system fonts, GPU device, OS events, network or sleeps.
PDF fixtures export real documents and inspect them with `lopdf`; their glyphs
come from a committed font subset and explicit cluster geometry.
The scroll and application animation tests pass explicit times derived from one
clock origin. Disk-cache fixtures use isolated temporary directories; eviction
uses distinct explicit file timestamps. The cache protocol fixtures use a local
Tokio runtime for disk work and immediate synthetic responses, without sockets
or assertions about wall-clock freshness.

Default configuration supplies 256 cases and a finite shrinking budget. All
strategies bound collection lengths, text sizes and operation sequences. No
property overrides `cases`, so `PROPTEST_CASES` expands every randomized test,
including the directed 64 KiB search tests. Those tests shrink padding within
65520–65544 bytes rather than losing the boundary condition. There are no fixed
exploration seeds or rejection filters.

Proptest prints the reduced parameters and saves replay seeds in the owning
crate's `proptest-regressions/` directory, mirroring the source module path.
Keep these files in Git. A failure can be reproduced with the persisted seed,
or rerun an exploratory run with `PROPTEST_RNG_SEED` set to its recorded
numeric RNG seed using the same Proptest version and strategies. The persisted
`cc` hash is a replay record, not the numeric value for that environment variable. A reduced input is a counterexample found by shrinking,
not a claim of global minimality. Diagnose the first divergent step and retain
an ordinary concrete regression test for a confirmed production defect; a seed
alone can change meaning when a generator changes. Seeds from model mistakes
are useful too, with their diagnosis recorded in the change evidence.

CI runs these separately in `pbt.yml` on Linux, Windows and macOS; regular
workspace CI tests skip `proptests::` to avoid duplicate runs. Failed jobs
upload `**/proptest-regressions/**` as `proptest-regressions-OS` for 14 days;
the Actions log retains assertions and reduced inputs. Download the artifact,
copy the files back to their crate paths and rerun the focused test. CI does not
write seeds back to the branch. A local saved failure has verified the path;
remote upload requires an actual CI run.

## Author a behavioral contract

Before writing a generator, identify the existing documentation, specification
or caller dependency that justifies the guarantee. Write a short checklist:
inputs and state preconditions, observable outputs, allowed fallbacks, properties
and their oracle, and boundaries still outside the model. Stable guarantees
belong in English Rust documentation at the API, rather than a separate contract
registry. Do not turn incidental implementation choices into commitments.

Generate semantic inputs directly. Construct valid identity, encoding and state
relationships, retaining them under shrinking. Include empty, singleton,
adjacent, duplicate, exact-threshold and just-over-threshold cases deliberately.
Use a small ordinary test where a category must be visited on every run. Separate
input domain from sample budget: lowering cases must not remove a category.

For numeric tests, declare bounds that prevent intermediate overflow. Small
integers and binary fractions permit exact checks; general floats need an error
bound stated in units and tied to operations and magnitude. Merely accepting
finite inputs does not prove intermediate arithmetic is safe.

Use a simpler oracle: prefix tables, a set plus suffix enumeration, a field list,
small exhaustive partitions or a sequence of requested destinations. Do not use
the tested function to calculate expected values, or rely on paired round trips
that could hide compensating errors. Stateful tests start fresh per case and
check after each operation; include repeated cancellation, empty flushes and
release after removal. Keep the model smaller than the implementation.

## Review by behavior

Read the purpose and contract first, and independently describe the API's inputs,
states and observable guarantees. Review the contract itself: passing tests do
not authorize a semantic change. Inspect generator domains and shrinking, then
check that the oracle does not reuse the core algorithm. Run existing properties
and concrete regressions; investigate a specific uncertainty by changing the
input distribution, adding a sequence or enumerating a small domain.

Classify each counterexample as an implementation defect, ambiguous contract,
model error or generator error before changing code. Read the implementation and
callers to explain the result and locate defects. Resolve clearly established
bugs directly; leave disputed semantics explicit for a human decision while
continuing independent work. Never filter a disagreement out of the generator.

The original coverage expansion recorded defects before changing production code.
Its fixes enable ten of the original eleven regressions in ordinary CI; the
upstream Comrak front-matter defect (PBT-003) and the recorded raw HTML
whitespace defect (PBT-013) remain unresolved; PBT-012 is fixed. Keep an
unresolved defect's reduced input, diagnosis and ignored reproducer with its bug
ID. An ignored reproducer is an unresolved contract, not passing coverage.

Record the behavior hypothesis, domain, oracle, command, result and limitations
in the change evidence. Optional fault injection belongs in an isolated temporary
copy and must be cleaned up. It is useful when it answers a concrete question;
there is no mutation count or score gate. Successful exploration is evidence
within its domain, not a proof or a requirement to find a bug.

## Examples and boundaries

| Boundary and tests | Model and limits |
| --- | --- |
| [Encoding positions](../../crates/markview-layout/src/source/proptests.rs) | UTF-8/UTF-16 prefix tables independently check every interior and beyond-end offset. Encoding boundaries, not grapheme boundaries. |
| [Link identity](../../crates/markview-document/src/proptests.rs) | Explicit address comparison and semantic cache hashing versus address-plus-occurrence-ID identity; independent allocations of the same address with the same ID agree, while `with_occurrence` preserves address sharing. |
| [Message scheduling](../../crates/markview-ui-core/src/component/proptests.rs) | Ordered payload lists, maximum invalidation, map preservation and regrouped flush batches; guarantees only the simple recorder component. |
| [Pan feed](../../crates/markview-interaction/src/pan/proptests.rs) | Active gesture intervals and accumulated deltas; exact binary fractions and bounded float error, no native adapter. |
| [Rectangles](../../crates/markview-geometry/src/proptests.rs) | Integer interval model, positive-area intersections, inclusive hit edges, degenerate boxes and bounded floats. NaN, infinity and overflowing extents are outside the caller domain. |
| [Heading anchors](../../crates/markview-document/src/slug/proptests.rs) | Explicit Unicode token semantics and a reserved-name set with explicit reservations, membership checks and suffix enumeration; no claim of compatibility with every external slug implementation. |
| [Reading mapping](../../crates/markview-layout/src/layout/mapping/proptests.rs) | Declared ordinary/atomic segments with all ordered start/end pairs, byte origins, typed footnote definitions/numbers/occurrences, half-open notes and span-link priority. Fixtures bypass shaping; note ranges are nonoverlapping. |
| [Image resources](../../crates/markview-layout/src/image/proptests.rs) | Complete versioned maps (including snapshot key sets), shared snapshot views and one consumable latest frame, demand joins, bounded sizes and independent attribute precedence. Sequential behavior only. |
| [Widgets](../../crates/markview-ui-core/src/widget/proptests.rs) | Ordered hit candidates, minimum touch distance, cyclic eligible identities, capture across removal/disable/movement/message changes, and release at the moved position. Unique IDs are required; equal-distance touch ordering is deliberately unspecified. |
| [Scroll](../../crates/markview-interaction/src/scroll/proptests.rs) | Requested destination versus displayed offset under partial/complete bounds, actual intermediate `Scroll` frames and active state, pending-target animation restart, endpoints and cancellation. Continuous wheel momentum/prediction is outside this phase. |
| [Microtype](../../crates/markview-layout/src/microtype/proptests.rs) | Capacity and direction constraints plus independently summed gains, including unfillable requests. No claim that every target is feasible. |
| [Line breaking](../../crates/markview-layout/src/linebreak/proptests.rs) | Ordered content conservation, trimmed glue, visible widths/hyphens, budget fallback and exhaustive short legal partitions that must take the normal path. No general optimality proof; fallback may advance at nonoptional boundaries. |
| [Semantic search](../../crates/markview-layout/src/search/proptests.rs) | Builder-created paragraph/heading/code/block-cell table/list/nested-disclosure/definition-list/figure fields with an independent field list, revision and typed identity checks, nonoverlapping literal scans, ASCII folding, chunk boundaries and cancellation. Whole-word checks cover stability/subset behavior; full ICU segmentation and Unicode case folding proofs are deferred. |
| [Document construction](../../crates/markview-document/src/build/proptests.rs) | Exact and one-under block/text/depth/table-cell budgets, structural paths, block visits and disclosure ancestry across eight container variants, with a separately accumulated field list. Fixtures are bounded chains and grouped table rows; branching and heading indexes have a separate model below. |
| [Semantic source maps](../../crates/markview-document/src/source/proptests.rs) | Disjoint field tables check all byte positions and semantic ranges, transformed spans return full covers, and validation checks source bounds and UTF-8 boundaries in blocks, spans and inline covers. An enabled regression checks reverse lookups after an overlapping inner span ends. |
| [Text input](../../crates/markview-layout/src/text_input/proptests.rs) | Known text/control token pairs, snapshot history after each append/replace/undo/redo/reset/select/IME-cancel operation, and prefix-sum scalar/UTF-16 surrounding deletion. Uses committed font subsets. Timing-based edit grouping, arbitrary cursor motion and pointer geometry are outside these properties; exposed combining marks check grapheme expansion, selection direction and undo/redo. |
| [Inertia](../../crates/markview-interaction/src/motion/proptests.rs) | Frame partition invariance, integrated travel versus velocity loss, speed caps, zero-time samples and release expiry. Bounded velocities and at most 100 ms of coasting avoid overflow and the low-speed stop branch. |
| [Style rules](../../crates/markview-layout/src/style/proptests.rs) | Explicit media-name membership checks OR filters after device/orientation changes through parsing and resolution; ordered lists and membership lists check condition chains and sets within `MAX_CHAIN`. Arbitrary cascading and chains beyond capacity are outside this model. |
| [Colors](../../crates/markview-document/src/color.rs) and [numbering](../../crates/markview-layout/src/style/numbering.rs) | Byte channels and normalized error bounds, malformed hex spellings, full-range signed decimal values, explicit small Roman/alphabetic tables and circled-number fallback. Other numeral systems remain delegated to the numeral library. |
| [Markdown parsing](../../crates/markview-commonmark/src/proptests.rs) | Shrinkable Unicode edit sequences compare reparse with a full parse, including normalized source maps, semantic keys and explicit link/reference occurrences. Directed leaf edits require the fast path and retain unaffected block ids; prefix cuts visit every interior byte, including UTF-8 interiors, under all three line endings. Reference/note definitions after a cut and verbatim front-matter payloads have directed generators. Obsidian wiki links check Unicode labels, percent escaping, code exclusion and heading fragments; callout aliases check folding, titles, body styles and reparsing across three line endings. EOF front matter with a delimiter-like payload has an unresolved reproducer. |
| [Branching document indexes](../../crates/markview-document/src/index/proptests.rs) | Independently declared paths, field ordering keys and disclosure ancestry over bounded heading/details/quote/figure/list trees, including empty branches, duplicate heading fragments and cancellation before traversal. Other container variants have chain fixtures above, not arbitrary branching fixtures. |
| [Document collectors](../../crates/markview-document/src/build/proptests.rs) and [link origins](../../crates/markview-document/src/proptests.rs) | Independently accumulated inventories preserve duplicate document/vault/diagram image specs across nine container categories, including callout titles and front-matter image suppression. Ordered code records check both collector APIs and existing output preservation; declared block lists check visits across sibling branches. Link origins use an explicit projected ID list with adjacent deduplication, anchor omission and explicit-link precedence over footnote references. |
| [Typed anchor indexes](../../crates/markview-document/src/index/proptests.rs) | Independent first-registration maps check fragment/footnote-definition/reference namespaces, distinct occurrences and nested disclosure/front-matter ancestry. Anchor-bearing fields include summaries, headings, block attributes, figure/table captions, definition terms and callout titles. Every case includes same-label typed targets at different depths; an unknown occurrence must not alias a known reference. |
| [Reading projection and cache identity](../../crates/markview-document/src/proptests.rs) | Explicit inline projection pairs check mixed text/alt/formula/reference/break/anchor fields. Cache keys must survive bijective link/reference occurrence renaming, shifted block identities and a new revision; this is distinct from `Block` equality, which keeps footnote reference occurrences. |
| [Pagination](../../crates/markview-layout/src/paginate/proptests.rs) | Declared disjoint bands constrain order/conservation, the existing half-pixel page allowance, fragment edges, anchors, heading/body pairing, widows/orphans and keep-together. Lone-image paragraphs up to four page heights stay whole and fit; wide tables use one scale derived from maximum horizontal stretch, bounded below by one quarter, without dropping row bands or anchors. Rectangles stand in for table rows; actual table shaping, image captions, bands that exceed the minimum-scale capacity and retained flattening combinations remain separate. |
| [Page templates](../../crates/markview-layout/src/paginate/furniture/proptests.rs) | A token list declares substituted text and numeric runs independently, including adjacent placeholders and braces inside replacement values. Unknown/unclosed placeholders are rejected. This checks expansion and validation, not shaped margin geometry. |
| [Cache rebinding](../../crates/markview-layout/src/layout/cache/proptests.rs) | Logical occurrence lists, including unpainted runs, constrain simultaneous id permutations, special links, footnote reference anchors, unchanged shared geometry and no-op sharing. A real layout engine checks cached paragraphs through repeated occurrence changes. Real figure cache hits preserve body and caption occurrences. |
| [Selection gestures](../../crates/markview-interaction/src/proptests.rs) | Integer time/distance episodes check click cycling; ordered anchor/focus pairs and active press intervals check shift extension, missing hits, drag latching, matching release targets and repeated clears. ASCII word and block drags retain a fixed base across reversed endpoints and blocks with descending ids. Tests supply explicit geometry fields, not native pointer events; Unicode word segmentation and atomic boxes remain outside this model. |
| [Localization](../../crates/markview-i18n/src/proptests.rs) | Declared placeholder names check first-appearance deduplication; ordered string maps check Unicode/control escaping and flat TOML validation. Generated methods are parsed as Rust and their parameter lists checked independently; extra parameters must be rejected. These tests do not compile and execute arbitrary macro expansions. Unmatched braces before placeholders and reserved Rust identifiers, including Rust 2024 `gen`, are rejected. |
| [Scene geometry](../../crates/markview-layout/src/scene/proptests.rs) | Scroll fractions, cross-axis paint extents, full-track thumbs, proportional corner/edge fitting and integer viewport origins. Pointer/scroll tolerances are bounded in pixels and by the scroll/travel ratio. Nested nonglyph draws are reconstructed at their translated origin to check all their fields. Glyph/math drawing, general float overflow and retained disclosure flow are outside this model. |
| [Retained disclosure flow](../../crates/markview-layout/src/scene/flow/proptests.rs) | Bounded stacked trees use independent visible subtree heights and prefix positions under declared states, overrides and force-open. Checks cover visible geometry/links/anchors, box resizing, marker rotation, flattening, following blocks and retained identity/text. Synthetic parallel regions share the same origin through preceding disclosure changes and an enclosing scene's minimum height. Their bridges use the production callers' unconstrained minimum; nested table-cell disclosures, footnote baselines and general clipping remain separate. |
| [Text hit testing and selection geometry](../../crates/markview-layout/src/text/geometry/proptests.rs) | Explicit ASCII/Greek/CJK/emoji/combining/ZWJ tokens supply independent byte boundaries. Enumerated carets model nearest hits under RTL, atomicity and clamped pan; equal-distance ties allow either nearest edge. Logical selected parts are formed before half-pixel clipping, including reversed/stale selections and whole atomic boxes. Bounded division allows 1e-4 layout pixel error. Fixtures bypass shaping and have one cluster; cross-line ordering, mixed automatic spacing and arbitrary grapheme grammars remain separate. |
| [Document hover and panning](../../crates/markview-interaction/src/document/proptests.rs) | Independent scalar regions check viewport/overflow clipping, text/link/image cursor priority, held-pointer suppression and owned link identities. Half-pixel probes exclude zero-area edge contacts. Integer pan sequences model one clamped overflow offset, unrelated-map preservation and scrollbar hit routing. Fixtures bypass shaping and native events. |
| [Outline collapse](../../src/state/outline/proptests.rs) | Independently enumerated descendant intervals and collapse flags check both initial modes and every toggle, including leaves and absent rows. These are original outline indices, not displayed row ordinals. |
| [Application animation](../../src/state/motion/proptests.rs) and [row slides](../../src/state/rows/proptests.rs) | Bounded scalar/time samples check endpoints, monotonicity, scalar-unit error, option completion and linear fading. Sorted unique row identities check retained slots, nonoverlapping visible bands, rapid retargeting and final target order. Retained band heights stay continuous across generated retarget sequences. No zero-duration tween contract is asserted; callers use positive durations. |
| [Tab strip](../../src/app/tab_strip/proptests.rs) | Width budgets, proportional shrinking, independent prefix positions, bounded scroll/reveal, midpoint insertion and list-based drop permutations constrain geometry and drag latching. Width/minimum inputs are bounded integers; tab-label font measurement and slide paint interpolation remain separate. |
| [Application resource authority](../../src/security/proptests.rs) | Numeric prefix partitions and their edges check IPv4, IPv6 and mapped addresses against Markview's declared policy. Grant/revoke/content-change sequences check an independent permission matrix, exact file/image sources, URL origin equivalence and invalid-address denial. These are local policy checks, not DNS/network tests or an exhaustive IANA policy audit. |
| [Image disk cache](../../src/images/cache/proptests.rs) | Explicit Unix times and redirect caps check freshness and partial 304 metadata replacement, including retained max-age precedence over Expires-only updates. Freshness provenance survives disk persistence; legacy entries without it are refetched. A body/target/validator state model checks offline reads, conditional revalidation, changed targets, missing entries and uncacheable replacement/removal with synthetic responses. Disk entries preserve exact bytes and reject truncation/other security namespaces. Distinct explicit timestamps and stored-file sizes model oldest-prefix eviction under a byte budget. Concurrent races and real HTTP remain outside these fixtures. |
| [Locale use](../../src/lang/proptests.rs) | A locale/script table checks case and separator normalization, explicit script precedence and unsupported-language fallback; real generated locale methods preserve arbitrary argument text. No host locale lookup or malformed-tag grammar is asserted. |
| [Request policies](../../src/net/proptests.rs) | Repeated header fields, directive case/quoting and wildcard `Vary` use explicit flags; an enumerated Gregorian calendar checks date parsing through response headers. Redirect storage/expiry constraints use set intersection and minimum absolute expiry. Built requests check URL-scoped validators without sending them; bounded reads check exact bodies and named size errors. Invalid calendar days are rejected through response headers. DNS, streaming and other malformed HTTP-date grammar remain separate. |
| [Preferences](../../src/settings/proptests.rs) | Independent numeric/list models check sidebar, scroll-speed and per-role font updates. Every numeric setting is checked at inclusive endpoints, outside values and nonfinite floats. Generated TOML preferences preserve values and the session/single-instance load rule; layout options preserve dialect and CJK choices with an independently bounded reading width. General stylesheet configuration merging and export settings remain separate. |
| [Preference transactions](../../src/settings/store/proptests.rs) | Eleven reader-field values, pending masks and a separate export value model local edits, external files, reloads, resets, deletes and flushes. Invalid files preserve the last good state; pending reader/export choices survive valid merges, while unknown fields/comments survive writes. Initially invalid UTF-8 is backed up byte-for-byte exactly once before replacement. Separate pending masks check whole theme, dialect, font-family and language groups through external reload/flush, explicit/follow-system choices and clearing optional values. Competing processes remain separate. |
| [Saved sessions](../../src/app/session/proptests.rs) | Generated files check version rejection and legacy structural-field migration while preserving search text, paths and scroll fallbacks. Incomplete snapshots defer fallback restoration; complete snapshots clamp, consume and clear the target exactly once. Existing integration tests cover semantic positions across real reflow; this fixture does not duplicate that shaping pipeline or native session ownership. |
| [Image source resolution](../../src/images/source/proptests.rs) | Temporary document/vault roots check literal and percent-encoded local paths. Explicit URL examples model relative, parent, root and authority references under a web origin, including unsupported-scheme rejection. Base64 and percent-encoded image data URIs preserve arbitrary bounded bytes. Symlinks, clipboard grants, remote fetching and image decoding remain separate. |
| [Browser publication](../../crates/markview-web/src/state/proptests.rs) and [stylesheets](../../crates/markview-web/src/stylesheets/proptests.rs) | Native state sequences check prefix retention, source allocation identity, pass invalidation, revision wraparound and UTF-16 selection-length caching. A small registry and ordered selection list check validation before replacement, rejected-selection atomicity, leftmost style precedence and default Arc reuse. Publication fixtures have no held selection; real geometry rebasing remains covered by existing integration tests rather than this model. |
| [Browser image delivery](../../crates/markview-web/src/images/proptests.rs) | An independent request/result map checks generation strings, stale/unknown/duplicate completions, invalid dimensions/RGBA lengths, semantic key maps, source order and old pixel views. Scalar rectangles model visible-first/nearest-vertical-distance scheduling through progressive prefixes and changed passes. Half-pixel viewport edges exclude zero-area contacts; browser events, nested clips and horizontal overflow are separate. |
| [Host font registration](../../crates/markview-web/src/fonts/proptests.rs) | Sequences of bundled OTF faces and deliberately invalid blobs check all-or-nothing replacement, the failing face index, unique configuration identity, explicit host-only lookup and the final family set. Empty configurations retain their old view after replacement. WOFF decoding itself uses existing font-format examples; no operating-system font set or browser installation is needed. |
| [HTML extraction and metadata](../../crates/markview-webpage/src/lower/proptests.rs) | A character/style/link model checks escaped literal Unicode and punctuation through direct DOM lowering and complete Readability extraction. Independent split/join whitespace models retain nonbreaking spaces; metadata checks display normalization, raw fields, relative URLs, canonical precedence and diagnostic order. Exact budgets include synthetic title/source text. Explicit column partitions check table group/cell order, spans and row/column/capacity limits; a blocked-cell set models colliding-span fallback. Single-column rowspans use independently declared occupied prefixes, including zero/overlong spans, group boundaries, empty rows and alignment conflicts. Mixed colspan/rowspan collision policy, arbitrary malformed HTML, formulas and Readability scoring remain separate. |
| [SVG icons](../../crates/markview-icon/src/svg/proptests.rs) | An independent affine calculation checks every cubic control/end point, nested transforms, viewbox translation, unit-box normalization and fill/stroke figures. Power-of-two viewport sizes keep the bounded coordinate model exact. Square-size validation and visible-figure counts reject unsupported/empty icons. Arbitrary SVG grammar, clipping and stroke anisotropy remain separate. |
| [Render preparation](../../crates/markview-render/src/paint/proptests.rs) and [color glyphs](../../crates/markview-render/src/raster/color/proptests.rs) | Independent device-grid/nearest-edge inequalities check image and formula-rule snapping, signed half-pixel ties and one-pixel minimums across eight DPRs with a 1e-3 device pixel allowance. Exhaustive candidate channels model nearest straight-alpha conversion, saturation, opacity and preserved alpha. These functions need no GPU; vertex generation, atlases, shader output and color profiles remain separate. |
| [PDF destinations](../../crates/markview-pdf/src/link/proptests.rs), [painting](../../crates/markview-pdf/src/paint/proptests.rs) and [font axes](../../crates/markview-pdf/src/text/proptests.rs) | Real serialized annotations preserve typed targets, decoded Unicode fragments, external URIs and page units. A fixed font checks copyable ASCII text across styled glyph runs, deterministic renderer reuse and Unicode metadata. Independent affine calculations check page/formula origins; bounded endpoint interpolation checks variable axes, clamps and monotonicity. Multi-reference footnote returns choose the earliest page and vertical position in the serialized PDF. Full shaping, image embedding and archival PDF conformance remain separate. |
| [Platform coordinates](../../crates/markview-platform/src/proptests.rs) | Quarter-pixel dimensions and eight DPRs check available titlebar space, optical balance, grid snapping or a clamped band edge, and translation invariance. No native window or platform service is called. |
| [Development syntax analysis](../../xtask/src/complexity/proptests.rs) | Bounded positive `cfg` trees use an independent production predicate model through real function/module filtering. Declared decision weights check complexity and nested function/closure ownership; parsed exemption markers preserve exact Unicode reasons and reject malformed arguments. Negative/contradictory predicates and arbitrary Rust syntax remain separate. |
| [Export configuration and PNG planning](../../src/export/proptests.rs) | Binary-fraction page units and device-pixel intervals check complete strip coverage, scroll origins, width/pixel-budget rejection, optional furniture overrides, unchanged input stylesheets, paper orientation, margins and finite configuration ranges. GPU drawing and PNG encoding remain covered by integration tests. |
| [Local file inputs](../../src/file/proptests.rs) and [clipboard text](../../src/paste/proptests.rs) | Real temporary files preserve UTF-8, remove one BOM, reject invalid bytes and classify final extensions through link resolution. Scalar/control inventories check clipboard validation; generated Unicode ATX headings check precedence, inline markup removal and the 72-scalar title limit in every locale. Oversized files, special-file races and arbitrary Markdown titles retain existing integration examples. |
| [Search snippets](../../src/app/file_search/proptests.rs) | Explicit Unicode grapheme tokens and selected positions constrain truncation, ellipses, exact highlighted bytes, complete cluster boundaries and merged adjacent highlights. Fuzzy scoring and Unicode normalization remain delegated to the matcher library and existing examples. |
| [Pixel allocation aliases](../../src/images/pixels/proptests.rs) | Independent allocation IDs check byte deduplication, incoming alias replacement, bounded memory, whole-allocation eviction and offscreen-first priority. Victim ordering among equally visible allocations is deliberately unspecified; real decode/concurrent publication remains separate. |
| [Archive member patterns](../../src/fonts/archive/proptests.rs) | A bounded exhaustive prefix-reachability model checks Unicode literals, `?`, `*` and separators. The enabled model includes member names containing literal `*`. Archive decompression, signatures and resource limits retain their existing integration tests. |

## Plan follow-up (2026-11-06)

The follow-up audited `artifacts/fuzz-pbt-plan.md` against the current tests.
It adds bounded models for the gaps below, without changing production behavior
or fixing defects. The plan itself is a local ignored artifact; this section
preserves the execution evidence in the repository.

| Plan item | Added domain and independent expectation | Remaining boundary |
| --- | --- | --- |
| Mixed HTML spans | Up to three 6×6 groups partitioned into disjoint rectangles; unique markers, canonical group order, exact spans, zero/overlong normalization and exact/one-under budgets. Rectangles are split before HTML is constructed, without a first-free-slot oracle. | General malformed/colliding HTML remains with the existing `fit_span` model and fuzz. |
| Table consumer | Independent column partitions in row stripes check mixed rectangles, occupied slots and group clipping through private `build_grid`, without shaping. Cell/row limits truncate; zero rowspan becomes one, unlike HTML's rest-of-group meaning. | Full table width/height, shared borders, retained disclosures and pagination remain integration/fuzz domains. |
| Source overlap | One to four blocks, one to three exact/transformed spans per field, and every source position in 24 ASCII bytes; enumerate covering candidates, allowing unspecified ties. Covers include nesting, shared endpoints, adjacency and zero width. | Arbitrary transformed front-end maps and Unicode source transformations remain separate. |
| Local text geometry | Two to four declared two-grapheme clusters, up to three lines, RTL/atomic flags and horizontal clipping/pan; explicit caret positions and rectangle intervals check nearest-box hits and reversed selections. | No shaping oracle, arbitrary bidi layout or vertical clipping model. |
| Finite MVSS cascade | Six selectors on body→blockquote→strong, up to four source layers; optional size/weight/color declarations model partial merge, depth, specificity and inheritance through parsing, source resolution and stylesheet merge. | Selectors avoid equal-specificity ties; duplicate selectors per source and size on body-containing selectors are invalid. General cascading/cache invalidation remains separate. |
| IME | Up to 24 operations with commit/cancel after two preedit updates, legal directed selections, start/end cursor extension and undo/redo; committed snapshots are independent of displayed composition. | Alphabet excludes grapheme merging and bidi movement; elapsed-time grouping remains outside the model. |
| Unicode search | Explicit sigma/Kelvin/long-s/sharp-s/dotted-I/combining examples, literal metacharacters, repeated fields and known Latin word boundaries; fixed byte ranges, cancellation and subsequent fresh-index equivalence. | Simple folding only; no full Unicode or ICU oracle. |
| Pagination | Quarter-pixel cases at one/four page heights retain a single image at the quarter-scale floor; impossible widow/orphan constraints advance until the remaining two-band suffix fits. | Minimum-scale overflow may be clipped; arbitrary retained flow remains fuzz. |
| Footnotes | Up to nine definitions and eleven references over a small ASCII label set, case variants, missing/unused labels and duplicates; last definition and first-reference numbering are recorded before parsing, across three newline forms. | Label whitespace/Unicode normalization, nested note bodies and backlink navigation remain outside this model. |
| Raw HTML | Known tags, literal Unicode, first duplicate href, decoded attribute entities, ignored CSS, inline anchors and sliceable source ranges. Text entities and literal nonbreaking spaces have recorded unresolved reproducers below. | The two defects are not passing coverage; arbitrary markup remains fuzz. |
| Decoder logic | ICO directory area/first-tie choice with bounded synthetic payloads and invalid ranges; full-range pixel budgets; exact integer initial-size arithmetic with power-of-two intrinsic widths; tiny uniform PNG target/alpha checks and SVG target edge examples. | Directory-only ICO fixtures do not decode PNGs. EXIF orientation, animation and hostile format combinations remain fuzz. |

Reparse/prefix identity, scroll/pan/inertia, archive globs, PDF/font/coordinate
algebra and resource source/security/cache items already have the distinct local
models requested by the plan. In particular, local resource tests already cover
Unicode and percent-encoded document/vault paths, and cache fixtures already
reject other security namespaces. They were retained without duplicate models.
The new decoder properties are included in `pbt.yml`.

The first MVSS generator incorrectly allowed duplicate selectors within a source
and size declarations on selectors containing body. Schema rejection was a
generator error, corrected by constructing unique valid declarations. The first
HTML attribute assertion expected a nested anchor on the block; the contract
represents it as an inline anchor, so that assertion was corrected. Neither is
a production defect. Power-of-two bitmap widths keep the rational-size oracle
exact instead of imposing real-number rounding on arbitrary float division.

### Recorded defect PBT-012: raw HTML text entities

Status: **fixed**. Before the fix, a supported raw HTML text run retained
`&amp;` rather than reading `&`, affecting reading/copy/search text. Attribute
entities already decoded. The reduced ordinary input was:

```html
<p><em>&amp;</em></p>
```

Expected text: `&`; previous text: `&amp;`. Text tokens now decode entities
once before whitespace normalization, after tokenization has recognized markup.
Encoded tags therefore stay literal text, and double-encoded entities are not
decoded twice. The [ordinary reproducer](../../crates/markview-commonmark/src/proptests/raw_html.rs)
is enabled, and [integration cases](../../crates/markview-commonmark/tests/raw_html_text.rs)
cover named/numeric entities, encoded markup, one-pass decoding, ASCII whitespace,
unknown names, styles and source-map validity.

The complete entity property retains its [saved seed](../../crates/markview-commonmark/proptest-regressions/proptests/raw_html.txt)
and input domain. It temporarily remains ignored under **PBT-013**, because its
`&nbsp;` cases now decode but still hit the independent nonbreaking-space collapse.
It will be enabled with that fix; no entity cases have been filtered out.
An explicit opt-in run shrank to `entities = [6], bold = false`, reading an
empty string instead of U+00A0. The full CommonMark package run passed 228 tests
with three known reproducers ignored; formatting, Clippy, documentation and
changelog checks passed.

```sh
cargo test --locked -p markview-commonmark --lib raw_html_ampersand_entity_reads_as_one_character
cargo test --locked -p markview-commonmark --test raw_html_text
```

### Recorded defect PBT-013: raw HTML nonbreaking spaces

Status: **recorded only, not fixed**. The literal text `a\u{a0}b` inside
`<p><em>…</em></p>` reads as `a b`. The HTML whitespace set excludes U+00A0;
collapsing it changes the text and removes its nonbreaking behavior. `push_text`
uses `char::is_whitespace`, which also includes this character. This is distinct
from PBT-012: no entity spelling occurs in the input. The ignored reduced
ordinary test `raw_html_preserves_literal_nonbreaking_spaces` was run and failed
with exactly that mismatch. Its input is fixed, so it needs no RNG seed.

```sh
cargo test --locked -p markview-commonmark --lib raw_html_preserves_literal_nonbreaking_spaces -- --ignored
```

### Follow-up verification

This batch adds **13 randomized properties and three directed tests** that pass,
and three ignored reproducers for the two newly recorded defects. Tests retain
the default case count and shrink budget; the extended run uses the environment.

```sh
PROPTEST_CASES=4096 cargo test --locked -p markview-document -p markview-commonmark -p markview-webpage -p markview-layout -p markview-image --lib proptests::
cargo test --locked -p markview-document -p markview-commonmark -p markview-webpage -p markview-layout -p markview-image --all-features --lib proptests::
cargo test --locked -p markview-document -p markview-commonmark -p markview-webpage -p markview-layout -p markview-image --all-features -- --skip proptests::
cargo clippy --locked -p markview-document -p markview-commonmark -p markview-webpage -p markview-layout -p markview-image --all-targets --all-features -- -D warnings
```

- The 4096-case run passed **94 tests** across the five packages, including the directed examples. Later strengthening of source semantic offsets and IME display/selection checks was also rerun at 4096 cases.
- The all-features property-module run passed **94 tests**, with **four ignored reproducers**: PBT-003, two for PBT-012 and one for PBT-013.
- The all-features unit/integration/documentation run excluding property modules passed **740 tests**, with two existing ignored tests. It includes the existing complex-table and Markdown HTML/details integration suites.
- Formatting, Clippy with warnings denied, documentation links/tables, changelog validation, all four CI infrastructure tests and the standalone fuzz workspace's locked metadata check passed.
- Each new ignored reproducer was explicitly executed and failed with its recorded mismatch. Production behavior was left unchanged.

Native commands used the NixOS environment documented in `AGENTS.local.md`:
`PKG_CONFIG_PATH=/run/current-system/sw/lib/pkgconfig:/run/current-system/sw/share/pkgconfig`,
`LD_LIBRARY_PATH=/run/current-system/sw/lib:/run/opengl-driver/lib` and
`FONTCONFIG_FILE=$PWD/tests/fontconfig.conf`. An initial build without these
variables failed to locate Fontconfig; the configured builds passed. No system
font, network request, GPU or Android emulator was needed for these models.
These are local results, not evidence of a remote CI run.

## Expansion verification (2026-11-05)

These historical results precede the production fixes. Counts below use the
package set included at each batch. Ten defect regressions are now enabled;
PBT-003 remains ignored. The commands above describe the current expanded suite.

The first expansion batch added 17 enabled properties and two ignored defect
reproducers. Local checks on Linux with Rust 1.99.1:

- A 4096-case run over the original five crates: 55 passed, two ignored.
- `cargo test --locked -p markview-document -p markview-layout -p markview-interaction`:
  unit, integration and documentation tests passed; recorded defects and the
  existing search measurement stay ignored.
- `cargo clippy --locked -p markview-document -p markview-layout -p markview-interaction --all-targets -- -D warnings`:
  passed, as did `cargo fmt --all --check`, `python3 scripts/test_documentation.py`
  and `python3 scripts/check_changelog.py`.
- Both defect commands failed with their stated reduced counterexamples, as
  expected. No implementation was fixed.

The parsing/index batch adds nine enabled properties and the PBT-003
reproducer, bringing the expansion to 26 enabled properties and three ignored
reproducers. The six-crate 4096-case command above passed 64 tests and skipped
the three recorded defects. Additional checks passed:

- `cargo test --locked -p markview-commonmark -p markview-document` (236 unit
  and integration tests passed, two recorded defects ignored, doc tests passed).
- `cargo clippy --locked -p markview-commonmark -p markview-document --all-targets -- -D warnings`.
- `cargo fmt --all --check`, the documentation/changelog checks above, and
  `python3 -m unittest discover -s scripts -p test_ci_infrastructure.py`.
- `cargo metadata --locked --format-version 1 --manifest-path fuzz/Cargo.toml`
  confirmed the standalone fuzz lock still resolves without changes.
- The PBT-003 command failed at `value = "a"`, as expected.

The pagination/cache/selection batch adds 11 enabled properties and the PBT-004
reproducer, bringing the expansion to 37 enabled properties and four ignored
reproducers. The six-crate 4096-case command above passed 75 tests and skipped
the four recorded defects. Checks also passed:

- `cargo test --locked -p markview-layout -p markview-interaction`: 517 passed;
  PBT-002, PBT-004 and two existing timing measurements stayed ignored.
- `cargo clippy --locked -p markview-layout -p markview-interaction --all-targets -- -D warnings`,
  formatting, documentation, changelog and whitespace checks.
- The PBT-004 command failed at `base = 1`, retaining the two correct URLs but
  exchanging occurrence ids 1 and 101 on the second layout.

The localization/scene/hover batch adds ten enabled properties and the PBT-005
and PBT-006 reproducers, bringing the expansion to 47 enabled properties and
six ignored reproducers. The seven-crate 4096-case command above passed 85 tests
and skipped the six recorded defects. Additional checks passed:

- `cargo test --locked -p markview-i18n -p markview-layout -p markview-interaction`:
  531 passed, with four defect reproducers, two timing measurements and the
  existing macro documentation example ignored.
- `cargo clippy --locked -p markview-i18n -p markview-layout -p markview-interaction --all-targets -- -D warnings`,
  formatting, documentation/changelog checks and CI infrastructure script tests.
- The standalone fuzz workspace still resolves with its existing locked file.
- PBT-005 failed at `prefix = ""`, `parameter = "p"`; PBT-006 failed at
  `keyword = "type"`, as expected.

The application-state/resource batch adds ten enabled properties and the
PBT-007 reproducer, bringing the expansion to 57 enabled properties and seven
ignored reproducers. The eight-package 4096-case command above passed 95 tests
and skipped the seven recorded defects. Additional checks passed:

- `cargo test --locked -p markview --lib -- --skip rendering_baselines` with
  the repository's Lavapipe environment: 915 passed, seven ignored, five
  golden baseline tests filtered out. The slower chrome frame checks passed.
- `cargo clippy --locked -p markview --all-targets -- -D warnings`, formatting,
  documentation/changelog checks and CI infrastructure script tests.
- The standalone fuzz workspace still resolves with its existing locked file.
- PBT-007 failed at `elapsed = 19`, changing the visible band from
  `0.51888806` to `0.48111194` while preserving its slot.

The retained-flow/request/persistence batch adds 17 enabled properties and the
PBT-008/PBT-009 reproducers, bringing the expansion to 74 enabled properties
and nine ignored reproducers. The eight-package 4096-case command above passed
112 tests and skipped the nine recorded defects. Additional checks passed:

- `cargo test --locked -p markview -p markview-layout --lib -- --skip rendering_baselines --skip proptests::`
  with the repository's Lavapipe environment: 1214 passed and six existing
  tests stayed ignored. Property tests and five golden baseline tests were
  filtered out; the slower chrome frame checks passed.
- `cargo clippy --locked -p markview -p markview-layout --all-targets -- -D warnings`,
  formatting, documentation/changelog checks and CI infrastructure script tests.
- The standalone fuzz workspace still resolves with its existing locked file.
- PBT-008 failed at `year = 1970`, `short_month = 0`, normalizing a nonexistent
  29 February to 1 March. PBT-009 failed at `age = 1`, `extra = 1`, replacing a
  one-second max-age lifetime with two seconds from a lower-precedence expiry.
  Review of RFC 9111 exposed that the first partial-header model also accepted
  this incorrect precedence. The enabled model now explicitly covers updates
  without `Expires`; the mixed-header guarantee is the ignored reproducer.

The document-collector/browser/HTML batch adds 15 enabled properties, bringing
the expansion to 89 enabled properties and nine ignored reproducers. The
ten-package 4096-case command above passed 127 tests and skipped the nine
recorded defects. No new production defect was found in this batch. Checks passed:

- `cargo test --locked -p markview-document -p markview-web -p markview-webpage`:
  104 unit/integration tests passed and PBT-001 stayed ignored; doc tests passed.
- `PROPTEST_CASES=4096 cargo test --locked -p markview-web --all-features`:
  24 tests passed, including the six browser properties and existing WOFF
  examples. The browser properties also passed 4096 cases with default features
  after strengthening key-map and revision-wrap checks.
- `cargo clippy --locked -p markview-document -p markview-web -p markview-webpage --all-targets --all-features -- -D warnings`,
  formatting, documentation/changelog checks and CI infrastructure script tests.
- The standalone fuzz workspace still resolves with its existing locked file.
- The PBT workflow now includes both web packages; the existing regression-file
  artifact glob already covers their future saved seeds.

The settings/flow/pagination/graphics batch adds 11 enabled properties, bringing
the expansion to 100 enabled properties and nine ignored reproducers. The
twelve-package 4096-case command above passed 138 tests and skipped the nine
recorded defects. No new production defect was confirmed. Checks passed:

- `cargo test --locked -p markview-layout -p markview-icon -p markview-render --lib -- --skip proptests::`
  with the repository's Lavapipe environment: 334 tests passed and one existing
  GPU test stayed ignored.
- `cargo test --locked -p markview --lib settings:: -- --skip proptests::`:
  36 existing settings tests passed. Both new settings properties passed
  4096 cases, including exact-byte backup and live-edit merge sequences.
- `cargo test --locked -p xtask`: 25 tests passed, confirming reuse of the SVG
  parser after moving the icon property entry to its owning crate root.
- `cargo clippy --locked -p markview -p markview-layout -p markview-icon -p markview-render --all-targets -- -D warnings`,
  formatting, documentation/changelog checks and CI infrastructure script tests.
- The standalone fuzz workspace still resolves with its existing locked file.
- Saved seeds retain the parallel-floor fixture correction, bounded selection
  arithmetic error and the paginator's existing half-pixel page allowance;
  their diagnoses are recorded on the corresponding ignored reproducers.
- The PBT workflow now includes the icon and renderer packages. These properties
  call SVG parsing and CPU preparation functions without creating a GPU device.

The final PDF/platform/tooling and remaining-local-API batches add 19 enabled
properties, bringing the expansion to **119 enabled properties** and eleven
ignored defect reproducers. The fourteen-library 4096-case command passed
**154 tests**, with the eleven recorded defects ignored. The three `xtask`
properties also passed 4096 cases, giving **157 passing properties across all
fifteen workspace packages**, including the 38 properties present before this
expansion. Additional checks passed:

- `PROPTEST_CASES=4096 cargo test --locked -p xtask`: all 28 tests passed.
- `cargo test --locked -p markview-pdf -p markview-platform -p xtask`:
  73 unit/integration tests passed, three defect/environment tests stayed
  ignored, and documentation tests passed.
- The fourteen-library command above with
  `--lib -- --skip proptests:: --skip rendering_baselines` and the repository's
  Lavapipe environment: 1456 existing tests passed and seven existing tests
  stayed ignored. This includes the slower chrome frame checks; the 165
  property/reproducer tests and five golden baseline tests were filtered out.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`, formatting,
  documentation/changelog checks and all four CI infrastructure script tests.
- The standalone fuzz workspace still resolves with its existing locked file.
- PBT-010 failed at `label = "a"`, `occurrence = 1`, `y = 0`, serializing the
  later page destination. PBT-011 failed at `prefix = ""`, `tail = "éé"`,
  rejecting `*éé` for `*?`. Both remain opt-in failing tests; neither was fixed.
- A saved HTML rowspan seed preserves the corrected synthetic-title fixture
  diagnosis. The new mixed preference, Markdown and utility models passed
  their expanded 4096-case runs.
- CI now runs all fourteen libraries and the `xtask` binary property suite.

This is local verification, not evidence of a remote CI run or of guarantees
outside the declared model domains.

## Expansion audit

The original audit covered fifteen workspace packages. It covered semantic
documents, parsing, layout, interaction, rendering preparation, browser delivery,
HTML extraction, localization, native adapters, PDF serialization, application
state/persistence and development tooling. The table maps the local contracts
to their independent models; it does not claim exhaustive line coverage.

The last audit also covered previously missing compound preferences, Obsidian
extensions, cross-row table occupancy, export planning, archive patterns, local
file inputs, clipboard text, search snippets and shared pixel allocations.
The original ten confirmed defects and PBT-012 have production fixes and enabled
regressions. PBT-003 and PBT-013 retain ignored failing tests; randomized
reproducers also retain saved seeds. They do not count as passing coverage. The follow-up adds `markview-image` as a sixteenth
package with properties.

Further work needs a distinct model or environment: arbitrary shaping and
Unicode segmentation, general CSS cascading, malformed Markdown/HTML/SVG
grammars, mixed retained-flow/table constraints, decompression formats, GPU
submission, native window effects and concurrent/network scheduling. Existing
examples, integration tests and fuzz harnesses continue to cover those areas.
Larger input grammars should be introduced with a documented oracle rather
than by widening generators past the guarantees actually asserted here.

These tests complement the [fuzz harnesses](../../fuzz/README.md): property tests
exercise local semantic objects and short sequences, while fuzzing explores
coverage-guided composition and full pipelines. Preserve existing example and
integration tests. Reuse relevant properties when changing an API; new properties
are necessary when the current evidence does not constrain the changed behavior.
