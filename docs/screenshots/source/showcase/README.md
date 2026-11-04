# Document showcase sources

The README previews are native `markview render` output from real source excerpts.
English uses original text; Chinese uses a published Rust translation, the
Chinese Wikipedia article, a Chinese translation of the manifesto and
a showcase translation of the OpenStax excerpt. The OpenStax translation is not
a published or author-approved edition. All headings, captions and prose in the Chinese
previews are localized; program identifiers and required OpenStax credit retain
their original spelling.

## Long-form prose

- Text: Aaron Swartz, [Guerilla Open Access Manifesto](https://archive.org/details/GuerillaOpenAccessManifesto), July 2008, Eremo, Italy. The English text is in `en/prose.md`; the Chinese translation is in `zh/prose.md`. The translator is not identified in the available source.
- The preview shows the opening of this long text, without an illustration. Author and title appear in the credit strip. The original work and Chinese translation are third-party material and are not relicensed under the repository's MIT license.

## Technical documentation

- English: [The Rust Programming Language, Data Types](https://doc.rust-lang.org/book/ch03-02-data-types.html), from [rust-lang/book](https://github.com/rust-lang/book/blob/main/src/ch03-02-data-types.md).
- Chinese: [Rust 程序设计语言：数据类型](https://kaisery.github.io/trpl-zh-cn/ch03-02-data-types.html), from [KaiserY/trpl-zh-cn](https://github.com/KaiserY/trpl-zh-cn/blob/master/src/ch03-02-data-types.md).
- Selected introduction, annotated code example, integer explanation, first table and the following signed/unsigned discussion. Compiler diagnostics and the intervening scalar-type introduction are omitted. Reference links are expanded to absolute URLs and presentation-only HTML is removed.
- Original book: MIT or Apache-2.0; Chinese translation: MIT, copyright 2017–2018 Rust 中文社区. The retained [MIT notices](LICENSE-RUST-MIT.txt) apply to these excerpts.

## Mathematics

- Edwin “Jed” Herman and Gilbert Strang, OpenStax / Rice University, [Calculus Volume 3, §6.7 Stokes' Theorem](https://openstax.org/books/calculus-volume-3/pages/6-7-stokes-theorem).
- The excerpt starts at the **Stokes' Theorem** subsection, after the learning objectives and introductory overview. It follows the definition, Theorem 6.19, the relation to Green's theorem and the beginning of the informal proof. Figures are omitted; references link to the original. HTML/MathML is represented as Markdown/LaTeX, preserving mathematical meaning. Chinese translates this selected prose; the flat-surface equality is set on its own line for clarity.
- OpenStax content, its Chinese translation and the corresponding `*-showcase-math.png` previews remain under [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/), **not** the repository's MIT license. Copyright Rice University. These assets are a noncommercial demonstration of mathematical reading.
- The required credit appears on each image: “Download for free at https://openstax.org/details/books/calculus-volume-3.”

## Web extraction

- English: [Webb's First Deep Field](https://en.wikipedia.org/wiki/Webb%27s_First_Deep_Field).
- Chinese: [韋伯的首次深空](https://zh.wikipedia.org/wiki/%E9%9F%8B%E4%BC%AF%E7%9A%84%E9%A6%96%E6%AC%A1%E6%B7%B1%E7%A9%BA), fetched as the `zh-cn` simplified-Chinese variant, retaining its regional terminology.
- `en/web.html` and `zh/web.html` retain Wikipedia's `action=render` article HTML, retrieved on 2026-10-07. A UTF-8 wrapper adds the page title omitted by that endpoint. Content and source links are retained.
- `web-extracted.md` is a frozen record of the retired Markdown-serializing extractor: the current extractor lowers the article into a semantic document instead. `web.md` selects the lead and first subsection from that record. Generated source metadata and the Chinese infobox are omitted from the **display excerpt**, while all full extraction output remains available for inspection. The prose and its hyperlinks are not rewritten; infobox images dropped by the extractor are not added back.
- Wikipedia contributors retain copyright. Text, adaptations and corresponding `*-showcase-web.png` previews are under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/), **not** the repository's MIT license. The original pages and their histories provide contributor attribution.

## Regeneration

Edit the localized Markdown files to select another passage. Render all eight
previews with a release binary and GPU access:

```sh
python3 scripts/generate_readme_showcase.py
```

Rendered frames are 1440 × 1600 physical pixels at scale 2, with a 650-logical-pixel
reading column. Body type is 18 px, or 14 px for the technical page so its code
and table fit together. A separate 64-pixel credit strip is appended. The reader
loads live font resources and runs offline; no fonts or live settings are changed.
