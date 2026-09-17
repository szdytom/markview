#!/usr/bin/env python3
"""Generate stress fixtures that the standard benchmarks cannot expose.

`tests/fixtures/*-100k.md` repeat a handful of paragraph bodies, and the layout
block cache is keyed by content, so a few cache entries cover every block. That
hides both the 256-entry cache cap and per-glyph memory growth. These fixtures
give every block distinct content, and add shapes that stress other caches:

  unique-100k.md      438 distinct prose paragraphs (100 KiB)
  unique-400k.md     1747 distinct prose paragraphs (400 KiB)
  many-blocks-100k.md 4000 distinct one-line paragraphs
  many-code-100k.md   300 fenced code blocks, past the 256-entry highlight cache
  text-cjk-1000k.md   ten copies of text-cjk-100k.md (1 MiB)

The output directory is ignored by Git by default.
"""
import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORDS = [
    "layout", "paragraph", "measure", "glyph", "justify", "column", "reader",
    "document", "render", "section", "analysis", "performance", "latency",
    "memory", "shape", "advance", "font", "baseline", "margin", "indent",
]


def unique_prose(target, path):
    parts, size, index = [], 0, 0
    while size < target - 200:
        body = " ".join(
            WORDS[(index * 7 + j) % len(WORDS)] for j in range(28))
        part = f"Paragraph {index}: {body}.\n\n"
        parts.append(part)
        size += len(part.encode())
        index += 1
    Path(path).write_text("".join(parts), encoding="utf-8")
    print(f"{path}: {Path(path).stat().st_size} bytes, {index} paragraphs")


def many_blocks(path):
    parts = [
        f"Paragraph {i}: a short line of ordinary prose that stays on one "
        "line.\n\n"
        for i in range(4000)
    ]
    Path(path).write_text("".join(parts), encoding="utf-8")
    print(f"{path}: {Path(path).stat().st_size} bytes, 4000 paragraphs")


def many_code(path):
    blocks = []
    for i in range(300):
        lines = "\n".join(
            f"    let value_{i}_{j} = compute({j}) + {i};" for j in range(8))
        blocks.append(
            f"## Section {i}\n\n```rust\nfn run_{i}() {{\n{lines}\n}}\n```\n\n")
    Path(path).write_text("".join(blocks), encoding="utf-8")
    print(f"{path}: {Path(path).stat().st_size} bytes, 300 code blocks")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", default="artifacts/perf-analysis")
    args = parser.parse_args()
    out = ROOT / args.output_dir
    out.mkdir(parents=True, exist_ok=True)
    unique_prose(100 * 1024, out / "unique-100k.md")
    unique_prose(400 * 1024, out / "unique-400k.md")
    many_blocks(out / "many-blocks-100k.md")
    many_code(out / "many-code-100k.md")
    unit = (ROOT / "tests/fixtures/text-cjk-100k.md").read_text(
        encoding="utf-8")
    (out / "text-cjk-1000k.md").write_text(unit * 10, encoding="utf-8")
    print(f"{out / 'text-cjk-1000k.md'}: {(out / 'text-cjk-1000k.md').stat().st_size} bytes")


if __name__ == "__main__":
    main()
