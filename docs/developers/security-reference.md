# Security controls and threat reference

Use this page when deciding which controls and regressions a security-sensitive
change must preserve. [Policy](security.md) owns authority and accepted risks;
[verification](security-verification.md) owns evidence requirements. Threat IDs
remain stable for references to earlier investigations.

## Threat catalog

| ID | Threat | Control or remaining concern |
| --- | --- | --- |
| T1 | Recursive input aborts the process | Depth budgets and plain-text fallback; keep reduced regressions |
| T2 | CPU exhaustion | Work/byte budgets and progress-making fallbacks; formula bytes are not a time bound |
| T3 | Memory exhaustion | Bound decoding and caches; aggregate peak still needs measurement |
| T4 | Dependency memory safety | Sanitizers, fuzzing and dependency review; no blanket safety claim |
| T5 | Local reads beyond document authority | Origin/mode and resource grants before reads |
| T6 | Dangerous OS file opening | Central classification and explicit confirmation outside the allowlist |
| T7 | Tracking and cache history | Accepted consequence of default public image loading; offline prevents requests |
| T8 | Symlink and file replacement races | Directory containment is intentionally absent; authorized reads retain race risk |
| T9 | Async state races | Versioned acceptance, cancellation and isolation tests |
| T10 | GPU/driver failure | Validation and device-loss handling; software tests do not prove driver safety |
| T11 | Interface impersonation | Restricted markup and resolved-target prompts; labels remain untrusted |
| T12 | Unauthorized local-service requests | Checked/pinned destinations and scoped grants |
| T13 | Data sent beyond reading authority | No scripts or response-to-URL composition; credential/routing controls |

## Processing budgets

The owning [layout limits](../../crates/markview-layout/src/limits.rs) and image/network implementations
are the source of exact defaults. Test both the boundary and its fallback when
changing a limit. A fallback must make progress, preserve useful content where
possible and avoid starting another unbounded operation.

Do not infer a whole-process memory bound from a decoded cache cap: simultaneous
source buffers, parser arenas, geometry, worker allocations and inactive tabs
also retain memory. Do not infer a math execution bound from formula length.
Native platform code and dependencies contain unsafe boundaries; review their
actual reachability rather than claiming the whole workspace forbids unsafe code.

## Document origin and mode

Preserve source authority through staging, navigation, tab identity, reload,
restoration and export. A temporary local path is not evidence of Trusted origin.
Resource grants belong to a content revision and identified target, not a shared
path or global cache. A grant never promotes the document's mode.

## Image source resolution

Semantic documents describe an address and base, or a diagram source. They do not
resolve paths or authorize I/O. The host resolves under the receiving security
context and supplies layout with resource keys and prepared metadata.

Cache keys must distinguish bases and authorities. Reusing bytes or decoded pixels
must not let Untrusted content borrow a Trusted session's access. Test both disk
cache and in-memory reuse, including identical paths and URLs in different tabs.

## Local-link classification

`src/link.rs` owns the exact extension allowlist and classification tests. Expanding
it changes security policy. Existing targets are canonicalized before classification;
Markdown opens in the reader, directories and allowlisted types use OS handlers,
and other local types require confirmation. Untrusted Markdown reads additionally
need their identified read authorization.

Classification does not prove a file is non-executable or that its handler is safe.
Test resolved paths, unusual extensions, directories, absent targets and platform
path syntax. Confirmations must display the resolved target and default to opening
the containing folder, not trust the document's label.

## Network and font downloads

The shared transport must classify resolved destinations, pin checked addresses
and recheck redirects. Grants bind origin and address class. Invalid destinations,
URL credentials and unsafe ports are rejected; proxies are disabled. Verify actual
received requests and routing, not just URL parsing.

An image admission cap limits default work; Load all lifts that cap without adding
authority. Offline mode prevents requests but must still enforce cached-resource
permissions. Cache freshness and authorization are separate decisions.

Font download is an explicit job over disclosed sources. Validate body size,
archive paths/types, declared hashes and font contents before installation. A
partial or failed transfer must not replace a valid installed file. Downloaded
fonts are personal resources and must not leak into isolated benchmark inputs.

## Historical findings

The [original investigation](history/security-findings.md) retains reduced recursion
failures and earlier estimates. It is historical evidence, not an exhaustive current
coverage matrix. [Fuzz findings](../../fuzz/FINDINGS.md) retain later minimized cases.
