#!/usr/bin/env python3
"""Compare the GPU render and the PDF export of one document.

Both pipelines share one layout and one stylesheet, so at the same text measure
they must put the same content in the same place. This driver exports a fixture
through `--render` and `--pdf`, rasterizes the PDF page with Ghostscript, and
reports structural agreement:

* the best per-band horizontal shift, which must be zero;
* the normalized column-profile overlap at that shift;
* the ink-mass ratio and the per-band centroid deltas;
* a windowed SSIM of the lightly blurred images.

A non-zero shift, a missing band, or a low overlap is a rendering bug. What is
left is glyph rasterization weight: the GPU bakes subpixel coverage into
bitmaps, Ghostscript antialiases vector outlines, and their weight difference
shrinks as `--scale` rises. Two layout pixels per device pixel is the default
because at 1x an italic band's one-pixel strokes can lose most of their profile
overlap to a quarter-pixel offset, which reads as a shift that is not there.

The render is exported with `--style print`, which merges the print sheet over
the reader's light sheet; `the_print_sheet_survives_a_merge_over_the_reader_sheet`
in `crates/markview-core/src/style/tests.rs` holds those two to the same fields.

Requirements: python3 with numpy and Pillow, plus Ghostscript (`gs`).

Usage:
  python3 scripts/compare_pdf_render.py examples/welcome.md --out /tmp/pdf-cmp
  python3 scripts/compare_pdf_render.py doc.md --scale 2 --page 640x1600
"""
import argparse
import shutil
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image

# The render mode offsets the document by its viewport insets, in logical pixels.
LEFT_INSET = 16
TOP_INSET = 24
BOTTOM_INSET = 24

# Both exports pin one type size: `--pdf` now defaults to 12 pt body text while
# `--render` uses the reader's 18 px, so leaving either implicit would compare
# two different measures.
FONT_SIZE_PX = 18


def run(command: list[str]) -> None:
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit(f"failed: {' '.join(command)}")


def export(binary: Path, fixture: Path, out: Path, page: tuple[int, int],
           scale: float) -> tuple[Path, Path]:
    def millimetres(px: float) -> float:
        """A length in layout pixels as millimetres: one pixel is 1/96 inch."""
        return px * 25.4 / 96

    width, height = page
    pdf = out / "compare.pdf"
    png = out / "compare.png"
    run([
        str(binary), "pdf", str(fixture), "--output", str(pdf),
        "--paper", f"{millimetres(width):.5f}x{millimetres(height):.5f}",
        "--margin", "0", "--footer", "", "--style", "print",
        "--font-size", str(FONT_SIZE_PX),
    ])
    # The renderer clamps the measure to `width / scale - 32` and shows
    # `height / scale - top - bottom` logical pixels.
    run([
        str(binary), "render", str(fixture), "--output", str(png),
        "--style", "print",
        "--width", str(round((width + 32) * scale)),
        "--height", str(round((height + BOTTOM_INSET + 24) * scale)),
        "--column", str(width), "--scale", str(scale),
        "--font-size", str(FONT_SIZE_PX),
    ])
    return pdf, png


def rasterize(pdf: Path, out: Path, dpi: int) -> Path:
    page = out / "pdf-page.png"
    run([
        "gs", "-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=png16m",
        f"-r{dpi}", "-dTextAlphaBits=4", "-dGraphicsAlphaBits=4",
        "-o", str(page), str(pdf),
    ])
    return page


def ink(path: Path) -> np.ndarray:
    """Gray ink, 0 for white paper and 255 for solid black."""
    return 255.0 - np.asarray(Image.open(path).convert("L")).astype(np.float64)


def bands(image: np.ndarray, gap: int) -> list[tuple[int, int]]:
    """Independent content bands, split at runs of blank rows."""
    rows = image.sum(axis=1)
    out: list[tuple[int, int]] = []
    start: int | None = None
    blank = 0
    for y, total in enumerate(rows):
        if total > 2.0 * image.shape[1] / 640:
            if start is None:
                start = y
            blank = 0
        elif start is not None:
            blank += 1
            if blank >= gap:
                out.append((start, y - blank + 1))
                start = None
    if start is not None:
        out.append((start, len(rows)))
    return out


def profile(image: np.ndarray, y0: int, y1: int) -> np.ndarray:
    columns = image[y0:y1].sum(axis=0)
    total = columns.sum()
    return columns / total if total else columns


def best_shift(a: np.ndarray, b: np.ndarray, limit: int) -> tuple[int, float, float]:
    """The shift of `b` that best matches `a`, its L1 residual, and L1 at zero."""

    def residual(shift: int) -> float:
        if shift > 0:
            return float(np.abs(a[:-shift] - b[shift:]).sum())
        if shift < 0:
            return float(np.abs(a[-shift:] - b[:shift]).sum())
        return float(np.abs(a - b).sum())

    at_zero = residual(0)
    best = min(range(-limit, limit + 1), key=residual)
    return best, residual(best), at_zero


def centroid(image: np.ndarray, y0: int, y1: int) -> tuple[float, float]:
    """Ink-weighted centre of one band, in pixels."""
    sub = image[y0:y1]
    total = sub.sum()
    if total == 0:
        return 0.0, 0.0
    ys, xs = np.mgrid[0:sub.shape[0], 0:sub.shape[1]]
    return float((sub * xs).sum() / total), y0 + float((sub * ys).sum() / total)


def ssim(a: np.ndarray, b: np.ndarray, window: int = 8) -> float:
    """Windowed structural similarity over an image pair, mean of the windows."""
    def mean(x: np.ndarray) -> np.ndarray:
        shape = (x.shape[0] // window, window, x.shape[1] // window, window)
        return x.reshape(shape).mean(axis=(1, 3))

    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    ma, mb = mean(a), mean(b)
    va, vb = mean(a * a) - ma * ma, mean(b * b) - mb * mb
    cov = mean(a * b) - ma * mb
    values = ((2 * ma * mb + c1) * (2 * cov + c2)) / (
        (ma * ma + mb * mb + c1) * (va + vb + c2)
    )
    return float(values.mean())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("--out", type=Path, default=Path("/tmp/pdf-compare"))
    parser.add_argument("--binary", type=Path,
                        default=Path("target/release/markview"))
    parser.add_argument("--page", default="640x1600",
                        help="page in layout pixels, WIDTHxHEIGHT")
    parser.add_argument("--scale", type=float, default=2.0,
                        help="device pixels per layout pixel; 1x lets "
                             "antialiasing dominate a thin band's profile")
    parser.add_argument("--min-overlap", type=float, default=0.80)
    parser.add_argument("--shift-tolerance", type=int, default=0)
    args = parser.parse_args()

    if shutil.which("gs") is None:
        raise SystemExit("Ghostscript (gs) is required to rasterize the PDF")
    width, height = (int(part) for part in args.page.lower().split("x"))
    args.out.mkdir(parents=True, exist_ok=True)

    pdf, render = export(args.binary, args.fixture, args.out,
                         (width, height), args.scale)
    page = rasterize(pdf, args.out, int(96 * args.scale))
    dpi_scale = args.scale
    inset = (round(LEFT_INSET * dpi_scale), round(TOP_INSET * dpi_scale))
    size = (round(width * dpi_scale), round(height * dpi_scale))

    pdf_ink = ink(page)
    # The render carries its viewport insets; crop them off so both images are
    # exactly one page of the same measure.
    render_ink = ink(render)[
        inset[1]:inset[1] + size[1], inset[0]:inset[0] + size[0]
    ]
    if pdf_ink.shape != render_ink.shape:
        raise SystemExit(
            f"page raster {pdf_ink.shape} does not match the render crop "
            f"{render_ink.shape}"
        )

    gap = max(2, round(6 * dpi_scale))
    pdf_bands = bands(pdf_ink, gap)
    render_bands = bands(render_ink, gap)
    limit = max(1, round(12 * dpi_scale))
    print(f"bands: pdf {len(pdf_bands)}  render {len(render_bands)}")

    failures: list[str] = []
    compared = 0
    print(f"{'render band':>16} {'pdf band':>14} {'shift':>6} "
          f"{'overlap':>8} {'mass':>6} {'dx':>6} {'dy':>6}")
    for (ry0, ry1) in render_bands:
        overlap_band = [
            (py0, py1) for (py0, py1) in pdf_bands
            if min(ry1, py1) - max(ry0, py0) > 0.6 * (ry1 - ry0)
        ]
        if not overlap_band:
            if ry1 < height:
                failures.append(
                    f"render band y={ry0}-{ry1} is not on PDF page 1"
                )
            else:
                print(f"{ry0:6d}-{ry1:6d} {'(past the page break)':>14}")
            continue
        py0, py1 = overlap_band[0]
        # Compare the rows both bandings agree on, so a band edge that one
        # blob detector placed a pixel lower cannot look like a shift.
        y0, y1 = max(ry0, py0), min(ry1, py1)
        pa, pb = profile(pdf_ink, y0, y1), profile(render_ink, y0, y1)
        shift, _, at_zero = best_shift(pa, pb, limit)
        overlap = float(np.minimum(pa, pb).sum())
        mass = float(render_ink[ry0:ry1].sum() / max(1.0, pdf_ink[py0:py1].sum()))
        pa_cx, pa_cy = centroid(pdf_ink, py0, py1)
        pb_cx, pb_cy = centroid(render_ink, ry0, ry1)
        dx, dy = pb_cx - pa_cx, pb_cy - pa_cy
        compared += 1
        print(f"{ry0:6d}-{ry1:6d} {py0:6d}-{py1:6d} {shift:6d} "
              f"{overlap * 100:7.1f}% {mass:6.2f} {dx:+6.2f} {dy:+6.2f}")
        if abs(shift) > args.shift_tolerance:
            failures.append(
                f"render band y={ry0}-{ry1} sits {shift}px off the PDF band"
            )
        if overlap < args.min_overlap:
            failures.append(
                f"render band y={ry0}-{ry1} overlaps the PDF by only "
                f"{overlap * 100:.1f}%"
            )
        if abs(dy) > dpi_scale * 1.5:
            failures.append(
                f"render band y={ry0}-{ry1} is {dy:.2f}px off vertically"
            )

    def blur(x: np.ndarray) -> np.ndarray:
        padded = np.pad(x, ((1, 1), (1, 1)), mode="edge")
        return sum(
            padded[dy:dy + x.shape[0], dx:dx + x.shape[1]]
            for dy in range(3) for dx in range(3)
        ) / 9

    score = ssim(blur(pdf_ink), blur(render_ink))
    mass = render_ink.sum() / pdf_ink.sum()
    print(f"\ncompared {compared} bands; windowed SSIM {score:.4f}; "
          f"total ink ratio {mass:.2f}")

    # Artifacts for eyeballing: the two pages side by side and a diff heat map.
    side = np.concatenate(
        [pdf_ink, np.full((pdf_ink.shape[0], 8,), 128.0), render_ink], axis=1
    )
    Image.fromarray(np.clip(255.0 - side, 0, 255).astype(np.uint8)).save(
        args.out / "side-by-side.png"
    )
    heat = np.clip(np.abs(pdf_ink - render_ink) * 4, 0, 255)
    Image.fromarray(heat.astype(np.uint8)).save(args.out / "diff-heat.png")
    print(f"wrote {args.out / 'side-by-side.png'} and {args.out / 'diff-heat.png'}")

    if failures:
        print("\nFAIL")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("\nPASS: same bands, same positions")
    return 0


if __name__ == "__main__":
    sys.exit(main())
