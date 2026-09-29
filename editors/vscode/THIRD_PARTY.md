# Third-party components

The application depends on the versions pinned in `Cargo.lock`. Dependency
source and original license texts are available in Cargo's registry packages.

| Component | Role | License |
| --- | --- | --- |
| winit, wgpu | Native windows and GPU rendering | Apache-2.0 OR MIT |
| Comrak | CommonMark and GFM parser | BSD-2-Clause |
| codex / chinese-number | Numbering patterns and numeral systems for ordered lists | Apache-2.0 (codex), MIT (chinese-number) |
| krilla / pdf-writer | PDF content, font subsetting and serialization | MIT OR Apache-2.0 |
| Skrifa / read-fonts | OpenType tables and variable-font instances | Apache-2.0 OR MIT |
| subsetter | Font subsetting for embedded text | MIT OR Apache-2.0 |
| Parley / Fontique | Shaping, font matching and Unicode analysis | Apache-2.0 OR MIT |
| ICU4X | Unicode segmentation | Unicode-3.0 |
| hypher | English hyphenation patterns | MIT OR Apache-2.0 |
| RaTeX | LaTeX mathematics parsing and layout | MIT |
| Swash | Glyph rasterization | Apache-2.0 OR MIT |
| tiny-skia | Mathematical path rasterization | BSD-3-Clause |
| mermaid-rs-renderer | Mermaid diagram parsing, layout and SVG output | MIT |
| notify | Filesystem observation | CC0-1.0 |
| open | Opening links with the system browser | MIT |
| rfd | Native file dialogs | MIT |
| arboard / wl-clipboard-rs | Platform clipboard integration | MIT OR Apache-2.0 |
| windows-sys | Attaching to the console that launched the reader | MIT OR Apache-2.0 |
| unicode-segmentation | Grapheme boundaries for reading selections | MIT OR Apache-2.0 |
| tempfile | Atomic settings replacement and tests | MIT OR Apache-2.0 |
| usvg | Compile-time parsing of the SVG icon sources | Apache-2.0 OR MIT |
| Lucide | Geometry of the `open` and `close` icons in `assets/ui` | ISC |

KaTeX mathematical fonts are embedded by `ratex-katex-fonts`. Their SIL Open
Font License is reproduced in `licenses/KaTeX-OFL.txt`; keep that file with
redistributed binaries. Markview does not bundle the JavaScript KaTeX runtime.
Body/UI fonts are discovered from the operating system and are not distributed
with this repository. Unit tests shape with the pinned Noto subsets under
`crates/markview-core/tests/fonts`; their SIL Open Font License is reproduced in
`licenses/Noto-OFL.txt`. The Lucide icon geometry under `assets/ui` is ISC
licensed; its license is reproduced in `licenses/Lucide-ISC.txt`.

Before packaging a release, include notices for the complete dependency tree,
not only this architectural summary. `cargo metadata --locked` records that
tree and each crate's declared license.
