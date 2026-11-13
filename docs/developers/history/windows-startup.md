# Windows startup investigation (#39)

> Historical evidence, archived from repository revision `37f22d622d41d5409321b8db7cf9f36b38b0d5a1`.
> Measurements and implementation descriptions apply to the investigations below,
> not necessarily to the current release. Original measurement dates are retained;
> the archive revision is not a claim that those measurements were rerun.


Measured on 2026-11-09 at commit
`1377e1a27642d44980c699972a49888a42572490`. The initial investigation below
used an isolated instrumented copy. The subsequent implementation and
allocator comparison are recorded separately below; window visibility stays
unchanged throughout.

The substantial avoidable work behind the blank window is D3D12 preparation
for **indirect draws that Markview never issues**. On the supplied Intel GPU,
six eager `CreateCommandSignature` calls cost about **238 ms** on a new
executable path. Three belong to device initialization; three more belong to
the first render pipeline's implicit layout. The latter accounted for about
176 ms inside what initially looked like slow shader compilation.

A separate delay occurs **before Rust `main`** when Defender scans a new EXE.
That delay must be measured separately from the already-visible blank window.

## Environment and measurement boundaries

- Windows 11 IoT Enterprise LTSC, build 26100; interactive desktop session 1.
- Intel Core i5-13500H, 16 logical processors, approximately 16 GiB RAM;
  balanced power plan.
- Intel Iris Xe, driver `31.0.101.4032`, D3D12, FXC.
- Rust 1.98.1; the repository's release profile, locked dependencies and
  `diagnostics` enabled. The baseline diagnostic harness needed the missing
  `Request::loaded: None` initializer to compile; this is in the experimental
  patch and does not affect the reader path.
- Empty reader, `--foreground --offline`, 1200 × 800 logical pixels,
  DPR 1.75, 2100 × 1400 framebuffer. Settings and session files use a private
  `APPDATA`. Sampling starts after compilation has finished.

Three pairs of fresh executable paths were tested with indirect validation
enabled and disabled. Each executable ran twice. The copies differ only in
the PE timestamp, leaving instructions and resources intact. This exercises
first use of a new application identity without clearing global driver caches
or rebooting Windows. It is **not** a measurement of an OS-cold or storage-cold
boot. A separate ten-process warm baseline corroborates the repeated-path
results.

Checkpoints distinguish Rust entry, visible-window observation, first present
return and first submission completion. Rust entry excludes process creation,
loading and pre-entry scanning. A queue completion wait occurs **after** the
first present; it does not postpone that present, but can affect later frames.
The external window observer polls every 5 ms and is subject to Windows timer
scheduling. Queue completion is not display scanout.

## Stage breakdown

All values below are milliseconds, medians of three processes in each column.
The first two columns use the default indirect-validation setting. The last
column changes only `WGPU_VALIDATION_INDIRECT_CALL=0` in the experiment.
Subtotals overlap their constituent rows and should not be added together.

| Stage | New path, first run | Same path, second run | New path, indirect validation off |
| --- | ---: | ---: | ---: |
| Arguments and logger | 8.21 | 8.32 | 8.41 |
| Initial stylesheet configuration | 0.14 | 0.14 | 0.16 |
| Instance discovery | 3.96 | 3.20 | 3.91 |
| Event loop creation | 2.98 | 3.87 | 2.97 |
| `App::new` | 4.95 | 1.40 | 4.50 |
| ↳ System font collection | 0.97 | 1.05 | 1.01 |
| Window creation, including attributes/icon | 25.24 | 28.64 | 26.24 |
| Native frame and Direct Manipulation | 2.04 | 1.43 | 1.83 |
| Reload styles | 0.56 | 0.54 | 0.56 |
| wgpu instance | 7.15 | 7.09 | 8.48 |
| Surface creation | 0.08 | 0.07 | 0.09 |
| Adapter request | 65.64 | 28.55 | 69.96 |
| Device request | 107.07 | 29.79 | 73.38 |
| Surface configuration | 7.48 | 7.43 | 7.54 |
| WGSL shader module | 0.32 | 0.26 | 0.79 |
| Flat render pipeline, including implicit layout | 190.81 | 5.24 | 13.02 |
| Image render pipeline | 7.58 | 3.08 | 7.57 |
| Renderer caches and buffers | 1.67 | 1.90 | 1.99 |
| **GPU initialization subtotal** | **388.05** | **84.45** | **182.35** |
| Session restoration/open request | 0.03 | 0.03 | 0.03 |
| Resume return to first redraw handler | 36.40 | 39.35 | 39.14 |
| First surface acquire | 0.06 | 0.07 | 0.07 |
| Overlay construction | 2.70 | 2.40 | 3.08 |
| Frame preparation and submission | 13.68 | 15.06 | 15.47 |
| Present | 0.79 | 0.72 | 1.01 |
| Completion wait after present | 13.70 | 24.83 | 20.73 |
| **Rust entry to first GPU completion** | **504.23** | **213.41** | **312.00** |
| **Observed visible window to first present return** | **471.02** | **162.55** | **276.84** |

The ten-process warm baseline measured 83.05 ms for GPU initialization and
214.29 ms from Rust entry to GPU completion. Font discovery and native-frame
creation are small on this machine. The roughly 36–39 ms before the first
redraw remains a separate event-delivery interval; the command-signature
control does not remove it.

Three additional post-build processes opened the 9855-byte `README.md` with
the default setting. The worker's accepted first-readable update and the GUI
trace measured these separate endpoints:

| Document stage | Median (ms) | Range (ms) |
| --- | ---: | ---: |
| File read | 0.09 | 0.09–0.09 |
| Markdown parse | 0.79 | 0.78–0.81 |
| Worker layout | 87.75 | 73.43–387.91 |
| Rust entry to first UI present return | 268.46 | 236.95–290.27 |
| Rust entry to first readable present return | 290.27 | 268.46–547.69 |

Two of these first UI frames already contained readable content; one was a
loading frame. Worker stages overlap GUI activity and must not be added as
serial startup stages. The large layout outlier and three-sample size limit
conclusions about document latency. The empty-window experiment above avoids
this additional work and still reproduces the startup bottleneck.

## Native cause and causal control

The startup sequence in [lifecycle.rs](../../../src/app/lifecycle.rs) creates a
visible window and synchronously calls the GPU initializer in
[painting.rs](../../../src/app/painting.rs). This keeps the UI thread occupied
until [renderer initialization](../../../crates/markview-render/src/lib.rs) and
[pipeline creation](../../../crates/markview-render/src/pipeline.rs) finish.

wgpu's release defaults include `VALIDATION_INDIRECT_CALL`. In wgpu-core this
creates two validation compute pipelines and adds `INDIRECT_BUILTIN_UPDATE`
to pipeline layouts. The D3D12 backend then creates root-aware indirect-command
signatures for draw, indexed draw and dispatch. Its device constructor also
creates three basic indirect-command signatures unconditionally.

The relevant upstream implementations are
[instance flags](https://docs.rs/crate/wgpu-types/30.0.1/source/src/instance.rs),
[device and implicit-layout creation](https://docs.rs/crate/wgpu-core/30.0.1/source/src/device/resource.rs)
and [D3D12 command signatures](https://docs.rs/crate/wgpu-hal/30.0.1/source/src/dx12/device.rs).
The same eager setup exists in wgpu 29.0.4, pinned by both v0.1.10, around the
original report, and v0.4.0, used by #72. The detailed profile here uses the
current checkout's wgpu 30.0.1.

A small GPU-only harness used a private instrumented copy of wgpu-hal to
time the native calls. Its three pairs of first/repeated executions found:

| Native work | First execution of a new path | Repeated execution |
| --- | ---: | ---: |
| Three basic command signatures, device constructor | 61.11 | 0.98 |
| Root-aware signatures across the two reader layouts | 176.42 | 1.72 |

For one first execution, the first layout's three native calls took 73.84,
77.06 and 30.19 ms. That layout took 186.09 ms overall. Serializing and creating
the root signature took 0.009 and 0.024 ms; the render shader compiler calls took
1.63 and 1.83 ms, and native graphics PSO creation took 6.58 ms. Calling this
whole interval “shader compilation” would identify the wrong optimization.

Changing only clip-space depth to obtain fresh vertex-shader cache keys
increased native graphics PSO creation from roughly 0.5 to about 10 ms. It did
not reproduce the approximately 180 ms layout cost. Descriptor-heap sizing
was also tested: reducing one million descriptors to 4096 saved roughly
3 ms in heap creation, far below the main signature cost.

The full GUI control with indirect validation disabled reduced device
creation by about 34 ms and flat-pipeline creation by about 178 ms. It removed
about 206 ms of actual GPU initialization and about 194 ms of visible blank
time. Native probes confirmed that root-aware signature creation disappeared;
the approximately 60 ms basic signatures remained. This is a reduction of
startup work with the original window visibility, rather than a later reveal.

The reader submits direct `RenderPass::draw` calls in
[frame.rs](../../../crates/markview-render/src/frame.rs); the renderer and host
contain no indirect draw or dispatch calls. This finding suggests two concrete
implementation targets: scope out indirect validation for this direct-draw
renderer, and make the backend's remaining indirect-command signatures lazy.
The latter needs a backend change; the environment control alone leaves
substantial initialization work in place. This investigation does not ship
either change or establish a universal first-frame target across GPUs.

### Cache interpretation and lifetime

The measurements establish cross-process acceleration of the same native
creation calls. They do not identify the exact cached representation, key
fields, or retention period. The command-signature objects are created again
in each process. D3DSCache file activity was observed, but individual cache
accesses have not been attributed to those calls. Driver-managed state,
disk-cached generated data and OS file caching have not been isolated.

Microsoft's [shader-cache specification](https://microsoft.github.io/DirectX-Specs/d3d/ShaderCache.html)
documents cross-run disk storage, versioning and disk-cleanup invalidation;
it does not promise a fixed retention time for the driver's internal cache.
That general contract does not establish which mechanism accelerated these
particular command-signature calls.

At 2026-11-09 16:19 UTC, the same native-probe EXE was rerun 65.33 minutes after
its recorded repeat. Its command-signature calls still totaled 2.748 ms,
compared with 242.982 ms in its first execution. The machine had not rebooted
and other graphics experiments occurred during that interval. This is one
later fast observation, not a measured TTL or proof that a particular cache
entry survived untouched. Reboot persistence, overnight retention and
individual cache-layer invalidation remain untested. The raw recheck is under
`artifacts/windows-startup/cache-recheck/`.

## Pre-entry scanning and presentation cross-check

External launch timestamps exposed a second, independent delay. In a separate
Defender recording, launch to Rust entry took 2548.59 ms on a new EXE and
18.92 ms on its repeat. Defender recorded an `OnClose` scan of 1373.31 ms and
an `OnOpen` scan of 1352.57 ms for that EXE. These durations include file-write
activity and can overlap the measured launch boundary; their sum is not an
additional startup stage to add blindly. The recording establishes a separate
file-scanning cost, not a Rust initializer or font-discovery cost. Antivirus
configuration was not changed.

PresentMon 2.3.0 independently captured eight warm reader processes. Its v1
CSV omitted the first present row. Reconstructing the preceding present from
`QPCTime` and `msBetweenPresents` agreed with the internal first-present-return
probe within about 1–5 ms. Reconstructing the first display change gave a
median 204.43 ms after the window was observed visible. These reconstructed
values are a cross-check, not direct first-frame CSV observations.
The field definitions are documented in
[PresentMon's v1 metrics](https://github.com/GameTechDev/PresentMon/blob/v1.9.2/README.md#csv-columns).

An official v0.4.0 executable also exhibited an early visible window followed
by delayed application presentation. That exploratory capture ran during the
initial build and is retained as qualitative evidence, not included in the
performance tables. The reporter's original hardware has not been profiled;
the numbers above apply to the supplied machine and its driver.

## Vulkan comparison (2026-11-10)

The Windows experiment copy was rebuilt with both `dx12` and `vulkan` enabled
in the root and renderer manifests. The **same binary bytes** were used for
both backends, selected by `WGPU_BACKEND=dx12` or `vulkan`. Indirect validation
remained enabled in both, so this comparison changes only the backend.
Production manifests and window visibility were not changed.

Three pairs of new executable paths were run twice each, alternating which
backend ran first. Six additional processes used the shared executable path
to corroborate warm behavior. Every formal sample logged wgpu's actual adapter
selection: both backends used Intel Iris Xe, vendor 32902, device 42912, with
`IntegratedGpu` type. The parser rejects a missing/mismatched backend,
different physical GPUs, or incomplete sample groups.

These new-path measurements followed a Vulkan capability pilot and do not
represent clearing driver caches or a reboot. The first preliminary Vulkan
run took 385.92 ms from Rust entry to GPU completion; it is retained separately
from the hardware-verified paired cohorts. The following values are medians
of three samples per column, in milliseconds:

| Stage | D3D12 first | Vulkan first | D3D12 repeat | Vulkan repeat |
| --- | ---: | ---: | ---: | ---: |
| Instance creation | 8.16 | 23.49 | 7.53 | 23.10 |
| Adapter request | 58.55 | 1.00 | 28.72 | 0.88 |
| Device request | 106.33 | 31.95 | 34.78 | 33.08 |
| Surface configuration | 7.35 | 20.37 | 7.43 | 19.91 |
| Flat pipeline | 190.85 | 9.06 | 4.99 | 9.05 |
| Image pipeline | 7.61 | 7.34 | 2.93 | 7.91 |
| **GPU initialization subtotal** | **382.56** | **100.15** | **87.80** | **99.12** |
| Resume return to first redraw handler | 36.50 | 113.41 | 39.43 | 112.88 |
| Frame preparation and submission | 14.45 | 5.87 | 14.51 | 5.41 |
| Completion wait after present | 14.06 | 4.87 | 30.09 | 7.85 |
| **Rust entry to first GPU completion** | **500.52** | **278.70** | **225.55** | **275.39** |
| **Observed visible window to first present return** | **466.19** | **255.00** | **171.71** | **251.59** |

Vulkan reduced first-use GPU initialization by about 282 ms and advanced first
GPU completion by about 222 ms. Repeated-path startup was about 50 ms slower.
The shared-path warm confirmation measured 218.06 ms for D3D12 and 275.57 ms
for Vulkan, consistent with that direction.

Vulkan avoids the D3D12-specific command-signature penalty, but has higher
instance/surface costs and a substantially longer interval before the first
redraw on this driver. That last interval was measured, not attributed to a
particular native call in this comparison. GPU initialization alone would
therefore give an incomplete account of the user-visible result. New-path
launch-to-entry delays were separately about 2.7 s for both backends and are
excluded from the Rust-entry numbers above.

Native `smoke` tests succeeded with both backends. Their entire reading area
below the animated tab bar was pixel-identical at 2100 × 1330 pixels. Results
apply to this GPU/driver and three-pair sample; they do not establish Vulkan
as a generally faster Windows default.

The added raw data, binary hash, manifest-only backend patch, launch scripts
and validated cohort summaries are in `artifacts/windows-startup/`, under
`backend-9f25d8a1-*`, `backend-smoke` and `backend-comparison-summary.json`.
`vulkan-evidence.zip` packages this comparison separately from the earlier
D3D12 investigation.

## Concrete optimization options

The candidates below were ranked after the initial investigation. The first
two have subsequently been implemented; their follow-up measurements are
recorded separately below. Native-call costs are not guarantees of equal
end-to-end savings and must not simply be added together.

| Order | Change | Evidence and implementation constraint |
| --- | --- | --- |
| 1 | Give the direct-draw renderer a default without `VALIDATION_INDIRECT_CALL`, retaining an explicit diagnostic opt-in. | The GUI ablation removed about 206 ms of first-use GPU initialization. Preserve and test the renderer's direct-draw/shader assumptions; this is a scoped configuration change, not removal of all GPU validation. |
| 2 | Measure individual startup resize/configure calls, coalesce ordinary resize events and skip unchanged dimensions. | The original `window_resized` reached an unconditional surface configure. The initial first-redraw interval was about 39 ms on D3D12 and 113 ms on Vulkan; this interval includes event handling and cannot all be attributed to configuration. Lost, outdated and suboptimal surfaces must still be forcibly reconfigured even at unchanged dimensions. |
| 3 | Make the backend's basic indirect-command signatures initialize only when an actual indirect command needs them. | Three unused basic signatures still cost about 61 ms in first-use native probes. This requires a wgpu-hal change, including a viable deferred-error path, rather than another application flag. |
| 4 | Start document preparation before synchronous GPU initialization, after window geometry and styles are known. | The current resume path starts file opening after `gpu()`. Reading, parsing and first-viewport layout could overlap GPU work. This targets document-readable latency; it has no document work to overlap in the empty-window case. Net benefit is unmeasured. |
| 5 | Share an explicit pipeline layout, initialize the image pipeline when needed, and choose descriptor capacity from actual resource bounds. | These are smaller candidates. Image-pipeline creation was about 7–8 ms on first use; the smaller descriptor-heap experiment saved about 3 ms. Most of the large first-pipeline cost was command signatures, not shader compilation. |
| 6 | Separately evaluate release-package signing/reputation and normal installation-path scanning. | The measured pre-entry scans are seconds long. Microsoft documents optimized handling of highly reputable content, including content signed by trusted sources. Signing benefits for Markview remain unmeasured; signing alone must not be assumed to remove the observed delay. |

The relevant contracts are documented in
[wgpu's indirect-validation flag](https://docs.rs/wgpu/30.0.1/wgpu/struct.InstanceFlags.html#associatedconstant.VALIDATION_INDIRECT_CALL)
and [Defender's trusted-content handling](https://learn.microsoft.com/en-us/defender-endpoint/mdav-scan-best-practices#scanning-and-exclusions).

Vulkan can remain an experimental backend choice while its first-redraw
interval is investigated. Its slower repeated-path result on this machine
does not justify changing the Windows default from the first-use result alone.
Application-side configuration and redundant-reconfiguration work should
precede taking on a backend fork. Retest fresh paths and repeated processes,
empty and populated readers, first UI and first readable GPU frames, and
rendered pixels for each change.

## Implemented optimizations and allocator comparison (2026-11-10)

The renderer disables unused indirect-call validation by default and
coalesces obsolete surface sizes before skipping unchanged configurations.
`WGPU_VALIDATION_INDIRECT_CALL=1` still explicitly enables indirect validation;
other validation flags are preserved. Lost, outdated and suboptimal surfaces
retain forced reconfiguration, and zero-sized hosts are treated as occluded
before acquisition so recovery never creates an unconfigured zero-sized surface.

Ordinary window resize events use the window's current physical size for GPU
configuration. Each event still updates the other UI state. Four queued startup
events described 1866 × 1099, 1890 × 1163, 2100 × 1400 and 2100 × 1400, while
the window was already 2100 × 1400. Applying every historical size unnecessarily
recreated the swapchain. Using the current size removes those configurations.

The mimalloc experiment was rejected after measuring its memory cost. The
application keeps the system allocator; no allocator feature, dependency,
global allocator or platform-boundary exception is retained. The comparison
below records the isolated experimental build: `mimalloc` 0.1.52,
`libmimalloc-sys` 0.1.49 and native mimalloc 3.3.2 with the wrapper's defaults.

### Measurement controls

Run ID `opt-20261110` includes 120 measured processes plus two prewarm processes.
The release profile, D3D12 adapter, window geometry and private configuration
setup match the investigation. All measured processes exited with code 0 in
interactive session 1. Each process runs for four seconds before a normal close;
CPU and memory counters are captured before closing it. No compiler was running
during these samples, and the controller checked for compiler processes before
each group. Four configurations alternate their order by round:

- **Original policy**: indirect validation enabled; configure every resize using
  the historical event dimensions.
- **Optimization 1**: the new default indirect-validation policy, with the original
  resize/configure behavior restored by the experimental harness.
- **Optimizations 1+2**: both production changes, using the system allocator.
- **mimalloc**: both changes plus the isolated Windows allocator experiment,
  which was subsequently removed from the application.

The first three use the same executable bytes. Restoration switches and
checkpoints exist only in the independent experiment source. Fresh-path copies
retain identical bytes within an allocator build; no OS-wide cache clearing or
reboot is implied. GPU completion instrumentation still waits after first
present, so first-present timings are unaffected by that wait. Later document
frames can be affected and should be interpreted under this common instrumented
condition; these are not an uninstrumented production document benchmark.

### Empty-reader results

Milliseconds, cohort medians. New-path first and second processes each have
n=5 per variant. Reused-path processes have n=8 per variant.

| Main entry → first present | Original policy | Optimization 1 | Optimizations 1+2 | + mimalloc |
| --- | ---: | ---: | ---: | ---: |
| New path, first process | 481.49 | 275.60 | 241.80 | 236.24 |
| Same path, second process | 190.62 | 172.01 | 134.56 | 127.38 |
| Reused path, new process | 191.46 | 171.03 | 134.80 | 132.21 |

The first application change saves 205.89 ms of new-path first-present latency;
the second saves another 33.80 ms. Together they save 239.69 ms (49.8%) on this
machine. The reused-path combined saving is 56.66 ms (29.6%). These differences
are between cohort medians, not sums of per-stage medians.

| New-path first-process stage (ms) | Original policy | Optimization 1 | Optimizations 1+2 | + mimalloc |
| --- | ---: | ---: | ---: | ---: |
| Device creation | 105.28 | 72.02 | 74.19 | 73.44 |
| Flat pipeline | 189.51 | 12.71 | 13.10 | 12.54 |
| GPU initialization | 380.15 | 169.44 | 172.46 | 170.30 |
| Extra surface resize/configure path | 35.47 | 37.52 | 0.00 | 0.00 |
| Resize prefix, including Direct Manipulation | 0.10 | 0.11 | 0.08 | 0.10 |
| Resume complete → first render starts | 37.35 | 39.80 | 1.61 | 1.83 |
| Main → first GPU completion | 500.90 | 299.35 | 261.96 | 257.15 |

The original and optimization-1 controls each performed four extra surface
configurations before first present; both optimized configurations performed
zero. The optimization-1 control spent 37.52 ms in that path, versus about
0.004 ms with both changes. Its 39.80 ms first-redraw interval fell to 1.61 ms.
This directly attributes most of that interval to redundant configurations;
[wgpu documents that reconfiguration waits for GPU idle and recreates the swapchain](https://docs.rs/wgpu/30.0.1/wgpu/struct.Surface.html#method.configure).

### Readable documents and memory

These cohorts reuse warmed executable paths, use independent processes and
profiles, and have n=6 per variant. README is 9,855 bytes; the larger fixture is
473,920 bytes with 1,000 sections containing CJK text, code, tables and a local
image. Reading/parse/layout work overlaps the UI, so the values below must not
be combined by adding serial phase totals.

| Main → first readable document present (ms) | Original policy | Optimization 1 | Optimizations 1+2 | + mimalloc |
| --- | ---: | ---: | ---: | ---: |
| README | 214.92 | 189.14 | 184.28 | 177.37 |
| Larger fixture | 214.38 | 196.28 | 189.79 | 187.21 |

Document-readable improvement is smaller than empty-window first-present
improvement: 30.64 ms for README and 24.59 ms for the larger fixture. Faster
initial UI presentation can precede the worker's first readable result; reporting
these separately avoids treating a loading frame as a readable document.

The following are whole-process Windows counters after approximately four
seconds, with both application changes enabled. Private Bytes is committed
private memory, not the same quantity as resident physical memory.

| Fixture / allocator | Private Bytes (MiB) | Working Set (MiB) | Peak Working Set (MiB) | CPU time (ms) |
| --- | ---: | ---: | ---: | ---: |
| README / System | 158.62 | 178.81 | 178.81 | 312.50 |
| README / mimalloc default | 250.60 | 186.42 | 186.70 | 273.44 |
| Larger fixture / System | 178.15 | 197.49 | 197.49 | 562.50 |
| Larger fixture / mimalloc default | 274.01 | 210.22 | 210.33 | 515.62 |

Default mimalloc saves only several milliseconds of readable-frame time, while
Private Bytes rises by about 92–96 MiB and Working Set by about 8–13 MiB. It is
therefore rejected; its implementation and dependencies were removed from the
application after this comparison.

A separate exploratory follow-up sets `MIMALLOC_PAGE_COMMIT_ON_DEMAND=1` using
the same mimalloc binary. The option was confirmed in its verbose startup log.
Each fixture has n=3; these samples were taken after, rather than interleaved
with, the main experiment.

| On-demand commit fixture | Private Bytes (MiB) | Working Set (MiB) | Main → readable present (ms) |
| --- | ---: | ---: | ---: |
| README | 184.13 | 188.12 | 183.88 |
| Larger fixture | 199.64 | 209.64 | 189.19 |

On-demand commit reduces the default mimalloc commitment, but it still exceeds
the system allocator by about 21–26 MiB of Private Bytes and 9–12 MiB of Working
Set, with similar readable-frame times. No allocator tuning is installed as a
production default.

### Keep pre-entry scanning separate

| Process creation → main (ms) | Original policy | Optimization 1 | Optimizations 1+2 | + mimalloc |
| --- | ---: | ---: | ---: | ---: |
| New path, first process | 1406.88 | 1405.11 | 1389.02 | 134.93 |
| Same path, second process | 13.55 | 15.39 | 15.39 | 13.92 |
| Reused path, new process | 12.80 | 12.83 | 16.80 | 16.50 |

The mimalloc binary's large pre-entry advantage is not assigned to Rust heap
speed. A separate Defender recording using neutral new filenames measured:

| Build | Process creation → main (ms) | EXE OnOpen scan (ms) |
| --- | ---: | ---: |
| System allocator, both application changes | 1672.53 | 1611.31 |
| mimalloc, both application changes | 137.41 | 86.92 |

Both scans are reported as `Not skipped`. This localizes most of the pre-entry
difference to scanning the different executable bytes. The engine's reason for
the different scan costs remains unclassified; this is not a guaranteed
allocator-runtime benefit or a promise about another build. Antivirus settings
were not changed. Selected records are in
`allocator-followup/defender-relevant.json`; the full ETL remains on Windows.

### Validation and artifacts

The final working tree passes 2,121 workspace tests, with 12 existing ignores,
and the local format, clippy, compile, icon and infrastructure checks. Analysis
has 16 tests; the platform-boundary checker has 12 tests. The 14 instrumented
source files reproduce exactly after formatting from the production changes
and `instrument.py` plus `instrument_optimizations.py`.

Native smoke renders validate CJK, typography, tables and a loaded local image.
All four configurations have identical pixels in the content rectangle
`(0,70)–(2086,1400)`. Full-frame images differ in the animated tab and, for
mimalloc, the right scrollbar state. The comparison excludes the 40-logical-pixel
tab bar and the 8-logical-pixel scrollbar track (14 physical pixels at this DPR);
full-frame difference bounds and the difference image are retained. Both
allocator builds pass native resize, minimize, restore and subsequent-present
checks. This does not claim a whole-frame static UI match.

The first smoke launcher treated PowerShell's null exit code as failure.
This was reproduced with the native GUI executable: `Start-Process` followed by
`WaitForExit` reported `HasExited=true, ExitCode=null`, while `Start-Process -Wait`
reported code 0. Smoke now uses `-Wait`; the interactive resize check directly
owns a .NET process handle. The 120 timing processes already used directly owned
handles and all recorded exit code 0. The failed launcher status and logs remain
under `harness-diagnostics`, and the corrected functional run exited 0.

Data and scripts are under `artifacts/windows-startup/`: `opt-20261110`,
`optimization-summary.json`, `allocator-followup`, `optimization-raw.zip`,
`run_optimizations.ps1`, `run_optimization_sample.ps1`,
`verify_optimization_smoke.py` and `optimization-report`. The temporary PDF
includes the results, earlier Vulkan comparison, cache limits and methodology.
Only the two application optimizations, their regression tests and this
investigation are retained in the repository. The allocator experiment remains
in the local evidence, rather than in the application's features or dependencies.

## Reproduction and validation

The independent Windows source copy and raw evidence remain under
`C:\Users\zurry\Documents\code\markview-startup`. Local probes, scripts,
summaries and selected raw evidence are under `artifacts/windows-startup/`.
The accompanying evidence archive contains `instrument.patch`, the pinned
GPU harness, `instrument_hal.py`, PowerShell launchers, per-process logs,
metadata and the relevant Defender summary. Full Defender ETL stays on the
Windows machine.

The ablation setting is:

```powershell
$env:WGPU_VALIDATION_INDIRECT_CALL = '0'
```

Use the supplied interactive-session launchers to repeat the measurements;
an SSH service-session window is not a desktop startup measurement. Keep the
executable path and configuration state explicit, and retain both present and
completion timestamps. The profiler's extra wait belongs after first present.

Validation includes three paired GUI controls, native-call probes, a
ten-process warm baseline, five parser tests, and native `smoke` renders with
indirect validation enabled and disabled. The smoke runs produced identical
pixels throughout the 2100 × 1330 reader area below the tab bar. Their only
whole-frame difference was the animated tab-label region. This validates the
measured fixture, not all future indirect-command uses or a production patch.

The initial diagnostic documentation did not require a release changelog entry.
