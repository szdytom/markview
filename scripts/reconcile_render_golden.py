#!/usr/bin/env python3
"""Minimally adjust a reviewed golden to accept multiple actual frames.

Requires Pillow. The fixed tolerance matches `tests/support/render_goldens.rs`.
"""

import argparse
from pathlib import Path

from PIL import Image

TOLERANCE = 2


def reconcile(baseline, actuals):
    if len(actuals) < 2:
        raise ValueError("Supply at least two actual frames")
    with Image.open(baseline) as image:
        reference = image.convert("RGBA")
    samples = []
    for path in actuals:
        with Image.open(path) as image:
            if image.size != reference.size:
                raise ValueError(f"Dimensions differ: {path}")
            samples.append(image.convert("RGBA").tobytes())

    result = bytearray(reference.tobytes())
    changed_pixels = set()
    changed_channels = 0
    for index, values in enumerate(zip(*samples)):
        low = max(values) - TOLERANCE
        high = min(values) + TOLERANCE
        if low > high:
            pixel = index // 4
            x, y = pixel % reference.width, pixel // reference.width
            raise ValueError(
                f"No common tolerance interval at ({x}, {y}), "
                f"channel {'RGBA'[index % 4]}: {min(values)}..{max(values)}"
            )
        value = min(max(result[index], low), high)
        if value != result[index]:
            result[index] = value
            changed_pixels.add(index // 4)
            changed_channels += 1
    if changed_channels:
        Image.frombytes("RGBA", reference.size, bytes(result)).save(baseline)
    return len(changed_pixels), changed_channels


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path, help="Existing PNG to update in place")
    parser.add_argument("actuals", type=Path, nargs="+", help="Reviewed actual PNGs")
    args = parser.parse_args()
    try:
        pixels, channels = reconcile(args.baseline, args.actuals)
    except ValueError as error:
        parser.error(str(error))
    print(f"Adjusted {pixels} pixels, {channels} channels in {args.baseline}")


if __name__ == "__main__":
    main()
