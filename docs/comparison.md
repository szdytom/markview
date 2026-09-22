# Comparison

The [README](../README.md) shows one figure that sets the same text twice, once
through a browser engine and once through Markview, and tables that time the
readers — opening a document, and what each holds in memory afterwards — and the
PDF pipelines against each other. This page records how all of them are produced
and what they do and do not claim. Markview's own numbers, measured against its
targets rather than against other programs, live in the [performance
model](performance.md).

## The figure

`docs/screenshots/en-comparison.png` is produced by
`scripts/render_typography_comparison.py` from one two-paragraph Markdown file,
`docs/screenshots/source/en/justification.md`, set as:

| Setting | Both panels |
| --- | --- |
| Measure | 340 logical pixels |
| Type | 18 logical pixels, Georgia, line height 1.65 |
| Paragraph gap | 0.8 em |
| Density | Two device pixels per logical pixel |

The right panel is `markview render` on the light stylesheet with the reader's
defaults, so justification, whole-paragraph line breaking, hyphenation and the
bounded word space are all in effect. The left panel is a headless Chromium page
that pandoc renders the same Markdown into, carrying a conventional
Markdown-preview stylesheet: the same font, size, measure, paragraph gap and
ragged right edge that a browser-based preview ships with, and `hyphens: auto`
with `hyphenate-limit-chars: 6 2 2`, so that the engine is asked for hyphenation
rather than silently denied it.

## What the figure shows

The left panel is what a browser-based preview gives by default: one line at a
time, a ragged right edge, and no hyphenation. Chromium on this host ships no
hyphenation dictionaries, so even the `hyphens: auto` it was given changes
nothing — the same paragraph renders byte for byte identically with `hyphens:
none`. A long word that does not fit is moved whole to the next line, and the
line it left behind ends short.

The right panel justifies the column. Every line reaches the same right edge, the
long words are broken rather than moved (`con-spicuous`,
`incomprehensibili-ties`, `com-mitted`), and every word space stays inside
Markview's bounds of two thirds and one and a half of the natural width.

## Why the left panel is ragged

Because that is what a browser-based reader does. Markdown previews in the
browser tradition do not justify their column, and neither VS Code nor MarkText
nor a default Obsidian vault turns it on; the ragged edge is the real
out-of-the-box behaviour, so it is the honest thing to show.

Asking such a preview to justify does not close the gap. It is one CSS rule away,
and it was the first version of this figure, but the engine still has only one
lever on a line that does not fit — the word space — because it cannot hyphenate.
At this measure its spaces then stretched to roughly two and a half times the
natural width and the paragraph read as rivers of white. That observation is
verified by the same fixture but is deliberately not what the figure shows: the
figure compares defaults, not a tuned browser.

## Opening a document

`scripts/compare_readers.py` starts each reader the way a person does — one
command, one file, a cold process — and watches its window until the page stops
changing. The fixtures come from `scripts/comparison_fixtures.py`: generated
prose, and the same prose with a block of display formulas after every paragraph,
at an exact number of bytes.

"The document is on screen" needs a definition that does not depend on the
toolkit, because only one of these programs can report its own timing. Each frame
is compared with the frame the application settles on by itself, and the reported
time is the first frame that agrees with it: half of the document's ink for *first
frame*, 95 per cent of it for *complete*. The readers are MarkText 0.19.1, a
browser-based preview that renders the document in its window, and
SuperGoodViewer 1.0.8, which compiles the document to a PDF and draws that, both
against Markview built from this tree. SuperGoodViewer is the project's released
Linux archive rather than a build from a checkout, because its interface is
Flutter and this host has no Flutter SDK.

| Document | Markview first / complete | SuperGoodViewer first / complete | MarkText first / complete |
| --- | --- | --- | --- |
| 10 KiB of prose | 0.117 / 0.117 s | 0.625 / 0.673 s | 0.957 / 0.957 s |
| 100 KiB of prose | 0.123 / 0.123 s | 0.730 / 0.730 s | 1.000 / 1.000 s |
| 10 KiB, 108 display formulas | 0.123 / 0.123 s | 0.645 / 0.645 s | 1.195 / 1.195 s |
| 100 KiB, 1092 display formulas | 0.127 / 0.127 s | 0.771 / 0.771 s | 2.863 / 2.863 s |

Medians of three runs, interleaved so that all three readers met the same machine
load. Every reader paints its page in one piece at these sizes, so its first frame
and its complete frame are the same number, save SuperGoodViewer on 10 KiB of
prose, where a late window update puts its complete frame 48 ms after its first.

SuperGoodViewer 1.0.8 renders the mathematics fixtures that 1.0.7 rejected. Its
`\begin{pmatrix}` failure was a missing mitex prelude, fixed upstream after the
previous comparison, so the two rows it used to fail are now measured like the
rest.

**Resident memory** comes from the same runs: once a document has settled, the
resident set of every process in the reader's process group is summed, because
an Electron reader is several processes and the number a person reads off a
system monitor is their sum.

| Document | Markview | SuperGoodViewer | MarkText |
| --- | ---: | ---: | ---: |
| 10 KiB of prose | 50.5 MiB | 299.9 MiB | 693.2 MiB |
| 100 KiB of prose | 52.2 MiB | 344.4 MiB | 703.2 MiB |
| 10 KiB, 108 display formulas | 54.7 MiB | 299.3 MiB | 750.4 MiB |
| 100 KiB, 1092 display formulas | 55.4 MiB | 362.2 MiB | 1147.6 MiB |

SuperGoodViewer keeps a cache of compiled documents under
`$HOME/.cache/supergoodviewer`, so each run is given a private `HOME` and every
open is a first open. Opening the same file twice is served from that cache
instead — its own documentation quotes 0 ms for it — and that is a different
measurement from the one in this table.

Two attempts were needed to define "on screen", and both failures are worth
recording because they are the obvious ways to get this wrong. The first was
agreement over the whole window, which is useless: a page is mostly background,
a loading page has the same background as a finished one, and so every frame
"agreed" from the moment the window was painted. The second was treating a still
window as a finished one, which is wrong for the same reason in the other
direction: MarkText holds a perfectly static loading page for about two seconds
before its document appears. The measurement therefore requires both, that the
window be unchanged and that it carry at least one per cent ink — a loading page
has 0.2 per cent against 2.8 to 6.1 per cent for a document — and scores
agreement over the pixels where the settled page actually has ink. A run that
never satisfies both is reported as having hit the limit rather than being
counted as finished. A reader that refuses the document outright is caught in its
own log and reported as such, because a compile-error notice is also perfectly
still and carries ink.

The frames are the reader's own window, read through Xlib at about 300 frames per
second (3 ms each, the top 400 physical rows). Each reader opens its own default
window on this display — 1200 × 800 logical pixels at device scale 2 for Markview,
2400 × 1600 physical for MarkText, 2560 × 1440 physical for SuperGoodViewer — so
none of them draws a smaller canvas than the others. Two things forced that design.
Under a Wayland compositor the X root window holds nothing, so a screen grab of
the desktop is blank; and neither Xvfb nor Xephyr can host Markview at all,
because Mesa's Vulkan needs DRI3 to present and neither server offers it to
clients. A real X server is therefore the only place all readers can run
together — SuperGoodViewer is asked for its X11 backend for the same reason.

## Sending a document to PDF

`scripts/compare_pdf_engines.py` runs one command per engine and waits for the
file, then reports what came out. Every engine is given the same reproducible
build environment (`SOURCE_DATE_EPOCH=0`); the figure is the median of three warm
runs, with a separate cold run recorded in `artifacts/comparison/`.

| Engine | 10 KiB | 100 KiB | Pages | Size | Same bytes twice |
| --- | --- | --- | --- | --- | --- |
| `markview pdf` | 0.041 s | 0.074 s | 4 / 40 | 62 / 425 KiB | yes |
| `sgv export` | 0.102 s | 0.163 s | 3 / 28 | 24 / 142 KiB | yes |
| `pandoc --pdf-engine=typst` | 0.486 s | 0.678 s | 3 / 31 | 28 / 162 KiB | yes |
| `pandoc` → headless Chromium | 0.614 s | 0.757 s | 4 / 38 | 45 / 172 KiB | no |
| `pandoc --pdf-engine=xelatex` | 1.838 s | 2.137 s | 4 / 35 | 19 / 99 KiB | no |

All five recover the document's text through `pdftotext` essentially perfectly
(a ratio of 1.00 against the source), so text extraction is a tie and not a
differentiator. The page counts are not comparable: each engine used its own
default paper and margins, which nobody asked them to agree on. Markview's file
is the largest and XeLaTeX's the smallest, and XeLaTeX's is only reachable after
installing TeX Live, which is measured in gigabytes here. SuperGoodViewer's
exporter is its reader's engine behind a command line, with nothing to install
beyond the archive, and it writes the same bytes on every run as well.

## What it does not claim

- These are one machine's numbers from one day, taken while the machine was doing
  other work (a load average of a few). They are not a specification.
- The readers are compared cold, from process start. An already-running MarkText
  is much faster than 0.96 s because most of that number is its own window being
  created; a person who keeps it open pays that cost once, not per document.
- SuperGoodViewer is measured as a released archive, not as a build from source,
  so its binary is whatever its maintainers shipped — here 1.0.8. MarkText is the
  packaged 0.19.1. Neither was rebuilt or reconfigured for this comparison.
- Editing is not compared. MarkText notices that the file changed on disk and
  asks whether to reload it rather than reloading it, so it is not doing the same
  job as a reader that follows the file, and timing the two against each other
  would measure the difference in behaviour rather than in speed.
- VS Code is not in the tables. Its Markdown preview could not be opened reliably
  from a script here — a first-run dialog interrupted it, and the frames showed
  its editor rather than its preview — so it was dropped rather than measured
  badly.
- It compares an engine where it can. Every reader built on the same browser
  engine inherits the same line breaking, which is why the figure names no
  product, and the tables name only what was actually run.
- Ragged right is a legitimate choice, and plenty of readers prefer it. The
  figure compares what each engine does out of the box, not right against wrong.
- It is a snapshot of one week's versions. Browser engines keep adding
  typographic features, and a later release may justify, or hyphenate, where this
  one does not.
- It does not say that a browser cannot set text well, or that XeLaTeX is a bad
  PDF engine. It says that the specific guarantees Markview makes —
  justification, hyphenation, a global line breaker, a bounded word space, a
  small deterministic export — are not ones the alternatives currently offer
  together.

## Reproducing them

The figure needs pandoc, Chromium, Pillow and numpy; the reader benchmark needs
python-xlib and a real X server with a desktop session; the PDF benchmark needs
pandoc, Chromium and Poppler. All three need a release build that can reach a
GPU:

```sh
cargo build --release
python3 scripts/render_typography_comparison.py
# SuperGoodViewer is optional; the reader benchmark skips it when it is absent.
mkdir -p artifacts/supergoodviewer && tar xzf SuperGoodViewer-*-linux-x64.tar.gz \
  -C artifacts/supergoodviewer
python3 scripts/compare_readers.py --task open --runs 3 \
  --fixtures 10k,100k,math-10k,math-100k \
  --apps markview,marktext,supergoodviewer --json readers.json
python3 scripts/compare_pdf_engines.py --runs 3 --fixtures 10k,100k
```

`compare_readers.py` looks for the SuperGoodViewer executable at
`artifacts/supergoodviewer/supergoodviewer`, its archive's own name, and
`compare_pdf_engines.py` looks for the exporter shipped beside it at
`artifacts/supergoodviewer/bin/sgv`; `SUPERGOODVIEWER` overrides that path for
both. Each script prints its table and can write the raw samples to JSON with
`--json`.
