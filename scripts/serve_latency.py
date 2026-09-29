#!/usr/bin/env python3
"""Measure what a served session costs a host that is drawing it.

Three questions, each over a real session against the built binary:

  edit to PNG      the engine-only round trip from buffer bytes to an encoded
                   tile; VS Code coalescing, decoding and paint are excluded
  bytes a screen   the protocol responses for one screenful, excluding the
                   extension/webview transport and background find text
  cost of opening  the engine's start and the first document against a second
                   document, which is what NFR-5 bounds

Usage: scripts/serve_latency.py [PATH-TO-MARKVIEW] [--iterations N]

Exit status is 0 unless the session could not be driven.
"""
from serve_protocol import read_response
import json
import queue
import statistics
import subprocess
import sys
import tempfile
import pathlib
import threading
import time

TILE_WIDTH = 1200
TILE_HEIGHT = 800
PARAGRAPHS = 200


def document():
    return "".join(f"Paragraph {index} of the served document.\n\n"
                   for index in range(PARAGRAPHS))


class Session:
    def __init__(self, binary):
        self.state = tempfile.TemporaryDirectory()
        self.process = subprocess.Popen(
            [binary, "serve", "--fonts", str(pathlib.Path(__file__).resolve().parents[1] / "crates/markview-core/tests/fonts"), "--ignore-system-fonts", "--offline", "--state-dir", self.state.name],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        self.answers = queue.Queue()
        threading.Thread(
            target=self._read, daemon=True
        ).start()

    def _read(self):
        while (answer := read_response(self.process.stdout)) is not None:
            self.answers.put(answer)

    def send(self, message):
        """Write one request and read until its answer, skipping notifications."""
        self.process.stdin.write((json.dumps(message) + "\n").encode())
        self.process.stdin.flush()
        # A response is named for its request's outcome, so it is recognised by
        # what it is not: only a `layout` is a notification.
        while True:
            answer = self.answers.get(timeout=120)
            if "layout" not in answer or "error" in answer:
                return answer

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=30)
        self.state.cleanup()


def percentile(samples, fraction):
    ordered = sorted(samples)
    index = min(len(ordered) - 1, int(round(fraction * (len(ordered) - 1))))
    return ordered[index]


def main():
    arguments = sys.argv[1:]
    binary = "./target/release/markview"
    iterations = 30
    if arguments and not arguments[0].startswith("--"):
        binary = arguments.pop(0)
    if arguments and arguments[0] == "--iterations":
        iterations = int(arguments[1])

    started = time.monotonic()
    session = Session(binary)
    opened = session.send(
        {"open": {"id": "d1", "text": document(), "settle": False}}
    )
    if "error" in opened:
        print(f"the session refused the document: {opened['error']}",
              file=sys.stderr)
        return 1
    first_tile = time.monotonic() - started

    # A screenful: the tile the host paints, the layer it selects against, and
    # the block map that told it where to scroll.
    tile = session.send(
        {"tile": {"id": "d1", "width": TILE_WIDTH, "height": TILE_HEIGHT}}
    )
    layer = session.send({"text": {"id": "d1", "top": 0.0,
                                   "bottom": float(TILE_HEIGHT)}})
    if "error" in tile or "error" in layer:
        print(f"the session refused a request: {tile.get('error')}"
              f" {layer.get('error')}", file=sys.stderr)
        return 1
    png = tile["tile"]["png"]
    screen_bytes = (
        tile["_wire_bytes"] + layer["_wire_bytes"] + opened["_wire_bytes"]
    )
    print(f"a screenful costs {screen_bytes} bytes: "
          f"{len(png)} of PNG, {layer['_wire_bytes']} of text response, "
          f"{opened['_wire_bytes']} of open response")

    # Engine-only edit latency, not the end-to-end NFR-1 boundary.
    body = document()
    samples = []
    for index in range(iterations):
        edited = body + f"A new sentence, number {index}.\n\n"
        started = time.monotonic()
        answer = session.send({"open": {"id": "d1", "text": edited}})
        if "error" in answer:
            print(f"the session refused an edit: {answer['error']}",
                  file=sys.stderr)
            return 1
        session.send(
            {"tile": {"id": "d1", "width": TILE_WIDTH, "height": TILE_HEIGHT}}
        )
        samples.append((time.monotonic() - started) * 1000.0)
    print(f"engine edit to encoded PNG over {len(samples)} edits: "
          f"p50 {percentile(samples, 0.5):.1f} ms, "
          f"p95 {percentile(samples, 0.95):.1f} ms, "
          f"median {statistics.median(samples):.1f} ms")

    # Opening a second document costs what parsing and laying it out costs; the
    # engine's own start is paid once per session.
    second = time.monotonic()
    answer = session.send({"open": {"id": "d2", "text": document()}})
    second_document = (time.monotonic() - second) * 1000.0
    if "error" in answer:
        print(f"the session refused a second document: {answer['error']}",
              file=sys.stderr)
        return 1
    print(f"start and first document: {first_tile * 1000.0:.0f} ms; "
          f"a second document in the same session: {second_document:.1f} ms")

    session.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
