# MVaaC font codec verification

Measured on 2026-10-02 with release WASM, wasm-bindgen 0.2.129 and headless
Chromium on Linux, Intel Core Ultra 5 125H, Rust 1.98.1. These are diagnostic observations,
not performance promises for other devices or network connections.

## Decoder and format coverage

The optional `markview-web/woff` feature uses
[wuff 0.2.9](https://github.com/nicoburns/wuff) with pure Rust `brotli` and `z`
features. wuff is MIT; brotli-decompressor is BSD-3-Clause OR MIT and flate2
is MIT OR Apache-2.0. No C decoder/toolchain is required. Official Web builds
enable the feature; both enabled and disabled native tests and WASM Clippy/builds
were verified. The disabled WASM also rejects WOFF input at runtime with the
feature-specific error.

Regression fixtures derive from the existing licensed Noto subsets:

| Input | Verified behavior |
| --- | --- |
| OTF/CFF | Original Serif faces and WOFF/WOFF2 decode, validate and paint |
| TTF/color bitmap tables | Original Emoji face and WOFF/WOFF2 decode, validate and paint |
| TTC | Uncompressed Serif + Sans collection validates and paints |
| Invalid/truncated bytes | Recognizable rejection, including input index; no partial font-set installation |

This set does not establish complete conformance for variable fonts, CFF2 or
compressed collections. Metadata/private WOFF tables are not exposed by the
component. Browser CSS font registration is unrelated to the WASM text faces.
Test fixtures never enter package assets; tests check that original text-font
bytes are absent from WASM. Only KaTeX math faces are embedded.

## Binary size

Same font-stage source state, release optimization and wasm-bindgen processing:

| Variant | WASM bytes | gzip bytes |
| --- | ---: | ---: |
| `--no-default-features` | 14,890,578 | 7,100,230 |
| `--features woff` | 15,133,719 | 7,205,806 |
| Decoder increase | 243,141 (1.63%) | 105,576 (1.49%) |

The pre-iteration baseline WASM was 14,864,809 bytes. The codec comparison
above isolates the build feature; it does not compare unrelated source changes.
Compression uses Python gzip with a fixed timestamp.

## Initialization and face input

`web/measure-fonts.mjs` loads already-fetched bytes into seven isolated pages in
one browser process and takes medians. Compilation caches can warm within that
process. Times exclude fetching, GPU initialization, layout and painting.

| Operation | Decoder disabled | Decoder enabled |
| --- | ---: | ---: |
| `WebAssembly.compile` | 28.0 ms | 30.0 ms |
| Instantiate compiled module | 9.5 ms | 9.8 ms |
| Serif OTF FontSet input | below timer resolution | below timer resolution |
| Serif WOFF FontSet input | explicit rejection | 1.2 ms |
| Serif WOFF2 FontSet input | explicit rejection | 2.2 ms |

Font input medians pool ten inputs per page. These small subset faces are
approximately 55 KB OTF, 28 KB WOFF and 23 KB WOFF2; full CJK/variable fonts may
have substantially different decode cost. Inputs are decoded once per set;
readers clone shared font configuration, and the helper reuses cached sets.

To reproduce, build both variants and save their processed binaries at
`artifacts/mvaac-woff/{disabled,enabled}.wasm`, keep matching official bindings,
and run `node web/measure-fonts.mjs`. Raw samples are written to
`artifacts/mvaac-woff/timings.json`; generated binaries and samples stay ignored.
