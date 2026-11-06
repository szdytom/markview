#!/usr/bin/env python3
"""Prepare deterministic integration seeds without downloads."""
from pathlib import Path

root = Path(__file__).resolve().parents[1]
repo = root.parent
for target in ("reading_pipeline", "webpage_layout", "layout_diff", "highlight_schedule", "image_decode", "reparse", "paginate_pipeline", "fonts", "shaping", "math", "interaction_sequence", "font_archive"):
    (root / "corpus" / target).mkdir(parents=True, exist_ok=True)

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
web.joinpath("malformed-html").write_bytes(b"\x02<table><tr><td rowspan=0><details><summary>broken")
for seed in range(16):
    web.joinpath(f"article-{seed}").write_bytes(bytes([0]) + bytes((seed * 17 + at * 31) % 256 for at in range(64)))

highlight = root / "corpus" / "highlight_schedule"
for index, controls in enumerate([
    bytes([0, 0, 0, 0, 0, 0, 0, 0]),
    bytes([1, 3, 0, 4, 5, 2, 6, 7]),
    bytes([0, 1, 2, 3, 4, 5, 6, 7]),
]):
    highlight.joinpath(f"queue-{index}").write_bytes(
        b"HLP1" + controls + b"fn main() { let value = 1; }\n"
    )

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
