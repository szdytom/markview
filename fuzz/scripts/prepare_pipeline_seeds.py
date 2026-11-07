#!/usr/bin/env python3
"""Prepare deterministic fuzz seeds without downloads.

A shared Markdown pool is copied into every target whose unit is a document.
Without one, the structural oracles start from an empty corpus and spend their
first blocks rediscovering headings and lists.
"""
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
repo = root.parent
corpus = root / "corpus"
fixtures = repo / "tests" / "fixtures"

TARGETS = (
    "parse",
    "prefix",
    "refdef",
    "sourcepos_content",
    "details_structure",
    "anchors",
    "layout",
    "layout_parallel",
    "pdf",
    "reading_pipeline",
    "webpage_layout",
    "layout_diff",
    "highlight",
    "highlight_schedule",
    "image_decode",
    "reparse",
    "paginate_pipeline",
    "fonts",
    "shaping",
    "math",
    "interaction_sequence",
    "font_archive",
    "webpage",
    "webpage_seam",
)
for target in TARGETS:
    (corpus / target).mkdir(parents=True, exist_ok=True)

# Target unit is Markdown; the value caps the files these targets accept.
MARKDOWN_TARGETS = {
    "parse": 512 * 1024,
    "prefix": 256 * 1024,
    "refdef": 256 * 1024,
    "sourcepos_content": 256 * 1024,
    "details_structure": 256 * 1024,
    "anchors": 64 * 1024,
    "layout": 128 * 1024,
    "layout_parallel": 32 * 1024,
    "pdf": 48 * 1024,
}

# Replaying every spec example costs seconds for the parser and minutes for the
# layout family. Keep the full corpus for `parse`, sample the rest evenly.
SPEC_LIMIT = {
    "prefix": 512,
    "refdef": 512,
    "sourcepos_content": 512,
    "details_structure": 512,
    "anchors": 256,
    "layout": 256,
    "layout_parallel": 64,
    "pdf": 256,
}

# `layout_parallel` pays five paired passes; a 10 KiB document takes tens of
# seconds per input, which starves the rest of the corpus. The PDF text oracle
# cannot yet account for the colour-emoji fallback, so that fixture would fail
# on a legal export.
MARKDOWN_EXCLUDE = {
    "layout_parallel": {
        "fixture-code-10k.md",
        "fixture-details-10k.md",
        "fixture-long-code-10k.md",
        "fixture-math-10k.md",
        "fixture-ordinary-10k.md",
        "fixture-math-cjk-100k.md",
        "fixture-text-cjk-100k.md",
    },
    "pdf": {"fixture-emoji-fallback.md"},
}

FIXTURE_SEEDS = (
    "details-10k.md",
    "footnote.md",
    "math-10k.md",
    "ordinary-10k.md",
    "emoji-fallback.md",
    "code-10k.md",
    "long-code-10k.md",
    "math-cjk-100k.md",
    "text-cjk-100k.md",
)

DIALECT = """---
title: Dialect coverage
tags: [fuzz, seeds]
---

# Heading one

## A heading with an explicit id <a id="explicit-anchor"></a>

Paragraph with **bold**, *italic*, ==mark==, `code`, ~~strike~~,
[link](https://example.org), [ref][ref-one], [^note], ![image](image.png),
$x^2$, and a wikilink [[target]].

<a id="inline-anchor"></a>

<details open>
<summary>Open summary</summary>

Body text with <a name="named-anchor"></a> and ![inner](inner.png).

### Nested heading

</details>

<details>
<summary>Closed summary</summary>

Hidden **body** and [^note].

<details open>
<summary>Nested open</summary>

Deep body.

</details>

</details>

> [!note] Callout title
> Callout body.

<figure>
<img src="figure.png" alt="Figure alt">
<figcaption>Figure caption</figcaption>
</figure>

<dl>
<dt>Term</dt>
<dd>Definition with **bold**.</dd>
<dt>Other term</dt>
<dd>Second definition.</dd>
</dl>

| left | center | right |
| :--- | :----: | ----: |
| one | two | three |

```rust
fn main() { let value = 1; }
```

$$
\\int_0^1 x^2\\,dx
$$

[ref-one]: https://example.org/reference

[^note]: Footnote body with <a id="footnote-anchor"></a>.
"""

REFERENCES = """[alpha]: https://example.org/alpha
[beta]: <https://example.org/beta> "title"

[A][alpha] [B][beta] [alpha]

<details open>
<summary>Summary</summary>

[gamma]: /inside
[G][gamma]

</details>

[^one]: first note
[^one]: shadowed note

Text [^one].
"""


def spec_examples(path: Path):
    """The markdown side of the spec's fenced examples: content up to the
    line holding only the separator period."""
    text = path.read_text(encoding="utf-8", errors="replace")
    return [
        match.group(1)
        for match in re.finditer(
            r"`{20} example\n(.*?)\n\.\n.*?`{20}", text, re.S
        )
    ]


def fit(content: bytes, cap: int) -> bytes:
    """Keep a long document under `cap` by cutting at a line boundary."""
    if len(content) <= cap:
        return content
    cut = content[:cap]
    newline = cut.rfind(b"\n")
    if newline > 0:
        return cut[: newline + 1]
    while cut and (cut[-1] & 0xC0) == 0x80:
        cut = cut[:-1]
    return cut


def write_markdown_seeds(spec_dir: Path | None) -> None:
    seeds: list[tuple[str, bytes]] = []
    for name in FIXTURE_SEEDS:
        path = fixtures / name
        if path.exists():
            seeds.append((f"fixture-{name}", path.read_bytes()))
    seeds.append(("dialect.md", DIALECT.encode()))
    seeds.append(("references.md", REFERENCES.encode()))
    if spec_dir is not None:
        for label, name in (
            ("cmark", "cmark-spec.txt"),
            ("gfm", "gfm-spec.txt"),
        ):
            path = spec_dir / name
            if not path.exists():
                continue
            for index, example in enumerate(spec_examples(path)):
                body = (example + "\n").encode()
                if body.strip():
                    seeds.append((f"spec-{label}-{index:04}.md", body))
    for target, cap in MARKDOWN_TARGETS.items():
        directory = corpus / target
        excluded = MARKDOWN_EXCLUDE.get(target, set())
        for name in excluded:
            directory.joinpath(name).unlink(missing_ok=True)
        if spec_dir is not None:
            for old in directory.glob("spec-*"):
                old.unlink()
        spec = [seed for seed in seeds if seed[0].startswith("spec-")]
        other = [seed for seed in seeds if not seed[0].startswith("spec-")]
        if (limit := SPEC_LIMIT.get(target)) is not None and len(spec) > limit:
            step = (len(spec) + limit - 1) // limit
            spec = spec[::step][:limit]
        for name, content in other + spec:
            if content and name not in excluded:
                directory.joinpath(name).write_bytes(fit(content, cap))


reading = root / "corpus" / "reading_pipeline"
reading.joinpath("unicode.md").write_text("alpha 中文 مرحبا e\u0301 👩‍💻 office\nsecond line alpha\n\n[alpha](https://example.org)\n")
reading.joinpath("atoms.md").write_text("alpha &amp; alpha\n\n$x + y$ ![alt](image.png)\n")
for field in range(7):
    controls = bytes([field * 2 + 1, 0, 0, 7, 0, 1, 2, 3])
    reading.joinpath(f"generated-{field}").write_bytes(b"RDP1" + controls + bytes(range(48)))
reading.joinpath("boundary-64k").write_bytes(
    b"RDB1" + bytes([255, 7, 0, 0, 0, 0, 0, 0]) + b"tail alpha"
)

web = root / "corpus" / "webpage_layout"
web.joinpath("span-disclosures-multipage").write_bytes(bytes([1]) + bytes(range(24)))
web.joinpath("span-empty-grid").write_bytes(bytes([1]))
web.joinpath("plain-cell-geometry").write_bytes(bytes([2, 0, 1, 2, 3]))
web.joinpath("malformed-html").write_bytes(b"\x03<table><tr><td rowspan=0><details><summary>broken")
for name in ("web-plain-article.html", "web-complex-table.html"):
    page = fixtures / name
    web.joinpath(f"raw-{page.stem}").write_bytes(b"\x03" + page.read_bytes())
for seed in range(16):
    web.joinpath(f"article-{seed}").write_bytes(bytes([0]) + bytes((seed * 17 + at * 31) % 256 for at in range(64)))

web_pages = root / "corpus" / "webpage"
for page in sorted(fixtures.glob("web-*.html")):
    web_pages.joinpath(page.name).write_bytes(page.read_bytes())

highlight = root / "corpus" / "highlight_schedule"
for index, controls in enumerate([
    bytes([0, 0, 0, 0, 0, 0, 0, 0]),
    bytes([1, 3, 0, 4, 5, 2, 6, 7]),
    bytes([0, 1, 2, 3, 4, 5, 6, 7]),
]):
    highlight.joinpath(f"queue-{index}").write_bytes(
        b"HLP1" + controls + b"fn main() { let value = 1; }\n"
    )

highlight = root / "corpus" / "highlight"
for language, code in {
    "rust": "fn main() {\n    let value = 1;\n}\n",
    "python": "def main():\n    return {1: 2}\n",
    "javascript": "const value = {key: 1};\n",
    "json": '{"key": [1, true, null]}\n',
    "yaml": "key:\n  - one\n  - two\n",
    "toml": "[section]\nkey = 1\n",
    "c": "int main(void) { return 0; }\n",
    "cpp": "template <class T> T id(T v) { return v; }\n",
    "css": "body { color: #fff; }\n",
    "html": "<article><p>text</p></article>\n",
    "shell": "for value in one two; do echo $value; done\n",
    "markdown": "# Heading\n\n- item\n",
}.items():
    highlight.joinpath(language).write_bytes(
        (language + "\0" + code).encode()
    )
highlight.joinpath("raw").write_bytes(b"rawtext\0plain code body\n")

images = root / "corpus" / "image_decode"
controls = bytes(12)
images.joinpath("invalid").write_bytes(b"IDC1" + controls + b"not an image")
images.joinpath("truncated-png").write_bytes(
    b"IDC1" + controls + (repo / "assets/icons/markview-16.png").read_bytes()[:40]
)
for name in ("markview-16.png", "markview.ico"):
    images.joinpath(name).write_bytes(
        b"IDC1" + bytes([80, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0])
        + (repo / "assets/icons" / name).read_bytes()
    )
images.joinpath("sample.gif").write_bytes(
    b"IDC1" + controls + (repo / "examples/images/sample.gif").read_bytes()
)
svg = b'<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><text x="1" y="15" font-family="sans-serif" font-size="14">Ab</text><rect width="40" height="20" fill="none"/></svg>'
images.joinpath("svg-text").write_bytes(b"IDC1" + controls + svg)
images.joinpath("svg-zero-target").write_bytes(
    b"IDC1" + bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]) + svg
)
images.joinpath("pdf-bitmap").write_bytes(
    b"IDC1" + bytes([16, 0, 0, 0, 16, 0, 0, 0, 0, 0, 1, 0])
    + (repo / "assets/icons/markview-16.png").read_bytes()
)

reparse = root / "corpus" / "reparse"
reparse_body = b"# title\n\nparagraph **bold**\n\n> quote\n\n- item\n\n~~~rust\nlet x = 1;\n~~~\n"
reparse_pool = b"## heading\n\n- inserted\n\n[ref]: https://example.org\n\nalpha"
reparse_records = [(3, 0, 9), (20, 5, 8), (40, 2, 6), (0, 1, 4)]
reparse_input = b"RPS1" + bytes([len(reparse_records)]) + len(reparse_body).to_bytes(4, "little") + len(reparse_pool).to_bytes(4, "little")
for start, delete, insert in reparse_records:
    reparse_input += start.to_bytes(4, "little") + delete.to_bytes(4, "little") + insert.to_bytes(2, "little")
reparse_input += reparse_pool + reparse_body
reparse.joinpath("syntax-history").write_bytes(reparse_input)

font_seeds = root / "corpus" / "fonts"
font_seeds.joinpath("committed-face").write_bytes(b"FNT1\x00\x00alpha beta")
font_seeds.joinpath("mutated-face").write_bytes(b"FNT1\x01\x08" + bytes(range(16)))
shaping = root / "corpus" / "shaping"
shaping.joinpath("scripts").write_bytes(b"SHP1\x10\x00\x00" + bytes(range(48)))
shaping.joinpath("plain").write_bytes(b"SHP1\x00\x01\x01" + b"alpha")
math = root / "corpus" / "math"
math.joinpath("valid-fragments").write_bytes(b"MTH1\x10\x00\x00" + bytes(range(32)))
math.joinpath("deep-scripts").write_bytes(b"MTH1\x00\x01\x01" + b"\\frac{\\sqrt{x^2}}{\\sum_{i=0}^{n} i}")

interactions = root / "corpus" / "interaction_sequence"
interactions.joinpath("selection-overflow").write_bytes(
    b"INT1" + bytes([60, 7, 1, 9, 0, 2, 3, 4, 5, 6, 1, 0]) + b"# Heading\n\nalpha beta gamma with several words to select and drag across the line.\n\n[link](https://example.org)\n\n| a | b | c | d | e | f | g | h |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| one | two | three | four | five | six | seven | eight |\n\n<details>\n<summary>summary</summary>\n\nhidden alpha\n\n</details>\n"
)

archives = root / "corpus" / "font_archive"
for container in range(4):
    controls = bytearray(12)
    controls[0] = container
    archives.joinpath(f"valid-{container}").write_bytes(
        b"ARC1" + bytes(controls) + bytes([container, 0]) + b"member contents alpha beta gamma delta"
    )
archives.joinpath("damaged").write_bytes(
    b"ARC1" + bytes(12) + bytes([0, 1]) + b"PK\x03\x04broken archive member"
)

pages = root / "corpus" / "paginate_pipeline"
pages.joinpath("nested-multipage.md").write_bytes(
    b"\x02\x01# Heading\n\nfirst paragraph with enough words to wrap across a line and then some more text.\n\n> quoted words\n> on two lines\n\n| left | right |\n| --- | --- |\n| one | two |\n\n<details>\n<summary>summary</summary>\n\nhidden body text\n\n</details>\n\n- first\n- second\n- third\n"
)
pages.joinpath("short.md").write_bytes(b"\x00\x00alpha")

history = root / "corpus" / "layout_diff"
body = b'[alpha](https://example.org)\n\n![pixels](image.png)\n\n<details>\n<summary>summary</summary>\n\nhidden alpha\n\n</details>\n'
ops = bytes([0, 0, 0, 3, 0, 0, 2, 5, 2, 0, 5, 73, 3, 1, 9, 0, 4, 1, 6, 0, 7, 0, 8, 0, 3, 0])
history.joinpath("state-history").write_bytes(b"LDH1" + bytes([len(ops) // 2]) + ops + body)
for op in range(10):
    history.joinpath(f"operation-{op}").write_bytes(b"LDH1\x01" + bytes([op, 73]) + body)

seam = root / "corpus" / "webpage_seam"
for kind in range(12):
    seam.joinpath(f"block-{kind:02}").write_bytes(
        bytes([0, kind]) + bytes(range(16))
    )
seam.joinpath("mixed").write_bytes(bytes([4]) + bytes(range(1, 65)))

spec_dir = None
if "--spec-dir" in sys.argv:
    spec_dir = Path(sys.argv[sys.argv.index("--spec-dir") + 1])
write_markdown_seeds(spec_dir)
print(
    "seeds: "
    + " ".join(
        f"{target}={sum(1 for _ in (corpus / target).iterdir())}"
        for target in TARGETS
    )
)
