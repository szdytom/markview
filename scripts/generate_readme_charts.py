#!/usr/bin/env python3
"""Draw the performance figures used by the READMEs.

The numbers are the measured baselines recorded in `docs/performance.md`; this
script only draws them, so that page stays the source of truth. They come from
one ordinary laptop, an Intel Core Ultra 5 125H with integrated Intel Arc
through Vulkan, on the `performance` power profile.
"""

from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "screenshots"

PAPER = "#FAFAF8"
INK = "#262B30"
MUTED = "#69747E"
RULE = "#E6E9EB"
ACCENT = "#315D86"
ACCENT_SOFT = "#7FA0BE"

# First readable frame in a native DPR-2 window: median and observed range of
# thirty runs per fixture, process entry and initialization included.
LATENCY_MEDIAN = [95.1, 102.5, 105.7, 102.7]
LATENCY_LOW = [81.9, 82.3, 84.5, 84.3]
LATENCY_HIGH = [123.3, 117.7, 114.5, 119.8]
# Process RSS after scrolling through the document.
MEMORY = [49.7, 55.9, 57.6, 90.8]

TEXT = {
    "file": "en-performance.png",
    "font": "Noto Sans",
    "latency_title": "First readable frame: about 100 ms",
    "latency_note": "process entry to the first readable frame, initialization "
                    "included",
    "latency_unit": "milliseconds",
    "latency_limit": 160,
    "memory_title": "Resident memory: tens of megabytes",
    "memory_note": "process RSS after scrolling through the whole document",
    "memory_unit": "MiB",
    "memory_limit": 100,
    "groups": ["10 KiB\nprose", "100 KiB\nCJK", "100 KiB\nCJK + math", "1 MiB\nCJK"],
}

ZH = {
    "file": "zh-performance.png",
    "font": "Noto Sans CJK SC",
    "latency_title": "第一帧可读画面：约 100 毫秒",
    "latency_note": "从进程启动到第一帧可读画面，含初始化",
    "latency_unit": "毫秒",
    "latency_limit": 160,
    "memory_title": "常驻内存：几十兆字节",
    "memory_note": "滚动全文之后进程的常驻内存",
    "memory_unit": "MiB",
    "memory_limit": 100,
    "groups": ["10 KiB\n普通文档", "100 KiB\n中文", "100 KiB\n中文+公式", "1 MiB\n中文"],
}


def panel(ax, values, groups, unit, color, limit, spread=None):
    x = list(range(len(groups)))
    bars = ax.bar(x, values, width=0.52, color=color, zorder=3)
    tops = list(values)
    if spread is not None:
        low, high = spread
        ax.errorbar(
            x, values,
            yerr=[[v - l for v, l in zip(values, low)],
                  [h - v for v, h in zip(values, high)]],
            fmt="none", ecolor=MUTED, elinewidth=1.2, capsize=5,
            alpha=0.75, zorder=4,
        )
        tops = list(high)
    for bar, value, top in zip(bars, values, tops):
        ax.text(bar.get_x() + bar.get_width() / 2, top + limit * 0.035,
                f"{value:.0f}", ha="center", va="bottom", fontsize=12,
                color=INK, zorder=5)
    ax.set_ylim(0, limit)
    ax.set_ylabel(unit, fontsize=10.5, color=MUTED, labelpad=8)
    ax.set_yticks([0, limit / 2, limit])
    ax.set_yticklabels([f"{v:.0f}" for v in (0, limit / 2, limit)])
    ax.set_xticks(x)
    ax.set_xticklabels(groups, fontsize=10.5, color=MUTED)
    ax.tick_params(axis="both", length=0, labelsize=11, colors=MUTED)
    ax.grid(axis="y", color=RULE, linewidth=1, zorder=0)
    ax.set_axisbelow(True)
    for side in ("top", "right", "left", "bottom"):
        ax.spines[side].set_visible(False)


def figure(spec):
    plt.rcParams["font.family"] = [spec["font"], "DejaVu Sans"]
    plt.rcParams["axes.unicode_minus"] = False

    fig, axes = plt.subplots(1, 2, figsize=(16.55, 7.0), dpi=100)
    fig.patch.set_facecolor(PAPER)
    fig.subplots_adjust(left=0.055, right=0.985, top=0.78, bottom=0.13,
                        wspace=0.16)

    specs = [
        (spec["latency_title"], spec["latency_note"], LATENCY_MEDIAN,
         spec["latency_unit"], ACCENT, spec["latency_limit"],
         (LATENCY_LOW, LATENCY_HIGH)),
        (spec["memory_title"], spec["memory_note"], MEMORY,
         spec["memory_unit"], ACCENT_SOFT, spec["memory_limit"], None),
    ]
    for ax, (title, note, values, unit, color, limit, spread) in zip(axes, specs):
        ax.set_facecolor(PAPER)
        panel(ax, values, spec["groups"], unit, color, limit, spread)
        ax.set_title(title, loc="left", fontsize=14, color=INK, pad=34,
                     fontweight="bold")
        ax.text(0, 1.055, note, transform=ax.transAxes, fontsize=10.5,
                color=MUTED, va="bottom")

    path = OUT / spec["file"]
    fig.savefig(path, facecolor=PAPER)
    plt.close(fig)
    print(f"wrote {path}")


for spec in (TEXT, ZH):
    figure(spec)
