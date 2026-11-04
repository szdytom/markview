# Security and threat model

Markview aims to make reading untrusted content safe while preserving ordinary document and web reading. This page defines the security policy, attacker model, trust boundaries, and accepted risks. It describes design goals and controls, not a proof that every input is harmless. The document trust modes and resource authorization below are enforced by the reader; the verification page records regression coverage and remaining investigation.

[Security reference](security-reference.md) owns the threat catalog, implementation details, limits, and historical findings. [Security verification](security-verification.md) tracks evidence and outstanding verification work. [Architecture](architecture.md) explains pipeline ownership and resource boundaries.

## Reporting a Vulnerability

If you believe you have found a security vulnerability in Markview, please report it *privately*.
The preferred method is to open a GitHub Security Advisory (Repository → Security → Advisories).

## Scope and assumptions

The model covers a user reading untrusted Markdown or extracted web articles in the desktop reader or Android app: downloads, mail attachments, extracted archives, generated text, shared folders, shared URLs, and documents reached through links. Desktop execution is unsandboxed; this model does not rely on an OS sandbox to enforce document policy. Parsing, web extraction, layout, image loading, and watched revisions are in scope for both the reader and personal PDF export where supported.

Documents do not execute scripts. The supported raw HTML subset expresses Markdown-like semantics; `class` and `style` attributes are not interpreted. Document-controlled paths and URLs reach the filesystem, network, and operating-system handlers only through the policies below. Installed stylesheets are user-selected resources with a separate trust boundary; their declared fonts are downloaded only on explicit request.

PDF export writes to the destination the user supplies, and `pdf --watch` rewrites that same destination. Documents cannot choose an additional output path. Export runs without a window, GPU, or link handler and shares the reader's parsing and image budgets. A downstream PDF viewer's handling of the output belongs to that viewer's boundary.

A service that converts documents for other users has a different boundary: attacker input can borrow the server's network reachability, credentials, and filesystem access. That is a server-side request forgery (SSRF) deployment scenario. The personal reader's permissions are not a service isolation policy; service operators must supply a separate sandbox and egress policy. Running a personal export without a window does not by itself make it that scenario.

Physical access, kernel or driver defects in themselves, dependency supply-chain compromise, and social engineering that persuades a user to install or approve something are outside this model. Vulnerabilities in dependencies reached by document input remain relevant to the reader's safety.

## Document trust modes

**Trusted** is the default for a local document the user opens through the file picker, file manager, or command line. **Untrusted** is the default for clipboard content and loaded web articles, including shared text and URLs. Trust grants resource access; it does not assert that the document's bytes are harmless. Attacker-controlled input remains possible in both modes.

Classification follows how content enters the reader, not where its bytes happen to be stored. The reader assigns the mode; document text and response headers cannot select it. Clipboard text staged into a temporary `.md` file and extracted articles held in a reading session remain Untrusted. Being on disk alone, residing in a familiar directory, or having a friendly link label grants no extra authority.

| Entry or navigation | Resulting mode |
| --- | --- |
| User directly opens a local file | Trusted |
| Clipboard paste, shared text, or web-page load | Untrusted |
| Trusted local document follows a link to local Markdown | Trusted |
| Untrusted content follows a link to local Markdown | Untrusted; the identified local document needs a separate read authorization |
| Either mode opens a web page | Untrusted |

Document origin and trust mode belong to the reading session. Temporary writes, tab reuse, reloads, session restoration, and PDF export must preserve them rather than infer trust from a path. Reusing a tab cannot let an Untrusted navigation borrow its Trusted authority. Target-specific grants remain separate from document mode: authorizing one resource never promotes the whole document.

The local-file default deliberately includes downloaded Markdown, mail attachments, and files from extracted archives when the user directly opens them as local documents. These receive Trusted resource permissions even if their author is hostile. Merely staging web or clipboard content internally does not count as that user action.

## Reading capabilities and authorization

Both modes authorize parsing, display, and bounded public HTTP(S) image requests. Trusted mode additionally authorizes supported local image reads and ordinary local-network image requests by default. Untrusted mode needs target-specific authorization for either. A cross-origin image request or a private destination is not, by itself, evidence of an attack. Network tracking, unauthorized service access, and disclosure of sensitive data are separate concerns.

The browser analogy is passive resource loading: an ordinary image may be displayed across origins without giving document code access to its response. Markview executes no document scripts and exposes no response-reading API to content. This does not make requests harmless: CORS is not a general request firewall, and a GET can change a poorly designed service before image decoding rejects its response. See the [HTML image-loading model](https://html.spec.whatwg.org/multipage/images.html#updating-the-image-data).

The target policy assigns these capabilities:

| Capability | Trusted local document | Untrusted clipboard or web content |
| --- | --- | --- |
| Read local images under the image-path policy | Allowed by default | Requires identification and authorization of the local resource |
| Load public HTTP(S) images | Allowed by default | Allowed by default |
| Load intranet, VPN, or localhost HTTP(S) images | Allowed by default | Requires a target grant for the current document session |
| Execute scripts or automatically launch a program | Prohibited | Prohibited |
| Processing budgets and decoder checks | Enforced | Enforced |
| Confirmation for local links outside the OS-handler allowlist | Required | Required |
| Send local reads, other responses, or ambient credentials to a document-selected destination | Prohibited | Prohibited |

Explicitly opening an HTTP(S) page or requesting a font download authorizes the identified target or disclosed mirror set; unrelated destinations do not inherit that authority. An origin here is the URL's scheme, hostname, and effective port. For an Untrusted session's local access, authorization also records the destination address class, so a grant for a LAN service does not silently become permission for a loopback or link-local service. OS handlers own subsequent network behavior under the separate link policy.

## Assets

| | Asset | Consequence of compromise |
| --- | --- | --- |
| A1 | Process availability | Loss of what the reader is reading, unsaved session state, an interrupted `--watch` session |
| A2 | Data confidentiality | Local files, credentials, or fetched responses disclosed beyond authorized reading |
| A3 | Host integrity | Arbitrary code execution |
| A4 | Network identity | Opening a document alone discloses IP address and online activity |
| A5 | Trust in the reader's own interface | Document content impersonating reader chrome or a security prompt |
| A6 | Clipboard and selected text | Injection through the paste path |
| A7 | Local service confidentiality and integrity | Unapproved requests inspect services or trigger operations on the host or local network |

## Attacker capabilities

| | Capability | Realistic setting |
| --- | --- | --- |
| K0 | Controls the `.md` bytes or extracted article content | The baseline; always assume it |
| K1 | Also controls other files reachable by relative path | Extracted archive, cloned repository, shared folder |
| K2 | Controls a referenced remote server, redirects, or its DNS answers | Common |
| K3 | Controls a stylesheet the user loads | Depends on distribution |

## Trust boundaries

```text
   .md bytes ──(B1)──► comrak ──► Document (pure data) ──(B2)──► Layout ──► Scene ──► GPU (B4)
      │                    │                                            ▲
      │                    ├─► fence info ──► syntect ────────────────┘
      │                    ├─► $...$ ───────► ratex ──────────────────┘
      │                    └─► img src ──┐
      │                                  ▼
      └────────────────────────► (B3) image source resolution ──► local file / HTTP / data:

   link click ────────────────────► (B5) src/link.rs ──► OS handler

B1 untrusted bytes → pure data        B2 pure data → geometry (no side effects)
B3 document → filesystem and network  B4 process → driver
B5 document → OS execution
```

Image resolution and link activation cross the filesystem, network, and OS-handler boundaries. Parsing, shaping, layout, and decoding also expose availability and dependency-safety risks even when they perform no external I/O. The GPU is a separate process-to-driver boundary.

Explicit web fetches and font download jobs also cross B3. Web extraction then supplies untrusted document data to the native pipeline; it does not grant article content the authority of the user's initial request.

## Security policy

### Document processing and resource budgets

Input in either mode should not crash, hang, or exhaust the reader. Trust does not relax parser, layout, decoder, or cache budgets. Input-dependent recursion and expensive rendering work use explicit budgets; the reader degrades presentation or reports a resource error when a limit is reached. Decoded images, concurrent image work, and cache residency are bounded. These controls do not establish a universal bound on total process memory or execution time.

Local document and image reads open without waiting and then accept only a regular file, so a FIFO or device beside the document cannot block a read that no token can cancel. Shutdown is bounded too: application-owned threads are waited for with a deadline and then abandoned, and the I/O service stops with a grace period instead of joining blocked work forever. Abandoning a thread cannot corrupt a destination, because every write goes through an exclusive temporary file and an atomic replace. A headless export also gives up on an image pipeline that reports nothing at all for its deadline, which sits above the transport's own per-hop request budgets so only work no other layer bounds is caught: a completed job counts as progress whether it succeeded or failed, so a run of ordinary failures never accumulates into a deadline that discards healthy images. Past the deadline the unfinished entries report a failure, though a job already inside a blocking call may keep a pipeline slot until the process exits.

Document rendering does not grant content authority to alter reader controls. Link confirmations identify the resolved target rather than trusting the document's label. Copied text remains document-controlled data; copying does not make it safe to execute elsewhere.

### Image paths

Local image sources must be relative paths. Absolute paths and `file:` image URLs are refused. Relative paths may contain `../` and may follow symlinks outside the document directory: directory containment is deliberately not a security boundary, because neighbouring image directories are an ordinary use case.

These reads are automatic only for Trusted documents. Untrusted content needs the user to identify and authorize the local resource, subject to the same path rules. A clipboard paste has no implicit local resource base; its temporary storage directory must not supply one. Relative web resource URLs resolve against the page URL, because a web article is never staged as a file. Clicking a blocked image asks for the identified file or network target. For a relative clipboard image, the user selects the actual file with the file picker; the source spelling does not grant access to the staging directory.

Local image reads do not feed file contents into remote URLs. Remote HTTP(S) images and bounded `data:image/` sources are separate supported source types; SVG images cannot load additional resources through their image-href resolver.

### Links and OS handlers

Clicked Markdown links open in Markview. Directories and a closed allowlist of file extensions open through the OS without confirmation; expanding that list is a security-policy change. Other local file types require a blocking confirmation with a safe default, no remembered exemption, and the resolved path rather than the link label. The user may open the containing folder, explicitly open the file, or close the prompt.

An Untrusted document's link to local Markdown also requires authorization identifying the local read target, and the opened document stays Untrusted. Trusted local Markdown links retain Trusted mode; loaded web pages always enter Untrusted mode. The OS-handler allowlist and confirmations apply in both modes. Tab identity includes origin and mode, so navigation cannot reuse a Trusted tab for an Untrusted local read.

HTTP, HTTPS, and mailto links may open through their OS handlers on a click. Other remote schemes are refused. Local links, including `file:` links, are classified under the local-file policy and are not confined to the document directory. Existing targets are canonicalized before classification. The [reference](security-reference.md#local-link-classification) records the precise extension list and classification limitations.

### Network access and caching

#### Target authorization policy

Public HTTP(S) images load by default, subject to resource budgets. Localhost image servers, NAS resources, intranet sites, and VPN destinations are valid reading use cases. Their addresses determine the required authority, rather than making the resource intrinsically hostile. Ordinary service destinations are distinct from invalid destinations such as unspecified, multicast, and broadcast addresses.

Trusted local documents may automatically load ordinary intranet, VPN, and localhost images without a per-target grant. In Untrusted mode, automatic local requests wait for a grant identifying the actual target origin and address class. The reader may group pending targets into one session prompt; a grant covers subsequent images for the approved targets rather than prompting for every image. Grants do not transfer to another tab, survive a content replacement or revision, or persist after the session closes. They never change the document's mode. A denied request remains a placeholder.

Cached local responses follow the receiving session's mode and authorization: Trusted mode may reuse them, while Untrusted mode needs the same target authority as a network load. A body cached by a Trusted session cannot bypass an Untrusted session's grant requirement. Offline mode permits no new network requests and does not grant extra authority to cached local resources.

Explicitly opening a local HTTP(S) page authorizes that origin and address class for the page session, including its same-origin images. The page stays Untrusted: other local origins and local files still need separate authorization. The Fonts-page action or CLI download may authorize disclosed mirrors for that download job, including its latency probes. For requests handled inside Markview, the reader must identify a local target before granting it authority; a click whose label hides a target is not equivalent to entering a URL.

DNS resolution classifies the actual destination, and the permitted addresses are pinned for the connection. Every redirect repeats scheme, destination, and mode-specific authorization checks before sending the next request. Public redirects need no local grant; in Untrusted mode, a transition to a local target outside the grant must wait for authorization. DNS rebinding or proxy routing must not bypass the receiving session's policy. Link-local services, including metadata endpoints, need their own target authorization in either mode; Trusted defaults cover ordinary LAN, VPN, and loopback services.

Document-triggered requests use HTTP(S) GET without a document-selected method, body, or arbitrary headers. They must not borrow browser cookies or ambient host credentials, use local paths as referrers, or encode local file contents or protected response data into outbound URLs. Resolving resource URLs declared in a web article is ordinary loading, not a response-reading API for document code. URL userinfo is refused; neither Trusted mode nor a local-network grant authorizes credential forwarding. The browser [unsafe-port policy](https://fetch.spec.whatwg.org/#port-blocking) is the baseline for avoiding HTTP requests to unrelated protocols.

The browser precedent is authorization rather than permanent refusal: Chrome's [Local Network Access design](https://developer.chrome.com/blog/local-network-access) gates covered public-to-local requests on permission. Markview adapts that principle to Untrusted sessions, with broader default resource authority for user-opened local files; it does not claim browser origin, sandbox, CORS, or permission equivalence.

Personal headless exports preserve the source document's mode. A local document opened directly by the caller uses Trusted defaults; an export of clipboard or web content stays Untrusted and needs explicit caller-supplied grants for local resources. Without such authority those resources remain blocked; the absence of a prompt is not consent. `markview pdf` accepts `--document-trust trusted|untrusted`, repeated `--allow-local-image FILE`, and repeated `--allow-network CLASS=ORIGIN`, with `private`, `loopback`, or `link-local` as the class. CLI grants apply to the initial document content; a changed source in a watch job revokes them.

#### Current implementation

`src/security.rs` defines `Trust`, `Origin`, `AddressClass`, and `Resource` enums. Each session, worker request, and export carries a `Security` context. Clipboard staging and web page loads preserve Untrusted origin; directly opened local files use Trusted defaults. Untrusted local Markdown links require read confirmation and retain their mode. Session restoration saves origin without resource grants.

The shared HTTP client checks every resolved address against this context, pins direct connections, and repeats scheme, port, credential, and address authorization on each redirect. Explicit page and font jobs authorize only the selected origin and its resolved address classes. System HTTP/SOCKS proxies are disabled so they cannot override the checked route. Invalid destinations and browser unsafe ports remain refused in either mode; Trusted defaults do not cover link-local targets.

Image jobs carry the session's context. Changing authority cancels prior jobs and clears decoded results. Disk cache keys include a SHA-256 partition of the receiving context, including grants and their content binding, and metadata verifies the partition. This conservatively separates caches with different authority, including public bodies; previously stored entries outside these partitions are not reused. Online cache access checks the requested destination and any stored redirect destination before reuse; offline cache access opens no sockets. Grants are bound to a SHA-256 digest of the document source, and changed content revokes them before loading resources even when a file change was missed while a tab was inactive.

Remote images load by default, subject to a per-document-revision source cap. Excess sources remain placeholders; a notice offers Dismiss or Load all. Load all lifts only that tab's source cap for that content revision. It neither grants local access nor changes the address policy. The notice is not a consent gate for requests within the cap.

Image requests have byte and time limits. Cached image bodies and their URLs persist in a bounded disk cache. `--offline` makes no image requests and may serve cached bodies even when stale, including entries whose cache directives would otherwise require revalidation.

### Font downloads

Stylesheet-declared fonts download only through an explicit Fonts-page action or `markview fonts download`, never just by opening or installing a stylesheet. Under the target policy this is an explicit download job whose authority covers its identified mirrors, not arbitrary local requests by the document. Currently downloads share the public-address restriction and are refused under `--offline`.

Downloads and archive extraction are bounded. Extraction cannot escape the download directory or install symlinks or hard links. Declared hashes must match, and font contents are validated before installation. HTTPS is preferred; HTTP mirrors are accepted with the corresponding transport privacy and integrity risk. Installed fonts are personal resources; measurement modes and explicitly isolated font configurations retain their configured font set.

## Accepted and residual risks

- **Network identity and history:** default remote-image requests can disclose an IP address, opening time, and a document-specific URL without prior confirmation. The source cap limits automatic activity; offline mode prevents it. Cached bodies and URLs leave a local record readable by anyone with access to the cache.
- **Trusted local-file default:** directly opening a downloaded or otherwise hostile local document grants its supported local reads and ordinary LAN, VPN, and localhost requests under the target policy. This is an intentional usability tradeoff, not evidence that the file is safe. Both modes retain execution restrictions and resource budgets.
- **Request side effects:** even an unauthenticated GET can trigger an operation at a service or reveal that the reader can reach it. Public resource loading accepts this risk for public destinations in both modes. Trusted mode additionally accepts it for ordinary local-network resources; Untrusted mode accepts it only within identified target grants. A failed image decode cannot undo a request. The receiving session's authority gates local requests; it does not remove side effects once a request is authorized.
- **Local reads and races:** relative image paths, symlinks, and clicked local links can reach outside the document directory. Sensitive images may be displayed, and errors can reveal local existence or file type. A local writer can race reads or preserve file metadata used for staleness checks. Directory containment is intentionally absent; no mechanism sends the resulting file contents back in a URL.
- **OS handlers and user decisions:** confirmation permits potentially executable files to open. Allowlisted viewers also have their own vulnerabilities; an SVG opened as a top-level browser document may execute script. Extension classification is not a proof that a file or its handler is safe.
- **Resource and dependency safety:** byte and work budgets reduce pathological workloads but cannot guarantee fast math evaluation or bounded aggregate memory. Dependency bugs, malformed geometry, and concurrency failures remain possible; their evidence and verification gaps belong in the verification document.
- **Non-blocking opens are not available everywhere:** a non-blocking open covers the special files a document can name on Unix. On Windows a device or pipe open can still wait, and on any platform a network or FUSE mount can block a stat or an open for as long as the mount decides. Those paths are not bounded by the read itself; only shutdown's deadline keeps them from holding process exit indefinitely.
- **An abandoned job can keep a resource:** killing a wait does not interrupt a thread already inside a blocking call. A document that stalls several reads can leave pipeline slots held until the process exits, after which images load more slowly rather than failing outright. Restarting is the recovery.
- **Interface impersonation:** headings, link labels, and image alt text remain attacker-controlled. Restricted HTML and target-based confirmations reduce the surface but do not authenticate displayed content.

## Non-goals

Markview does not make a hostile document safe to act on, confine local paths to a document subtree, or protect against an adversary who already controls the user's files beyond the controls described above. It does not prohibit misleading content from being displayed or guarantee the safety of external handlers, downstream PDF viewers, or drivers.

Anonymous network use, complete browser compatibility, and isolation for a multi-user conversion service are not guarantees of the personal reader. Ordinary resource loading and explicitly authorized local service access are supported policy capabilities, rather than vulnerability classes in themselves.
