#!/usr/bin/env python3
"""Deterministic prose fixtures shared by the comparison benchmarks.

Every tool in a comparison has to read the same bytes, so the fixtures are
generated rather than checked in. They are prose only: a browser-based preview
and a PDF pipeline do not support the same document, and a comparison that
depends on mathematics or images measures the extension set instead of the
engine.
"""
import pathlib

SIZES = {"10k": 10 * 1024, "100k": 100 * 1024, "1m": 1024 * 1024,
         "math-10k": 10 * 1024, "math-100k": 100 * 1024}

PARAGRAPHS = [
    "A reader notices the measure before anything else. A column that is too "
    "wide loses the return sweep at the end of each line, and a column that is "
    "too narrow breaks so often that the rhythm of the sentence disappears "
    "under the line breaks. Sixty to seventy characters is the usual answer, "
    "which is why a reading pane is rarely as wide as the window that holds it.\n\n",
    "Justification is the harder half of the problem. Every line has to reach "
    "the same right edge, and the only material a compositor has to spend is "
    "the space between words. Stretch it too far and the paragraph acquires "
    "rivers of white; shrink it too far and the words crowd together. A "
    "bounded space, a hyphenation dictionary and a line breaker that considers "
    "the whole paragraph are what keep the texture even.\n\n",
    "Hyphenation is a language-specific business. English compounds break in "
    "predictable places, German stacks nouns into very long words, and Chinese "
    "does not break words at all but forbids certain punctuation at the start "
    "of a line. A reader that ships one dictionary and one rule set serves "
    "whatever language the dictionary was written for, and hands the rest back "
    "to the fallback.\n\n",
    "Mathematics in a paragraph is measured with the paragraph. An inline "
    "formula has to sit on the same baseline as the words around it, take part "
    "in the justification, and scroll sideways rather than push the column "
    "apart when it does not fit. Display formulas are easier: they stand alone, "
    "they are centred, and the paragraph resumes underneath them.\n\n",
    "Reading is not editing. A reader watches a file, keeps its place when the "
    "file changes underneath it, and follows a link to another document "
    "without leaving the one it is showing. An editor owns the file, so its "
    "problems are different ones: undo history, multiple cursors, and the "
    "question of what a save means when the file has changed on disk.\n\n",
    "Print is the other output. A page has a fixed measure, a fixed height, "
    "and rules about what may not be left alone at the foot of it: a heading "
    "travels with the paragraph it introduces, and a paragraph keeps a line or "
    "two on each side of a break. None of that exists on a screen that "
    "scrolls, which is why a reader and a printer disagree about the same "
    "document.\n\n",
]

FILLER = "Short words keep the final line readable and unremarkable. "

# Mathematics, as a block of its own after every paragraph, so a reader has to
# lay out display formulas, matrices and aligned systems to draw the page. A
# display formula is fenced with `$$` on their own lines: that is the form every
# reader in the comparison parses, and a one-line `$$...$$` is not.
MATH = [
    "An inline formula such as $E = mc^2$ or $x_1^2 + x_2^2 = r^2$ has to sit on "
    "the same baseline as the words around it and take part in the justification "
    "of the line it lands in.\n\n",
    "$$\n\\int_0^1 x^2\\,dx = \\frac{1}{3}\n$$\n\n",
    "$$\n\\sum_{i=1}^{n} \\frac{1}{i^2} = \\frac{\\pi^2}{6}\n$$\n\n",
    "$$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}"
    "\\begin{pmatrix} x \\\\ y \\end{pmatrix} = "
    "\\begin{pmatrix} ax + by \\\\ cx + dy \\end{pmatrix}\n$$\n\n",
    "$$\n\\begin{aligned}\n"
    "\\nabla \\cdot \\mathbf{E} &= \\frac{\\rho}{\\varepsilon_0} \\\\\n"
    "\\nabla \\cdot \\mathbf{B} &= 0 \\\\\n"
    "\\nabla \\times \\mathbf{E} &= -\\frac{\\partial \\mathbf{B}}{\\partial t}\n"
    "\\end{aligned}\n$$\n\n",
    "$$\n\\hat{f}(\\xi) = \\int_{-\\infty}^{\\infty} f(x)\\,e^{-2\\pi i x \\xi}\\,dx"
    "\n$$\n\n",
    "$$\n\\frac{\\partial u}{\\partial t} = \\alpha \\nabla^2 u + f(x, t)\n$$\n\n",
]


def prose(target, maths=False):
    """The fixture text for `target` bytes of UTF-8."""
    text = "# A comparison document\n\n"
    index = 0
    while True:
        block = PARAGRAPHS[index % len(PARAGRAPHS)]
        if maths:
            block += "".join(MATH)
        if len((text + block).encode()) > target:
            break
        text += block
        index += 1
    remaining = target - len(text.encode())
    return text + (FILLER * (remaining // len(FILLER) + 1))[:remaining]


def write(directory, sizes=("10k", "100k", "1m")):
    """Write the requested fixtures and return their paths, smallest first."""
    directory = pathlib.Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    paths = []
    for name in sizes:
        target = SIZES[name]
        maths = name.startswith("math-")
        path = directory / f"{'math' if maths else 'prose'}-{name.removeprefix('math-')}.md"
        text = prose(target, maths)
        assert len(text.encode()) == target
        path.write_text(text)
        paths.append(path)
    return paths
