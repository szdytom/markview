#!/usr/bin/env python3
"""Render the four source-backed document previews for each README."""

import argparse
import subprocess
from pathlib import Path

from PIL import Image, ImageDraw
from readme_fonts import font

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / "docs/screenshots/source/showcase"
OUTPUT = ROOT / "docs/screenshots/readme"
KINDS = ("prose", "technical", "math", "web")


def render(binary, output):
    output.mkdir(parents=True, exist_ok=True)
    for language in ("en", "zh"):
        for kind in KINDS:
            target = output / f"{language}-showcase-{kind}.png"
            subprocess.run(
                [
                    str(binary),
                    "render",
                    str(SOURCES / language / f"{kind}.md"),
                    "--output",
                    str(target),
                    "--width",
                    "1440",
                    "--height",
                    "1600",
                    "--column",
                    "650",
                    "--font-size",
                    "14" if kind == "technical" else "18",
                    "--scale",
                    "2",
                    "--light",
                    "--offline",
                ],
                check=True,
            )
            compose_credit(target, language, kind)


def compose_credit(target, language, kind):
    with Image.open(target) as rendered:
        page = Image.new("RGB", (1440, 1664), "#F9FAFC")
        page.paste(rendered.crop((0, 0, 1440, 1600)), (0, 0))
        draw = ImageDraw.Draw(page)
        if kind == "math":
            credit = "Download for free at https://openstax.org/details/books/calculus-volume-3."
        elif language == "zh":
            credit = {
                "prose": "Aaron Swartz ·《游击队开放访问宣言》· 2008 年 7 月",
                "technical": "《Rust 程序设计语言》· Rust 中文社区译本",
                "web": "维基百科 ·《韦伯的首次深空》· 原文转写节选",
            }[kind]
        else:
            credit = {
                "prose": "Aaron Swartz · Guerilla Open Access Manifesto · July 2008",
                "technical": "The Rust Programming Language · Data Types",
                "web": "Wikipedia · Webb’s First Deep Field · Extracted article",
            }[kind]
        draw.text((70, 1616), credit, font=font(21, language), fill="#687587")
        page.save(target, optimize=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/markview")
    parser.add_argument("--output-directory", type=Path, default=OUTPUT)
    args = parser.parse_args()
    render(args.binary, args.output_directory)
