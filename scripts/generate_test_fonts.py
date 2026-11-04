#!/usr/bin/env python3
"""Regenerate the pinned test fonts under `crates/markview-layout/tests/fonts`.

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
from fontTools.ttLib import TTFont

# Rust spells characters outside the source as `\u{XXXX}`; those never appear
# literally, so the scan has to decode them too.
ESCAPE = re.compile(r"\\u\{([0-9a-fA-F]+)\}")

NOTO = "/usr/share/fonts/noto"
NOTO_CJK = "/usr/share/fonts/noto-cjk"
OUT = "crates/markview-layout/tests/fonts"

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
        + glob.glob("tests/fixtures/**/*.md", recursive=True)
        + glob.glob("crates/markview-render/tests/fixtures/*.md")
        + glob.glob("assets/locales/*.toml")
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
            TTFont(source, fontNumber=index)
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
    render_regional_fonts(options)
    render_synthetic_bold_font(options)


def render_regional_fonts(options):
    """Pin TC and JP faces for regional punctuation and glyph comparisons."""
    out = "crates/markview-render/tests/fonts"
    os.makedirs(out, exist_ok=True)
    text = "".join(
        open(path, encoding="utf-8").read()
        for path in glob.glob("crates/markview-render/tests/fixtures/*.md")
    ) + "".join(chr(c) for c in range(0x20, 0x7F))
    for region, index in [("tc", 3), ("jp", 0)]:
        for family in ["Serif", "Sans", "SansMono"]:
            for weight in ["Regular", "Bold"]:
                source = f"{NOTO_CJK}/Noto{'Sans' if family == 'SansMono' else family}CJK-{weight}.ttc"
                font = TTFont(source, fontNumber=index + (5 if family == "SansMono" else 0))
                subsetter = subset.Subsetter(options=options)
                subsetter.populate(text=text)
                subsetter.subset(font)
                name = f"Noto{family}CJK{region}-{weight}-subset.otf"
                font.save(os.path.join(out, name))
                print(f"{name:42} {os.path.getsize(os.path.join(out, name)) / 1024:8.1f} KiB")


def render_synthetic_bold_font(options):
    """Pin one face under a unique family for deterministic synthetic bold."""
    out = "crates/markview-render/tests/fonts"
    fixture = "crates/markview-render/tests/fixtures/synthetic-bold.md"
    with open(fixture, encoding="utf-8") as source:
        text = source.read()
    text += "".join(chr(c) for c in range(0x20, 0x7F))
    font = TTFont(f"{NOTO_CJK}/NotoSansCJK-Medium.ttc", fontNumber=2)
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(text=text)
    subsetter.subset(font)
    # The source family also ships a bold face. A unique typographic family
    # makes the single Medium face the only match at every requested weight.
    family = "Markview No Bold CJK"
    names = font["name"]
    for platform, encoding, language in {
        (record.platformID, record.platEncID, record.langID)
        for record in names.names
    }:
        names.setName(family, 1, platform, encoding, language)
        names.setName("Regular", 2, platform, encoding, language)
        names.setName(family, 4, platform, encoding, language)
        names.setName(
            family.replace(" ", ""), 6, platform, encoding, language
        )
        names.setName(family, 16, platform, encoding, language)
        names.setName("Regular", 17, platform, encoding, language)
    path = os.path.join(out, "MarkviewNoBoldCJK-subset.otf")
    font.save(path)
    print(f"{os.path.basename(path):42} {os.path.getsize(path) / 1024:8.1f} KiB")


if __name__ == "__main__":
    main()
