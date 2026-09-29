#!/usr/bin/env python3
"""Check that a served tile matches what the offscreen renderer draws.

The reader and a served session must produce the same pixels for the same
document: a tile is a crop of the document, and `render` is the same crop
inside a 24 px viewport inset. Nothing but the engine may decide typography, so
a difference here means a served document is not laid out the way the reader
lays it out.

Usage: scripts/serve_render_parity.py [PATH-TO-MARKVIEW]

Exit status is 0 when every case matches, 1 when a case differs, and 2 when the
probe could not run.
"""
from serve_protocol import read_response
import json
import pathlib
import queue
import struct
import subprocess
import sys
import tempfile
import threading
import time
import zlib

# `render` insets its viewport for window chrome; a tile does not.
RENDER_INSET = 24
TILE_WIDTH = 900
TILE_HEIGHT = 400


def png_chunk(kind, data):
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data))
    )


def solid_png(width, height):
    """A red PNG, so a probe needs no fixture on disk."""
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    body = zlib.compress((b"\0" + b"\xff\0\0" * width) * height)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", header)
        + png_chunk(b"IDAT", body)
        + png_chunk(b"IEND", b"")
    )


def pixels(data):
    """RGBA rows of a PNG, undoing the scanline filters."""
    index = 8
    packed = b""
    width = height = depth = kind = 0
    while index < len(data):
        length = struct.unpack(">I", data[index : index + 4])[0]
        name = data[index + 4 : index + 8]
        body = data[index + 8 : index + 8 + length]
        index += 12 + length
        if name == b"IHDR":
            width, height, depth, kind = struct.unpack(">IIBB", body[:10])
        if name == b"IDAT":
            packed += body
    if depth != 8 or kind != 6:
        raise SystemExit(f"unexpected PNG: depth {depth}, color type {kind}")
    raw = zlib.decompress(packed)
    stride = width * 4
    previous = bytearray(stride)
    rows = []
    for y in range(height):
        offset = y * (stride + 1)
        filt = raw[offset]
        row = bytearray(raw[offset + 1 : offset + 1 + stride])
        for x in range(stride):
            a = row[x - 4] if x >= 4 else 0
            b = previous[x]
            c = previous[x - 4] if x >= 4 else 0
            p = a + b - c
            candidates = [abs(p - a), abs(p - b), abs(p - c)]
            predictor = [a, b, c][candidates.index(min(candidates))]
            row[x] = (row[x] + [0, a, b, (a + b) // 2, predictor][filt]) % 256
        rows.append(bytes(row))
        previous = row
    return rows


def differing_bytes(left, right):
    return sum(
        x != y for a, b in zip(left, right) for x, y in zip(a, b)
    )


CASES = {
    "text": "Hello.\n\n```rust\nfn main() {}\n```\n",
    "svg": '<img src="shape.svg" width="200" height="200">\n',
}


def main():
    binary = sys.argv[1] if len(sys.argv) > 1 else "./target/release/markview"
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        (root / "dot.png").write_bytes(solid_png(1, 1))
        (root / "shape.svg").write_text(
            '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">'
            '<circle cx="5" cy="5" r="4" fill="red"/></svg>'
        )
        document = root / "doc.md"

        server = subprocess.Popen(
            [binary, "serve", "--fonts", str(pathlib.Path(__file__).resolve().parents[1] / "crates/markview-core/tests/fonts"), "--ignore-system-fonts", "--offline", "--state-dir", str(root / "state")],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        answers = queue.Queue()
        threading.Thread(
            target=lambda: [
                answers.put(answer) for answer in iter(lambda: read_response(server.stdout), None)
            ],
            daemon=True,
        ).start()

        def send(message):
            server.stdin.write((json.dumps(message) + "\n").encode())
            server.stdin.flush()
            return answers.get(timeout=30)

        # Progressive publication: an answer that does not wait for the images
        # is followed by a layout notification nobody asked for.
        published = send(
            {
                "open": {
                    "id": "progress",
                    "path": str(root / "doc.md"),
                    "text": "Before.\n\n![dot](dot.png)\n\nAfter.\n",
                    "settle": False,
                }
            }
        )["opened"]
        pushed = answers.get(timeout=30)["layout"]
        if published["complete"] or not pushed["complete"]:
            print(
                f"FAIL progressive publication: {published['complete']} -> "
                f"{pushed['complete']}",
                file=sys.stderr,
            )
            return 2
        print(
            f"push: incomplete {published['height']:.0f}px -> complete "
            f"{pushed['height']:.0f}px"
        )

        # A tile asked for while the images are still settling consumes their
        # completion, so the geometry it settles has to reach the client even
        # though a tile answers with an image rather than a block map.
        (root / "big.png").write_bytes(solid_png(4000, 4000))
        send(
            {
                "open": {
                    "id": "during",
                    "path": str(root / "doc.md"),
                    "text": "Before.\n\n![big](big.png)\n\nAfter.\n",
                    "settle": False,
                }
            }
        )
        tile = send(
            {
                "tile": {
                    "id": "during",
                    "width": TILE_WIDTH,
                    "height": TILE_HEIGHT,
                }
            }
        )
        if "error" in tile:
            print(f"FAIL tile during settling: {tile['error']}", file=sys.stderr)
            return 2
        # Whatever the tile settled has to be published rather than absorbed.
        # The notification follows the tile response, so it is waited for.
        settled_height = None
        deadline = time.time() + 5
        while settled_height is None and time.time() < deadline:
            try:
                message = answers.get(timeout=0.2)
            except queue.Empty:
                continue
            if "layout" in message:
                settled_height = message["layout"]["height"]
        if settled_height is None:
            print(
                "FAIL tile during settling: geometry was never published",
                file=sys.stderr,
            )
            return 2
        print(
            f"tile during settling: geometry published at {settled_height:.0f}px"
        )

        # An export over the protocol is the command line's own path run on
        # the client's bytes, so the same input must produce the same file.
        # A document with no heading takes its title from the file name, which
        # is where a temporary source would leak into the reader's metadata.
        export_text = "# Title\n\nA paragraph with a [link](https://example.com).\n"
        headingless = "A paragraph without a heading.\n"
        document.write_text(export_text)
        send(
            {
                "open": {
                    "id": "export",
                    "path": str(document),
                    "text": export_text,
                }
            }
        )
        over_protocol = root / "protocol.pdf"
        answer = send(
            {"export": {"id": "export", "output": str(over_protocol)}}
        )
        if "error" in answer:
            print(f"FAIL export: {answer['error']}", file=sys.stderr)
            return 2
        over_command_line = root / "command-line.pdf"
        subprocess.run(
            [
                binary,
                "pdf",
                str(document),
                "--fonts",
                str(pathlib.Path(__file__).resolve().parents[1] / "crates/markview-core/tests/fonts"),
                "--ignore-system-fonts",
                "--output",
                str(over_command_line),
                "--offline",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        identical = (
            over_protocol.read_bytes() == over_command_line.read_bytes()
        )
        print(
            f"export: {over_protocol.stat().st_size} bytes, identical to the "
            f"command line: {identical}"
        )
        if not identical:
            print(
                "FAIL export: the protocol export differs from the command "
                "line",
                file=sys.stderr,
            )
            return 2

        # The same comparison for a document whose title can only come from
        # its own name.
        document.write_text(headingless)
        send(
            {
                "open": {
                    "id": "headingless",
                    "path": str(document),
                    "text": headingless,
                }
            }
        )
        headingless_protocol = root / "headingless-protocol.pdf"
        answer = send(
            {
                "export": {
                    "id": "headingless",
                    "output": str(headingless_protocol),
                }
            }
        )
        if "error" in answer:
            print(f"FAIL headingless export: {answer['error']}", file=sys.stderr)
            return 2
        headingless_cli = root / "headingless-cli.pdf"
        subprocess.run(
            [binary, "pdf", str(document), "--fonts",
                str(pathlib.Path(__file__).resolve().parents[1] / "crates/markview-core/tests/fonts"),
                "--ignore-system-fonts",
                "--output", str(headingless_cli),
             "--offline"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        headingless_same = (
            headingless_protocol.read_bytes() == headingless_cli.read_bytes()
        )
        print(
            f"headingless export: {headingless_protocol.stat().st_size} bytes, "
            f"identical to the command line: {headingless_same}"
        )
        if not headingless_same:
            print(
                "FAIL headingless export: differs from the command line",
                file=sys.stderr,
            )
            return 2

        failed = False
        for label, text in CASES.items():
            document.write_text(text)
            send({"open": {"id": label, "path": str(document), "text": text}})
            tile = send(
                {
                    "tile": {
                        "id": label,
                        "width": TILE_WIDTH,
                        "height": TILE_HEIGHT,
                    }
                }
            )["tile"]
            rendered = root / "out.png"
            subprocess.run(
                [
                    binary,
                    "render",
                    str(document),
                    "--fonts",
                    str(pathlib.Path(__file__).resolve().parents[1] / "crates/markview-core/tests/fonts"),
                    "--ignore-system-fonts",
                    "--offline",
                    "--width",
                    str(TILE_WIDTH),
                    "--height",
                    str(TILE_HEIGHT + 2 * RENDER_INSET),
                    "--output",
                    str(rendered),
                ],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            served_rows = pixels(tile["png"])
            render_rows = pixels(rendered.read_bytes())
            expected = render_rows[RENDER_INSET : RENDER_INSET + TILE_HEIGHT]
            differed = differing_bytes(served_rows, expected)
            status = "ok" if differed == 0 else "DIFFERS"
            print(f"{label}: {differed} differing bytes ({status})")
            failed |= differed != 0

        server.stdin.close()
        server.wait(timeout=15)
        return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
