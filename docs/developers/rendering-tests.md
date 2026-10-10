# Rendering regression tests

Run the committed Linux visual suite with no physical GPU:

```sh
bash scripts/test_render_goldens.sh
```

It needs Lavapipe and `pdftoppm` (`poppler-utils`). The normal Linux workspace
tests run the same comparisons in CI. The suite uses the production parser,
layout, highlighting, image scheduler, Vulkan renderer and PDF writer.
Windows and macOS continue to run their existing rendering tests.

Use `--update` only after reviewing an intended change. Normal runs never
create or replace baselines. Every RGBA channel of every pixel must be within
two levels of its baseline, and dimensions must match. This accommodates the
observed Mesa rounding difference without allowing a percentage of bad pixels.

| Suite | Baselines | Failure artifacts |
| --- | --- | --- |
| Markdown, MVSS and PDF pages | `crates/markview-render/tests/goldens/` | `artifacts/render-goldens/` |
| Native settings and button states | `tests/goldens/ui/` | `artifacts/ui-goldens/` |
| Mermaid and inline SVG | `tests/goldens/diagrams/` | `artifacts/diagram-goldens/` |
| Search and text selection | `tests/goldens/search/` | `artifacts/search/` |

Failures retain actual, expected and magenta difference PNGs. PDF cases also
retain the exported PDFs. CI uploads the artifacts even when tests fail.
Unused baselines fail the Markdown, UI and diagram suites, so removing a case
cannot silently leave an untested PNG. Every Markdown fixture must be registered.

## Markdown and composition

The fixtures cover the supported CommonMark and enabled Comrak extensions:

| Syntax | Fixtures |
| --- | --- |
| ATX headings 1–6, Setext headings and styled headings | `headings`, `prose` |
| Paragraphs, soft breaks, two-space and backslash hard breaks, HTML `<br>` | `breaks`, `prose` |
| Emphasis, strong, nested emphasis/strong, strikethrough, CJK adjacent emphasis | `inline`, `typography-zh` |
| Inline code, backtick literals, escapes, named/numeric entities | `inline` |
| Inline/reference/collapsed/shortcut links, URI/email and GFM autolinks | `inline` |
| Bullets, ordered lists, nested/mixed lists, tight/loose/empty items, task checkboxes | `lists`, `list-forms`, `combinations`, `mvss-markers` |
| Block quotes, nested quotes and all five GitHub alert types | `alerts`, `combinations` |
| GFM tables, left/center/right alignment, styled/multiline/image cells, escaped pipes and header-only tables | `table`, `table-forms`, `footnotes`, `combinations`, `overflow` |
| Fenced/backtick/tilde and indented code, known/unknown/unlabelled/empty code | `code`, `code-forms` |
| Markdown and HTML images, inline/block/nested images, dimensions, captions and transparency | `images`, `combinations`, `html` |
| Pending/failed images and invalid formula diagnostics | `diagnostics` |
| Footnotes, repeated/adjacent citations, multiline/list/code note bodies | `footnotes`, `theme-sampler` |
| Inline/display dollar, LaTeX and code math delimiters; math fences | `math`, `math-delimiters` |
| Thematic breaks with `*`, `-`, `_` and HTML `<hr>` | `breaks` |
| Front matter, collapsed/open/nested details and forced-open/export states | `details` |
| Supported HTML text tags, superscripts, anchors, comments, grouping containers and literal fallback | `inline`, `html`, `headings` |
| Mermaid flowcharts, sequence diagrams, Git graphs, pie charts and inline SVG text | `tests/fixtures/diagrams/` |

`combinations` crosses containers with inline styles: code, math, links, images
and tasks inside lists and quotes; styled cells inside a quoted table;
disclosures containing lists and tables; references inside quotes and cells.
The small fixtures identify failures more precisely than a single long sample.

## MVSS configurations

The theme sampler runs `builtin`, every `READER_THEMES` entry, and every
`PDF_THEMES` entry on desktop and phone media. Each PDF theme also exports
multiple real pages, rasterized at 96 DPI, with bilingual furniture. This uses
the configured fallback fonts; downloadable theme fonts are not fetched.

Custom overlays explicitly configure all drawing fields. The schema check
reads the Rust structs and requires a fixture declaration for every field;
new fields fail CI until they receive a visual case:

```sh
python3 scripts/check_render_coverage.py
```

| Configuration | Visual cases |
| --- | --- |
| Font candidates, fallback, weight/minimum weight, real italic, synthetic oblique, size, color, underline/strike, tracking | `mvss-text`, `inline`, regional typography, UI |
| Layout/background text edges, named metrics/numeric values and positive/negative baseline shifts | `mvss-text`, `mvss-edges-*`, bundled inline code |
| Padding, spacing, line height, borders, per-edge widths, radii/corners, heading markers, first/last-child compound rules | `mvss-boxes`, theme sampler, PDF custom |
| All six bullet shapes and cycling, marker columns, list indentation, three marker alignments, decimal/alphabetic/Roman/Chinese/circled numbering | `mvss-markers*`, `mvss-numbering-*` |
| Pending/completed checkbox fill, accent, check, border and radius | Marker cases and theme sampler |
| Collapsed/separate table borders and compound row styling | `table`, `mvss-boxes`, PDF custom |
| Caption title/alt/fallback/none, placement, image padding/background/borders and placeholders | `mvss-caption-*`, `mvss-boxes`, `diagnostics` |
| Code highlighting presets/none, label visibility, wrapped/scrolled blocks; error/front-matter visibility | Code, overflow, custom boxes, `mvss-hide-*` |
| Scrollbar track/thumb/hover colors, rest/hover thickness and overflow gutter | Custom UI, `mvss-scrollbars`, scrolled overflow |
| UI surfaces, typography, muted/accent/warning/error, shadow/scrim and button rest/hover/pressed/selected/disabled/focus | UI settings and button sampler |
| Selection, current/other search matches, hovered links | Search suite, `inline-selected`, `inline-hovered` |
| Paper dimensions, orientation, asymmetric margins, all six furniture slots, rule colors/widths and page styles | PDF custom |
| Widow/orphan limits, keep-together and fragmented cell decorations | PDF custom and existing pagination integration tests |
| Every Mermaid palette/layout field and generic SVG font mapping | All five Mermaid presets and custom flowchart/sequence/Git/pie/SVG |
| Output/device/orientation/platform media filters, including mobile inheritance | `media-*` resolves UI/PDF, desktop/phone/tablet, landscape/portrait and all six platforms on Lavapipe |

Version, metadata, output-target declarations and downloadable-font source
bookkeeping do not change document pixels directly. Their parsing, installation,
downloads and target validation remain covered by the stylesheet/font integration
tests. Pixel snapshots do not replace those checks, interaction/hit testing, or
the separate Android/Web platform tests.

## Chinese and English typography

Typography fixtures exercise paragraph-wide and greedy line breaking,
justified/ragged text, enabled/disabled hyphenation, first-line indentation and
larger text. They cover Latin ligatures, accents and combining marks,
nonbreaking spaces, soft hyphens, quotation marks, brackets and hanging
punctuation; Chinese opening/closing marks, punctuation compression, mixed
Han/Latin/digit spacing, code-chip gaps and CJK adjacent markup; formula and
superscript baselines; and emoji presentation beside bold and CJK text.

SC, TC and JP cases use pinned faces for their actual regional family and run
at 1.25× on a narrow column. Font subsets include fixture and localization text.
The render suites reject missing glyphs and clipped document fixtures. English
without hyphenation and intentionally wide tables also retain the expected
paragraph fallback behavior. The Noto subsets derive from the installed Noto
TTF/TTC families; the [SIL Open Font License](../../licenses/Noto-OFL.txt) is
retained. `scripts/generate_test_fonts.py` regenerates the shared SC/Latin/emoji
faces and the render-only TC/JP subsets.

## Reviewing a baseline change

Inspect the source fixture and every changed actual image, then compare its
expected/difference artifacts. Check that text, images and equations are present;
that only the intended layout/color changed; and that another theme, width,
regional convention, page or state did not change unexpectedly. Re-run the
normal suite after updating, followed by workspace tests and Clippy.

Keep the two-level tolerance fixed. Driver or Poppler changes need a controlled
comparison before updating images. Pinning the test fonts prevents host font
installation from changing the baseline; it does not assert that every user's
downloaded/system font produces identical pixels.
