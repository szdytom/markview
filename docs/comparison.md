# Comparison

The [README](../README.md) shows one figure that sets the same text twice, once
through a browser engine and once through Markview, and two tables that time the
readers and the PDF pipelines against each other. This page records how all three
are produced and what they do and do not claim. Markview's own numbers, measured
against its targets rather than against other programs, live in the [performance
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

The right panel is `markview --render` on the light stylesheet with the reader's
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
frame*, 95 per cent of it for *complete*. The reader is MarkText 0.19.1, a
browser-based preview that renders the document in its window, against Markview
built from this tree.

| Document | Markview first / complete | MarkText first / complete |
| --- | --- | --- |
| 10 KiB of prose | 0.088 / 0.088 s | 0.970 / 0.970 s |
| 100 KiB of prose | 0.099 / 0.118 s | 0.991 / 0.991 s |
| 10 KiB, 108 display formulas | 0.110 / 0.112 s | 1.238 / 1.238 s |
| 100 KiB, 1092 display formulas | 0.094 / 0.094 s | 2.939 / 2.939 s |

Medians of three runs, interleaved so that both readers met the same machine
load. MarkText's window is created at about 0.85 s and its page then arrives in
one piece: its first frame and its complete frame are the same number in every
run, because it paints nothing until the whole document is rendered. Markview
paints the first screen as soon as it has one.

Two attempts were needed to define "on screen", and both failures are worth
recording because they are the obvious ways to get this wrong. The first was
agreement over the whole window, which is useless: a page is mostly background,
a loading page has the same background as a finished one, and so every frame
"agreed" from the moment the window was painted. The second was treating a still
window as a finished one, which is wrong for the same reason in the other
direction: MarkText holds a perfectly static loading page for about two seconds
before its document appears. The measurement therefore requires both, that the
window be unchanged and that it carry at least one per cent ink — a loading page
has 0.2 per cent against 4.8 to 6.2 per cent for a document — and scores
agreement over the pixels where the settled page actually has ink. A run that
never satisfies both is reported as having hit the limit rather than being
counted as finished.

The frames are the reader's own window, read through Xlib at about 300 frames per
second (3 ms each, the top 400 physical rows). Two things forced that design.
Under a Wayland compositor the X root window holds nothing, so a screen grab of
the desktop is blank; and neither Xvfb nor Xephyr can host Markview at all,
because Mesa's Vulkan needs DRI3 to present and neither server offers it to
clients. A real X server is therefore the only place all readers can run
together.

## Sending a document to PDF

`scripts/compare_pdf_engines.py` runs one command per engine and waits for the
file, then reports what came out. Every engine is given the same reproducible
build environment (`SOURCE_DATE_EPOCH=0`); the figure is the median of three warm
runs, with a separate cold run recorded in `artifacts/comparison/`.

| Engine | 10 KiB | 100 KiB | Pages | Size | Same bytes twice |
| --- | --- | --- | --- | --- | --- |
| `markview --pdf` | 0.042 s | 0.096 s | 4 / 34 | 62 / 307 KiB | yes |
| `pandoc --pdf-engine=typst` | 0.482 s | 0.723 s | 3 / 31 | 28 / 162 KiB | yes |
| `pandoc` → headless Chromium | 0.646 s | 0.833 s | 4 / 38 | 45 / 172 KiB | no |
| `pandoc --pdf-engine=xelatex` | 1.890 s | 2.156 s | 4 / 35 | 19 / 99 KiB | no |

All four recover the document's text through `pdftotext` essentially perfectly
(a ratio of 1.00 against the source), so text extraction is a tie and not a
differentiator. The page counts are not comparable: each engine used its own
default paper and margins, which nobody asked them to agree on. Markview's file
is the largest and XeLaTeX's the smallest, and XeLaTeX's is only reachable after
installing TeX Live, which is measured in gigabytes here.

## What it does not claim

- These are one machine's numbers from one day, taken while the machine was doing
  other work (a load average around three). They are not a specification.
- The readers are compared cold, from process start. An already-running MarkText
  is much faster than 0.97 s because most of that number is its own window being
  created; a person who keeps it open pays that cost once, not per document.
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
python3 scripts/compare_readers.py --task open --runs 3 \
  --fixtures 10k,100k,math-10k,math-100k --apps markview,marktext
python3 scripts/compare_pdf_engines.py --runs 3 --fixtures 10k,100k
```

Each script prints its table and can write the raw samples to JSON with `--json`.
