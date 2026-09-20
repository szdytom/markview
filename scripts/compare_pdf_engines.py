#!/usr/bin/env python3
"""Compare the document-to-PDF pipelines a Markdown document can be sent to.

Each engine turns the same Markdown fixture into a PDF in one command, from a
cold process, and the report covers the time that takes as well as what came
out: how many pages, how many bytes, whether the text is extractable, whether
the fonts are embedded, and whether two runs produce identical bytes. Every
engine is given its own documented reproducibility recipe, which here means
`SOURCE_DATE_EPOCH` is fixed for all of them.

The engines are Markview, pandoc through XeLaTeX, pandoc through Typst, and
pandoc through a headless Chromium, which is what the browser-based exporters
do underneath.

Usage: scripts/compare_pdf_engines.py [--runs 3] [--fixtures 10k,100k] [--json FILE]
"""
import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import comparison_fixtures

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/markview"

# A print stylesheet for the browser engine, in the spirit of what a
# "Markdown to PDF" tool ships: a serif face, a readable size, and margins.
PRINT_CSS = """@page { margin: 20mm }
body { font: 12pt/1.5 Georgia, "Noto Serif", serif; color: #000; margin: 0 }
h1 { font-size: 1.6em }
"""


def engines():
    def markview(fixture, work):
        return [str(BINARY), "pdf", str(fixture), "-o", str(work / "out.pdf")]

    def xelatex(fixture, work):
        return ["pandoc", str(fixture), "-o", str(work / "out.pdf"),
                "--pdf-engine=xelatex"]

    def typst(fixture, work):
        return ["pandoc", str(fixture), "-o", str(work / "out.pdf"),
                "--pdf-engine=typst"]

    def chromium(fixture, work):
        html = work / "document.html"
        css = work / "print.css"
        css.write_text(PRINT_CSS)
        subprocess.run(["pandoc", str(fixture), "-o", str(html),
                        "--standalone", "--css", str(css)],
                       check=True, capture_output=True)
        return ["chromium", "--headless", "--no-sandbox", "--disable-gpu",
                f"--user-data-dir={work}/profile",
                "--no-pdf-header-footer",
                f"--print-to-pdf={work}/out.pdf", f"file://{html}"]

    return {"markview": markview, "pandoc+xelatex": xelatex,
            "pandoc+typst": typst, "pandoc+chromium": chromium}


def run_engine(build, fixture, work):
    """Run one engine once and return its wall time and output bytes."""
    work.mkdir(parents=True, exist_ok=True)
    pdf = work / "out.pdf"
    pdf.unlink(missing_ok=True)
    start = time.perf_counter()
    result = subprocess.run(build(fixture, work), capture_output=True,
                            text=True, env=environment())
    elapsed = time.perf_counter() - start
    if result.returncode != 0 or not pdf.exists():
        tail = (result.stderr or result.stdout).strip().splitlines()[-3:]
        raise RuntimeError(f"{fixture.name}: exit {result.returncode}: "
                           + " / ".join(tail))
    return elapsed, pdf


def environment():
    """Every engine gets the same reproducible-build environment."""
    env = dict(os.environ)
    env["SOURCE_DATE_EPOCH"] = "0"
    return env


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inspect(pdf, source):
    """Page count, byte size, extractable text and embedded fonts."""
    info = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True)
    pages = None
    for line in info.stdout.splitlines():
        if line.startswith("Pages:"):
            pages = int(line.split()[1])
    text = subprocess.run(["pdftotext", str(pdf), "-"],
                          capture_output=True, text=True).stdout
    letters = lambda s: sum(c.isalnum() for c in s)
    fonts = subprocess.run(["pdffonts", str(pdf)], capture_output=True,
                           text=True).stdout.splitlines()[2:]
    embedded = [line.split() for line in fonts if line.split()]
    return {
        "pages": pages,
        "bytes": pdf.stat().st_size,
        "text_ratio": round(letters(text) / max(letters(source), 1), 3),
        "fonts": len(embedded),
        "fonts_embedded": sum(1 for row in embedded if row[-4:-3] == ["yes"]
                              or "yes" in row),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=3,
                        help="timed runs per engine and fixture")
    parser.add_argument("--fixtures", default="10k,100k")
    parser.add_argument("--json", help="also write the raw report here")
    parser.add_argument("--engine", action="append",
                        help="restrict to one engine; repeatable")
    args = parser.parse_args()
    if not BINARY.exists():
        sys.exit(f"build {BINARY} first: cargo build --release")
    if not shutil.which("pandoc") or not shutil.which("chromium"):
        sys.exit("this script needs pandoc and chromium on PATH")

    report = {}
    with tempfile.TemporaryDirectory() as directory:
        work = pathlib.Path(directory)
        fixtures = comparison_fixtures.write(work / "fixtures",
                                             tuple(args.fixtures.split(",")))
        for name, build in engines().items():
            if args.engine and name not in args.engine:
                continue
            for fixture in fixtures:
                source = fixture.read_text()
                samples, first = [], None
                hashes = []
                for attempt in range(args.runs):
                    where = work / f"{name}-{fixture.stem}-{attempt}"
                    elapsed, pdf = run_engine(build, fixture, where)
                    hashes.append(digest(pdf))
                    if attempt == 0:
                        first = elapsed
                    else:
                        samples.append(elapsed)
                _, pdf = run_engine(build, fixture,
                                    work / f"{name}-{fixture.stem}-inspect")
                entry = {
                    "cold_s": round(first, 3),
                    "warm_median_s": round(statistics.median(samples), 3),
                    "warm_range_s": [round(min(samples), 3),
                                     round(max(samples), 3)],
                    "runs": args.runs,
                    "deterministic": len(set(hashes)) == 1,
                    **inspect(pdf, source),
                }
                report[f"{name}|{fixture.stem}"] = entry
                print(f"{name:16} {fixture.stem:6} "
                      f"cold {entry['cold_s']:6.3f}s "
                      f"warm {entry['warm_median_s']:6.3f}s "
                      f"{entry['pages']:>4} pages "
                      f"{entry['bytes'] / 1024:8.1f} KiB "
                      f"text {entry['text_ratio']:.3f} "
                      f"fonts {entry['fonts_embedded']}/{entry['fonts']} "
                      f"identical={entry['deterministic']}")
    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(report, indent=2) + "\n")
        print(f"wrote {args.json}")


if __name__ == "__main__":
    main()
