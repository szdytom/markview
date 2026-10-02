#!/usr/bin/env python3
"""Report whether the committed test faces cover a document's characters.

The web demo ships only subsetted faces, so a character none of them covers
draws as tofu. With no argument, the demo document is located in
`web/apps/demo/src/documents.ts` (template literals and Markdown imports), else
stdin is read. `cmap` subtable formats 4 and 12
are parsed with `struct`, mirroring how the engine's `swash` selects and
maps them. Exit 0 when fully covered, 1 otherwise.

The coverage is the union over every face, which is coarser than what a run
draws: a character only one face carries is reported as covered even when the
family a body text run reaches cannot fall back to that face, so a style gap
still shows as tofu. A body-text sample is worth an eye on the canvas as well.
"""

import html
import os
import re
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONTS = os.path.join(ROOT, "crates", "markview-core", "tests", "fonts")
# The SPA keeps its samples as template literals in `documents.ts`.
DEMO_JS = os.path.join(ROOT, "web", "apps", "demo", "src", "documents.ts")
DEMO_HTML = os.path.join(ROOT, "web", "apps", "demo", "index.html")
JS_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", "`": "`", "$": "$"}


def format4(data, base):
    # Parallel arrays: endCode[], pad, startCode[], idDelta[], idRangeOffset[],
    # then the id values; `swash` caps codes at 0xFFFE and reads OOB ids as 0.
    (_, _, _, seg_x2) = struct.unpack_from(">HHHH", data, base)
    if len(data) < 16 + seg_x2 * 4:
        return set()
    n = seg_x2 // 2
    starts = struct.unpack_from(">%dH" % n, data, base + 16 + seg_x2)
    deltas = struct.unpack_from(">%dh" % n, data, base + 16 + 2 * seg_x2)
    offsets = struct.unpack_from(">%dH" % n, data, base + 16 + 3 * seg_x2)
    covered = set()
    for i, (start, delta, offset) in enumerate(zip(starts, deltas, offsets)):
        end = min(struct.unpack_from(">H", data, base + 14 + 2 * i)[0], 0xFFFE)
        if start > end:
            continue
        if offset == 0:
            covered.update(range(start, end + 1))
            zero = (-delta) & 0xFFFF  # the one code mapping to the .notdef glyph
            if start <= zero <= end:
                covered.discard(zero)
        else:
            pos = base + 16 + 3 * seg_x2 + 2 * i + offset
            for code in range(start, end + 1):
                p = pos + 2 * (code - start)
                value = struct.unpack_from(">H", data, p)[0] if p + 2 <= len(data) else 0
                if value and (value + delta) & 0xFFFF:
                    covered.add(code)
    return covered


def format12(data, base):
    # Header: format, reserved, length, language, numGroups; `swash` truncates
    # glyph ids to u16, so codes wrapping to 0 map to the .notdef glyph.
    (_, _, _, _, groups) = struct.unpack_from(">HHIII", data, base)
    if len(data) < 16 + 12 * groups:
        return set()
    covered = set()
    for i in range(groups):
        start, end, first = struct.unpack_from(">III", data, base + 16 + 12 * i)
        if start <= end:
            covered.update(range(start, end + 1))
            for code in range(start + (0x10000 - first) % 0x10000, end + 1, 0x10000):
                covered.discard(code)
    return covered


def face_coverage(path):
    """The codepoints the engine maps to a real glyph in this face."""
    data = open(path, "rb").read()
    (_, count) = struct.unpack_from(">IH", data, 0)
    # `cmap` offset out of the sfnt table directory.
    cmap = next((struct.unpack_from(">I", data, 20 + 16 * i)[0]
                 for i in range(count) if data[12 + 16 * i: 16 + 16 * i] == b"cmap"), None)
    if cmap is None:
        return set()
    # `swash` prefers a Unicode format 12 subtable over the first format 4
    # one; a (3, 0) symbol subtable is taken outright.
    chosen = None
    for i in range(struct.unpack_from(">H", data, cmap + 2)[0]):
        platform, encoding, offset = struct.unpack_from(">HHI", data, cmap + 4 + 8 * i)
        fmt = struct.unpack_from(">H", data, cmap + offset)[0]
        if fmt in (4, 12) and (
                (platform == 3 and encoding == 0)
                or (platform == 0 or (platform == 3 and encoding in (1, 10)))
                and (fmt == 12 or chosen is None)):
            chosen = (cmap + offset, fmt)
            if platform == 3 and encoding == 0:
                break
    if chosen is None:
        return set()
    return format4(data, chosen[0]) if chosen[1] == 4 else format12(data, chosen[0])


def unescape_js(literal):
    def expand(match):
        body = match.group(1)
        if len(body) > 1:
            return chr(int(body[1:].strip("{}"), 16))
        return JS_ESCAPES.get(body, match.group(0))
    return re.sub(r"\\([uUx][0-9a-fA-F{}]+|\S)", expand, literal)


def demo_document():
    """The demo document from the web demo, or None when not on disk yet."""
    if os.path.exists(DEMO_JS):
        source = open(DEMO_JS, encoding="utf-8").read()
        samples = [unescape_js(literal) for literal in
                   re.findall(r"`((?:[^`\\]|\\.)*)`", source, re.S)]
        for imported in re.findall(r"from [\"']([^\"']+\.md)[\"']", source):
            path = os.path.join(os.path.dirname(DEMO_JS), imported)
            samples.append(open(path, encoding="utf-8").read())
        if samples:
            return "\n".join(samples)
    if os.path.exists(DEMO_HTML):
        match = re.search(r'<textarea[^>]*id="source"[^>]*>(.*?)</textarea>',
                          open(DEMO_HTML, encoding="utf-8").read(), re.S)
        if match and match.group(1):
            return html.unescape(match.group(1))
    return None


def main(argv):
    if argv and argv[0] == "--self-test":
        sans = face_coverage(os.path.join(FONTS, "NotoSans-Regular-subset.otf"))
        cjk = face_coverage(os.path.join(FONTS, "NotoSansCJKsc-Regular-subset.otf"))
        assert 0x41 in sans and 0x4E2D in cjk, "known faces miss 'A' or '中'"
        print("self-test passed: 'A' and '中' are covered by their committed faces")
        return 0
    if argv:
        where, text = argv[0], open(argv[0], encoding="utf-8").read()
    else:
        where, text = "web demo", demo_document()
        if text is None:
            # Reading a terminal would block forever, and an empty pipe would
            # report success for zero characters, so a missing document is said
            # plainly instead.
            if sys.stdin.isatty():
                print("no document given and the demo document was not found "
                      f"({DEMO_HTML}); pass a path or pipe one in", file=sys.stderr)
                return 2
            where, text = "stdin", sys.stdin.read()
    covered = set()
    covered.update(c for name in sorted(os.listdir(FONTS)) if name.endswith((".otf", ".ttf"))
                   for c in face_coverage(os.path.join(FONTS, name)))
    # Control characters shape the source but are not drawn as glyphs.
    codes = {ord(c) for c in text if ord(c) >= 0x20}
    if not codes:
        # Otherwise an empty read looks like a pass and checks nothing.
        print(f"nothing to check: {where} has no printable characters", file=sys.stderr)
        return 2
    missing = sorted(codes - covered)
    for codepoint in missing:
        print("U+%04X  %s" % (codepoint,
              chr(codepoint) if chr(codepoint).isprintable() else "\\u%04x" % codepoint))
    if missing:
        print("MISSING: %d of %d distinct characters of %s are covered by no face"
              % (len(missing), len(codes), where))
        return 1
    print("OK: all %d distinct characters of %s are covered by the committed faces" % (len(codes), where))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
