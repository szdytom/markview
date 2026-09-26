#!/usr/bin/env python3
"""Regenerate the pinned test fonts under `crates/markview-core/tests/fonts`.

The layout tests assert exact geometry, so they must not shape with whatever
fonts the machine happens to have installed: the same test then passes on
macOS and fails on Linux or Windows. The unit tests register these subsets
with system fonts disabled, and the integration tests point their font
configuration at this directory. Either way the stylesheet finds the subsets
by the family names its `lookfor` lists already ask for.

Run this from the repository root with `fontTools` (and a Noto installation):

    python3 scripts/generate_test_fonts.py

The subsets are committed, so regenerating is only needed after the tests
start using characters the current subsets do not cover. Keep the output small
by subsetting to the characters the tests actually use.
"""

import glob
import os
import re
import sys

from fontTools import subset
from fontTools.ttLib import TTCollection, TTFont

# Rust spells characters outside the source as `\u{XXXX}`; those never appear
# literally, so the scan has to decode them too.
ESCAPE = re.compile(r"\\u\{([0-9a-fA-F]+)\}")

NOTO = "/usr/share/fonts/noto"
NOTO_CJK = "/usr/share/fonts/noto-cjk"
OUT = "crates/markview-core/tests/fonts"

# (source, face index, output file). The face index selects one family out of a
# `.ttc`, so a single Noto CJK collection provides the SC faces per style.
FONTS = [
    (f"{NOTO}/NotoSerif-Regular.ttf", 0, "NotoSerif-Regular-subset.otf"),
    (f"{NOTO}/NotoSerif-Bold.ttf", 0, "NotoSerif-Bold-subset.otf"),
    (f"{NOTO}/NotoSerif-Italic.ttf", 0, "NotoSerif-Italic-subset.otf"),
    (f"{NOTO}/NotoSans-Regular.ttf", 0, "NotoSans-Regular-subset.otf"),
    (f"{NOTO}/NotoSans-Bold.ttf", 0, "NotoSans-Bold-subset.otf"),
    (f"{NOTO}/NotoSans-Italic.ttf", 0, "NotoSans-Italic-subset.otf"),
    (f"{NOTO}/NotoSansMono-Regular.ttf", 0, "NotoSansMono-Regular-subset.otf"),
    (f"{NOTO}/NotoSansMono-Bold.ttf", 0, "NotoSansMono-Bold-subset.otf"),
    (f"{NOTO_CJK}/NotoSerifCJK-Regular.ttc", 2, "NotoSerifCJKsc-Regular-subset.otf"),
    (f"{NOTO_CJK}/NotoSerifCJK-Bold.ttc", 2, "NotoSerifCJKsc-Bold-subset.otf"),
    (f"{NOTO_CJK}/NotoSansCJK-Regular.ttc", 2, "NotoSansCJKsc-Regular-subset.otf"),
    (f"{NOTO_CJK}/NotoSansCJK-Medium.ttc", 2, "NotoSansCJKsc-Medium-subset.otf"),
    (f"{NOTO_CJK}/NotoSansCJK-Bold.ttc", 2, "NotoSansCJKsc-Bold-subset.otf"),
    (f"{NOTO_CJK}/NotoSansCJK-Regular.ttc", 7, "NotoSansMonoCJKsc-Regular-subset.otf"),
    (f"{NOTO_CJK}/NotoSansCJK-Bold.ttc", 7, "NotoSansMonoCJKsc-Bold-subset.otf"),
    (f"{NOTO}/NotoColorEmoji.ttf", 0, "NotoColorEmoji-subset.ttf"),
]


def test_text():
    """Every character the tests can reach, plus printable ASCII.

    The sources are scanned as text rather than parsed, so a comment can only
    add a glyph, never drop one.
    """
    sources = (
        glob.glob("crates/**/*.rs", recursive=True)
        + glob.glob("src/**/*.rs", recursive=True)
        + glob.glob("tests/**/*.rs", recursive=True)
        + glob.glob("tests/fixtures/*.md")
    )
    chars = {chr(c) for c in range(0x20, 0x7F)} | {"\t", "\n"}
    for path in sources:
        with open(path, encoding="utf-8") as source:
            text = source.read()
        chars.update(text)
        if path.endswith(".rs"):
            chars.update(chr(int(code, 16)) for code in ESCAPE.findall(text))
    return "".join(sorted(c for c in chars if c != "\r"))


def main():
    if not os.path.isdir(OUT):
        sys.exit(f"{OUT} does not exist; run from the repository root")
    text = test_text()
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    options.recalc_bounds = True
    total = 0
    for source, index, name in FONTS:
        if not os.path.exists(source):
            sys.exit(f"missing source font {source}")
        font = (
            TTCollection(source).fonts[index]
            if source.endswith(".ttc")
            else TTFont(source)
        )
        subsetter = subset.Subsetter(options=options)
        subsetter.populate(text=text)
        subsetter.subset(font)
        path = os.path.join(OUT, name)
        font.save(path)
        total += os.path.getsize(path)
        print(f"{name:42} {os.path.getsize(path) / 1024:8.1f} KiB")
    print(f"{'total':42} {total / 1024:8.1f} KiB")


if __name__ == "__main__":
    main()
