#!/usr/bin/env python3
"""Render the WebView-versus-Markview typography figure used by the READMEs.

Both panels set the same Markdown to the same measure, type size and font, and
both ask their engine for justified text. The left panel is a headless Chromium
with a conventional Markdown-preview stylesheet; the right one is Markview's
default light stylesheet. The figure is written to docs/screenshots/.

Requirements: pandoc, chromium, Pillow and numpy, plus a release Markview build
that can reach a GPU. The result depends on the host's fonts and browser build,
so the figure published in the README is the one this host produces.

Usage: scripts/render_typography_comparison.py [--column 340] [--scale 2]
"""
import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs/screenshots/source/en/justification.md"
OUTPUT = ROOT / "docs/screenshots/en-comparison.png"

# The render viewport is physical pixels and the reading column is inset by
# 16 logical pixels on each side (`src/app/launch.rs`).
INSET_X = 16
INSET_Y = 24
FOOT = 48
FONT_SIZE = 18
LABEL_FONT = "/usr/share/fonts/TTF/DejaVuSans.ttf"
INK = (38, 43, 48)
RULE = (226, 226, 222)

# A conventional Markdown preview: the same font, size, measure and paragraph
# spacing as the light stylesheet, a ragged right edge as every browser-based
# preview ships it, and `hyphens: auto` so the engine is asked for hyphenation
# rather than silently denied it.
PAGE = """<!doctype html><meta charset="utf-8"><style>
html, body {{ margin: 0; padding: 0; background: #FAFAF8 }}
body {{
  width: {width}px;
  padding: {top}px {left}px;
  box-sizing: border-box;
  font: {size}px/1.65 Georgia, "Noto Serif", serif;
  color: #262B30;
  text-align: left;
  hyphens: auto;
  -webkit-hyphens: auto;
  hyphenate-limit-chars: 6 2 2;
}}
p {{ margin: 0 0 0.8em }}
</style><body>
{body}
"""


def run(*args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True,
                          **kwargs)


def markview_panel(binary, work, width, column, scale):
    """Render the reading view and return the image plus its content height."""
    target = work / "markview.png"
    result = subprocess.run(
        [binary, "--render", str(SOURCE), "--output", str(target),
         "--width", str(width * scale), "--height", "6000",
         "--column", str(column), "--font-size", str(FONT_SIZE),
         "--scale", str(scale), "--light"],
        check=True, capture_output=True, text=True)
    for token in result.stdout.split() + result.stderr.split():
        if token.endswith("px"):
            return Image.open(target).convert("RGB"), int(float(token[:-2]))
    sys.exit("markview --render did not report the document height")


def webview_panel(work, width, height, scale):
    """Render the same Markdown the way a browser-based preview would."""
    html = work / "document.html"
    run("pandoc", str(SOURCE), "-f", "gfm", "-t", "html5", "-o", str(html))
    page = work / "page.html"
    page.write_text(PAGE.format(width=width, top=INSET_Y, left=INSET_X,
                                size=FONT_SIZE, body=html.read_text()))
    target = work / "webview.png"
    run("chromium", "--headless", "--no-sandbox", "--disable-gpu",
        f"--user-data-dir={work}/profile",
        f"--force-device-scale-factor={scale}",
        f"--window-size={width},{height}", "--hide-scrollbars",
        "--default-background-color=00000000",
        f"--screenshot={target}", f"file://{page}")
    return Image.open(target).convert("RGB")


def ink_height(image, scale):
    """Height of the last inked row, in logical pixels."""
    dark = np.array(image.convert("L")) < 128
    rows = np.flatnonzero(dark.any(axis=1))
    return 0 if rows.size == 0 else int(rows[-1] // scale) + 1


def text_lines(image):
    """Number of bands of inked rows, which is the number of set lines."""
    dark = np.array(image.convert("L")) < 128
    rows = dark.any(axis=1)
    edges = np.diff(np.concatenate(([False], rows, [False])).astype(np.int8))
    return int((edges == 1).sum())


def label(draw, font, x, y, text):
    draw.text((x, y), text, font=font, fill=INK)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--column", type=int, default=340,
                        help="reading column in logical pixels")
    parser.add_argument("--scale", type=int, default=2, help="device pixels")
    parser.add_argument("--binary", default=str(ROOT / "target/release/markview"))
    parser.add_argument("--out", default=str(OUTPUT))
    args = parser.parse_args()
    if not shutil.which("chromium") or not shutil.which("pandoc"):
        sys.exit("this script needs chromium and pandoc on PATH")

    width = args.column + 2 * INSET_X
    with tempfile.TemporaryDirectory() as directory:
        work = pathlib.Path(directory)
        markview, content = markview_panel(args.binary, work, width,
                                           args.column, args.scale)
        # The viewport is generous: a browser that cannot hyphenate needs more
        # lines for the same text, and the crop below picks the taller panel.
        webview = webview_panel(work, width, content * 2, args.scale)
        height = max(content, ink_height(webview, args.scale)) + FOOT
        pixels = height * args.scale
        markview = markview.crop((0, 0, width * args.scale, pixels))
        webview = webview.crop((0, 0, width * args.scale, pixels))
        lines = text_lines(webview), text_lines(markview)

        panel = width * args.scale
        gap = 12 * args.scale
        band = 40 * args.scale
        figure = Image.new("RGB", (panel * 2 + gap, band + pixels),
                           (255, 255, 255))
        figure.paste(webview, (0, band))
        figure.paste(markview, (panel + gap, band))
        draw = ImageDraw.Draw(figure)
        font = ImageFont.truetype(LABEL_FONT, 15 * args.scale)
        label(draw, font, INSET_X, 12 * args.scale, "Typical WebView")
        label(draw, font, panel + gap + INSET_X, 12 * args.scale, "Markview")
        draw.rectangle([0, band, panel - 1, band + pixels - 1], outline=RULE)
        draw.rectangle([panel + gap, band, panel * 2 + gap - 1,
                        band + pixels - 1], outline=RULE)
        path = pathlib.Path(args.out)
        path.parent.mkdir(parents=True, exist_ok=True)
        figure.save(path, optimize=True)
        print(f"{path}: {figure.width}x{figure.height}, "
              f"{args.column}px column, "
              f"{lines[0]} lines in the webview and {lines[1]} in Markview")


if __name__ == "__main__":
    main()
