"""Check that the README composites reproduce from their real captures."""

import json
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch

import numpy as np
from generate_readme_comparison import SOURCE, draw, measurements
from generate_readme_images import capture_profile
from generate_readme_showcase import KINDS, SOURCES, compose_credit, extract_web
from PIL import Image, ImageChops
from readme_fonts import font, save_chart
from render_typography_comparison import (
    FOOTER,
    GAP,
    HEADER,
    INSET_X,
    MARGIN,
    prepare_text,
)

ROOT = Path(__file__).resolve().parents[1]
IMAGES = ROOT / "docs/screenshots/readme"


class ReadmeImagesTest(unittest.TestCase):
    def test_chinese_captions_use_the_sc_face(self):
        self.assertEqual(font(36, "zh").getname()[0], "Noto Sans CJK SC")
        self.assertEqual(font(21, "zh").getname()[0], "Noto Sans CJK SC")
        self.assertEqual(font(36, "en").getname()[0], "DejaVu Sans")

    def test_chinese_chart_uses_sc_for_regular_and_bold_text(self):
        import matplotlib.pyplot as plt
        from matplotlib.ft2font import FT2Font
        from matplotlib.text import Text

        figure, axes = plt.subplots()
        axes.set_title("中文标题", fontsize=18, fontweight="bold")
        axes.set_xlabel("中文说明", fontsize=12)
        savefig = figure.savefig

        def check_and_save(*args, **kwargs):
            for text in figure.findobj(Text):
                self.assertEqual(
                    FT2Font(text.get_fontproperties().get_file()).family_name,
                    "Noto Sans CJK SC",
                )
            self.assertEqual(
                FT2Font(axes.title.get_fontproperties().get_file()).style_name,
                "Bold",
            )
            self.assertEqual(axes.title.get_fontsize(), 18)
            return savefig(*args, **kwargs)

        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "chart.png"
            with patch.object(figure, "savefig", side_effect=check_and_save):
                save_chart(figure, target, "zh")
            with Image.open(target) as chart:
                self.assertGreater(len(chart.getcolors(100000)), 100)
        plt.close(figure)

    def test_web_previews_replay_the_production_extractor(self):
        expected = {
            language: (SOURCES / language / "web-extracted.md").read_text()
            for language in ("en", "zh")
        }
        extract_web()
        for language in ("en", "zh"):
            self.assertEqual(
                (SOURCES / language / "web-extracted.md").read_text(),
                expected[language],
            )
            display = (
                (SOURCES / language / "web.md").read_text().split("\n\n", 1)[1].strip()
            )
            self.assertIn(display, expected[language])

    def test_showcase_sources_and_generated_frames(self):
        for language in ("en", "zh"):
            for kind in KINDS:
                source = (SOURCES / language / f"{kind}.md").read_text()
                self.assertTrue(source.startswith("#"))
                with Image.open(IMAGES / f"{language}-showcase-{kind}.png") as picture:
                    self.assertEqual(picture.size, (1440, 1664))
            technical = (SOURCES / language / "technical.md").read_text()
            self.assertIn("```rust", technical)
            self.assertIn("| `i128`", technical)
            math = (SOURCES / language / "math.md").read_text()
            self.assertGreaterEqual(math.count("$$"), 6)

    def test_showcase_credits_reproduce_without_changing_reader_pixels(self):
        with tempfile.TemporaryDirectory() as directory:
            for language in ("en", "zh"):
                for kind in KINDS:
                    name = f"{language}-showcase-{kind}.png"
                    target = Path(directory) / name
                    shutil.copyfile(IMAGES / name, target)
                    compose_credit(target, language, kind)
                    with (
                        self.subTest(image=name),
                        Image.open(IMAGES / name) as expected,
                        Image.open(target) as actual,
                    ):
                        self.assertIsNone(
                            ImageChops.difference(actual, expected).getbbox()
                        )

    def test_plain_text_is_literal_in_both_engines(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            source = work / "example.txt"
            source.write_text(
                "# A *literal* heading <tag> & text\ncontinues here.\n\nAnother paragraph."
            )
            prepare_text(source, work)
            markdown = (work / "document.md").read_text()
            html = (work / "document.html").read_text()
            self.assertIn(r"\# A \*literal\* heading \<tag\>", markdown)
            self.assertIn("&lt;tag&gt; &amp; text continues here.", html)
            self.assertEqual(html.count("<p>"), 2)

    def test_typography_comparison_shows_an_even_right_edge(self):
        for language in ("en", "zh"):
            with Image.open(
                ROOT / f"docs/screenshots/{language}-comparison.png"
            ) as figure:
                scale = 2
                margin, gap = MARGIN * scale, GAP * scale
                panel = (figure.width - gap - 2 * margin) // 2
                spreads = []
                for left in (margin, margin + panel + gap):
                    ink = (
                        np.asarray(
                            figure.crop(
                                (
                                    left + INSET_X * scale,
                                    HEADER * scale,
                                    left + panel - INSET_X * scale,
                                    figure.height - FOOTER * scale,
                                )
                            ).convert("L")
                        )
                        < 128
                    )
                    edges = np.diff(np.r_[False, ink.any(axis=1), False].astype(int))
                    bands = zip(np.where(edges == 1)[0], np.where(edges == -1)[0])
                    right_edges = [
                        np.where(ink[start:end].any(axis=0))[0][-1]
                        for start, end in bands
                    ]
                    spreads.append(
                        np.percentile(right_edges, 90) - np.percentile(right_edges, 25)
                    )
                self.assertLess(spreads[1], 5)
                self.assertGreater(spreads[0], 30)

    def test_comparison_charts_use_recorded_measurements(self):
        opening, memory = measurements(SOURCE.read_text())
        self.assertEqual(len(opening), 4)
        self.assertEqual(len(memory), 4)
        self.assertEqual(opening[0], [0.101, 0.957])
        self.assertEqual(memory[-1], [53.8, 1147.6])
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            for language in ("en", "zh"):
                draw(language, output)
                with Image.open(
                    output / f"{language}-performance-comparison.png"
                ) as chart:
                    self.assertEqual(chart.size, (1800, 920))
                    self.assertGreater(len(chart.getcolors(100000)), 100)

    def test_capture_preserves_fonts_and_live_preferences(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live = root / "config/markview"
            live.mkdir(parents=True)
            settings = (
                "single-instance = true\nrestore-session = true\nfont_size = 18.0\n"
                'window-layout = "linux"\nlanguage = "ja"\n[export]\nformat = "pdf"\n'
            )
            (live / "settings.toml").write_text(settings)
            (live / "fonts").mkdir()
            (live / "fonts/custom.otf").write_bytes(b"font resource")
            for language, folder in (("en", False), ("zh", True)):
                capture = root / language
                capture.mkdir()
                with patch.dict("os.environ", {"XDG_CONFIG_HOME": str(live.parent)}):
                    capture_profile(capture, language, folder)
                copied = tomllib.loads((capture / "markview/settings.toml").read_text())
                self.assertEqual(copied["single-instance"], folder)
                self.assertEqual(copied["restore-session"], folder)
                self.assertEqual(copied["tab-style"], "connected")
                self.assertEqual(copied["window-layout"], "windows")
                self.assertEqual(
                    copied["language"], "en" if language == "en" else "zh-hans"
                )
                self.assertEqual(copied["markdown-flavor"], "obsidian")
                self.assertFalse(copied["footer-path"])
                self.assertEqual(copied["font_size"], 18.0)
                self.assertEqual(copied["export"], {"format": "pdf"})
                self.assertEqual(
                    (capture / "markview/fonts/custom.otf").read_bytes(), b"font resource"
                )
                if folder:
                    session = json.loads((capture / "markview/session.json").read_text())
                    self.assertTrue(session["folder_open"])
                    self.assertTrue(Path(session["folder"]).is_dir())
                    for tab in session["tabs"]:
                        self.assertTrue(Path(tab["path"]).is_file())
            self.assertEqual((live / "settings.toml").read_text(), settings)

    def test_recompose_from_desktop_and_android_captures(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            for capture in IMAGES.glob("*.png"):
                if not any(kind in capture.stem for kind in ("hero", "themes")):
                    shutil.copyfile(capture, output / capture.name)
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/generate_readme_images.py"),
                    "--output-directory",
                    str(output),
                ],
                check=True,
            )
            for language in ("en", "zh"):
                self.assertFalse((output / f"{language}-details.png").exists())
                for kind in ("hero", "themes"):
                    name = f"{language}-{kind}.png"
                    with (
                        self.subTest(image=name),
                        Image.open(IMAGES / name) as expected,
                        Image.open(output / name) as actual,
                    ):
                        self.assertEqual(actual.size, expected.size)
                        self.assertIsNone(
                            ImageChops.difference(actual, expected).getbbox()
                        )


if __name__ == "__main__":
    unittest.main()
