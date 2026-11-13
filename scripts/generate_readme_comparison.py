#!/usr/bin/env python3
"""Draw reader comparisons from the recorded Markdown measurement tables."""

from pathlib import Path

import matplotlib
from readme_fonts import save_chart

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Patch

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs/developers/history/comparison.md"
OUTPUT = ROOT / "docs/screenshots/readme"
PAPER, INK, MUTED = "#fafaf8", "#262b30", "#69747e"
COLORS = ("#315d86", "#b8bfc5")


def measurements(source):
    """Read first-frame seconds and total reader RSS in table order."""
    opening, memory = [], []
    for line in source.splitlines():
        if not line.startswith(("| 10 KiB", "| 100 KiB")):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) != 3:
            continue
        if " / " in cells[1]:
            opening.append([float(cell.split(" / ")[0]) for cell in cells[1:]])
        elif cells[1].endswith(" MiB"):
            memory.append([float(cell.removesuffix(" MiB")) for cell in cells[1:]])
    return opening, memory


def draw(language, output=OUTPUT):
    opening, memory = measurements(SOURCE.read_text())
    chinese = language == "zh"
    plt.rcParams["font.family"] = [
        "Noto Sans CJK SC" if chinese else "Noto Sans",
        "DejaVu Sans",
    ]
    labels = (
        ["10 KiB 正文", "100 KiB 正文", "10 KiB · 108 个公式", "100 KiB · 1,092 个公式"]
        if chinese
        else [
            "10 KiB prose",
            "100 KiB prose",
            "10 KiB · 108 formulas",
            "100 KiB · 1,092 formulas",
        ]
    )
    titles = (
        ["冷启动至首个可读画面", "阅读器总常驻内存"]
        if chinese
        else ["Cold start to first readable frame", "Total reader resident memory"]
    )
    fig, axes = plt.subplots(1, 2, figsize=(18, 9.2), dpi=100)
    fig.patch.set_facecolor(PAPER)
    fig.subplots_adjust(left=0.035, right=0.97, top=0.77, bottom=0.19, wspace=0.21)
    for ax, title, rows, unit, limit in zip(
        axes, titles, (opening, memory), ("s", "MiB"), (3.5, 1400)
    ):
        ax.set_facecolor(PAPER)
        for group, (label, values) in enumerate(zip(labels, rows)):
            base = group * 3
            ax.text(0, base - 0.8, label, fontsize=13, color=INK, va="center")
            for reader, value in enumerate(values):
                y = base + reader * 0.65
                ax.barh(y, value, height=0.48, color=COLORS[reader], zorder=3)
                number = f"{value:.3f}" if unit == "s" else f"{value:.1f}"
                ax.text(
                    value + limit * 0.018,
                    y,
                    number,
                    va="center",
                    color=INK,
                    fontsize=12,
                )
        ax.set_xlim(0, limit)
        ax.set_ylim(10.1, -1.3)
        ax.set_yticks([])
        ax.set_xticks([0, 1, 2, 3] if unit == "s" else [0, 400, 800, 1200])
        ax.tick_params(axis="x", length=0, colors=MUTED, labelsize=11)
        ax.grid(axis="x", color="#e6e9eb", zorder=0)
        for spine in ax.spines.values():
            spine.set_visible(False)
        ax.set_xlabel(unit, loc="right", color=MUTED, fontsize=12)
        ax.set_title(title, loc="left", fontsize=18, color=INK, pad=20)
    fig.legend(
        handles=[
            Patch(color=color, label=label)
            for color, label in zip(COLORS, ("Markview", "MarkText 0.19.1"))
        ],
        loc="upper left",
        bbox_to_anchor=(0.029, 0.98),
        ncol=2,
        frameon=False,
        fontsize=14,
    )
    note = (
        "同一台 Linux 笔记本 · 三次测量的中位数 · 越低越好\n"
        "2026-09-22 / 23 · 完整方法与限制：docs/developers/history/comparison.md"
        if chinese
        else "Same Linux laptop · Median of three runs · Lower is better\n"
        "2026-09-22 / 23 · Method and limitations: docs/developers/history/comparison.md"
    )
    fig.text(0.035, 0.055, note, color=MUTED, fontsize=12, linespacing=1.7)
    output.mkdir(parents=True, exist_ok=True)
    save_chart(
        fig,
        output / f"{language}-performance-comparison.png",
        language,
        facecolor=PAPER,
    )
    plt.close(fig)


if __name__ == "__main__":
    for language in ("en", "zh"):
        draw(language)
