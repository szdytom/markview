# Stylesheet guide

Markview stylesheets are UTF-8 TOML files with the `.mvss.toml` suffix. They define portable visual themes. Personal reading preferences—font size, column width, alignment, hyphenation, and first-line paragraph indent—belong in `settings.toml`, not in a stylesheet.

## Install and select a style

```sh
markview ss validate paper.mvss.toml
markview ss install paper.mvss.toml
markview document.md --style paper
markview document.md --style paper --style dark
```

Installation validates the complete file and copies it to the user stylesheet directory. It does not install fonts or enable the style. Use `--force` to replace an installed style whose `version` is equal to or lower than the incoming version. `ss validate` runs the same parse without copying anything, so a draft can be checked in place before it is installed; it reports the sheet's version and rule count on success.

The directory is next to `settings.toml`:

| Platform | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/styles/` or `~/.config/markview/styles/` |
| macOS | `~/Library/Application Support/markview/styles/` |
| Windows | `%APPDATA%/markview/styles/` |

The filename without `.mvss.toml` is the style ID. Only the first directory level is scanned. The bundled IDs `light`, `dark`, `celadon`, `blueprint`, `rosewood`, `print`, `monochrome`, `qibaishi`, `vangogh`, `mondrian`, and `builtin` are reserved (including case variants).

In the Settings panel, **Styles…** lets you enable, disable, and reorder styles. The leftmost selected style has the highest priority. `--style` replaces the session's selected list and is not saved. It cannot be combined with `--light` or `--dark`.

## Output targets

Declare the destinations at the top level, before any TOML table:

```toml
format_version = 2
version = 1
targets = ["ui", "pdf"]
```

- `targets = ["ui"]`: reader window and its theme selector.
- `targets = ["pdf"]`: PDF export and its theme selector.
- `targets = ["ui", "pdf"]`: both destinations; order does not matter.

Omitting `targets` defaults to `["ui", "pdf"]` for existing files. Empty arrays, duplicates, unknown names and non-array values are rejected. Destinations describe the whole file, not individual rules, and are checked separately on every selected stylesheet before merging. They do not cascade.

Bundled reader themes declare `["ui"]`, bundled paper themes declare `["pdf"]`, and the hidden `builtin` supports both. Selectors hide incompatible themes. A previously selected theme that changes destinations remains visible with an error so it can be removed; invalid reader updates retain the last valid appearance. Explicitly loading an incompatible theme reports its ID and the required destination.

The export panel shares paper layout and stylesheet selection between PDF and PNG, so both use the `pdf` destination. The diagnostic `--render` command can preview either destination, including `--style print`; it does not select a theme for the reader window. `ss validate` reports the declared targets.

## Choose a starting point

| Theme | Direction | Typography and signature |
| --- | --- | --- |
| `light` | Graphite on cool white; mineral blue accents | Serif reading text, sans-serif headings, quiet blue-grey chrome |
| `dark` | Soft slate with glacier blue accents | The same reading rhythm with subdued surfaces and silver text |
| `celadon` | Porcelain green and botanical ink | Spacious serif headings, diamond bullets, green inset quotations |
| `blueprint` | Chalk blue on drafting-paper navy | Sans-serif text and headings, square/minus bullets, blue quotation panels |
| `rosewood` | Plum shadows and rose accents | Literary serif headings, diamond bullets, plum quotation panels |

### PDF themes

All paper themes use `targets = ["pdf"]` and appear in the export selector. They keep white paper and inherit Print's page setup, font fallbacks and 0.75em page furniture. The new themes add a running title and right-aligned page count; `print` retains its centered page count and empty header.

| ID | Direction | Signature |
| --- | --- | --- |
| `print` | Neutral paper default | Black text, modest grey surfaces and sans-serif headings |
| `monochrome` | Minimal black and white | Unfilled quotation and code boxes, greyscale rules and uncolored code |
| `qibaishi` | Ink and vermilion | Literary serif headings, open quotations and restrained red details |
| `vangogh` | Indigo and wheat gold | Large serif title, indigo headings and golden quotation panels |
| `mondrian` | Vivid primary colors | Red title, blue headings and quote bars, yellow table header, black grid |

The three artist themes interpret the supplied CSS references through native MVSS typography and geometry. They do not depend on CSS, downloaded fonts or decorative images. Monochrome controls stylesheet and syntax colors; embedded images and color Emoji retain their original colors.

```sh
markview --pdf examples/themes.md --style monochrome --output monochrome.pdf
markview --pdf examples/themes.md --style vangogh --output vangogh.pdf
```

Their source files live in [`crates/markview-core/styles/`](../crates/markview-core/styles/). Copy a visible theme to a **new filename** to start a standalone palette, or write a small override and layer it above an existing theme.

`builtin.mvss.toml` supplies shared fonts, base typography, geometry, and safe fallback colors. It is always the last, lowest-priority layer, never appears in the UI, and cannot be selected, installed, or replaced. An empty reader style list uses only this fallback. The normal default chooses `light` or `dark`; neither is an implicit parent of another theme. PDF exports put `print` above `builtin` before applying the requested styles.

## A minimal valid file

```toml
format_version = 2
version = 1

[meta]
name = "Paper"
description = "Warm reading theme"

[[rule]]
when = ["body"]
color = "#292524"
background = "#FAF8F2"
font = [{ family = "serif" }, { family = "serif[cjk]" }, { family = "emoji", weight = 400 }]
line_height = 1.65

[[rule]]
when = ["link"]
color = "#315D86"
decoration = ["underline"]
```

`format_version` describes the file format and must be `2`. `version` is the installed theme's revision, a non-negative integer. `meta` is optional and does not participate in styling.

## Rules and conditions

A stylesheet is a list of `[[rule]]` tables. Each rule names the **conditions** it requires in `when` and then declares visual fields:

```toml
[[rule]]
when = ["code"]
font = [{ family = "monospace" }]
size = 0.9
background = "#EFF1F3"

[[rule]]
when = ["strong", "code"]
weight = 400
```

A condition is one fact about a rendered run: the blocks that contain it, the part of a block it belongs to, the inline markup it carries, and its state. The vocabulary is closed:

| Area | Conditions |
| --- | --- |
| Blocks | `body`, `p`, `h1`–`h6`, `blockquote`, `list`, `enum`, `list_item`, `footnote`, `code_block`, `table`, `hr` |
| Block parts | `label`, `cell`, `header`, `marker`, `task_marker`, `caption`, `placeholder` |
| Inline | `em`, `strong`, `link`, `del`, `sup`, `footnote_ref`, `code`, `math` |
| State | `hover`, `error` |
| Surfaces and UI | `img`, `selection`, `scrollbar`, `ui`, `toolbar`, `statusbar`, `panel`, `button` |
| Paper | `page`, `page_header`, `page_footer`, `page_number` |

`page` paints the exported sheet; the other three style page furniture. They never apply to the reader window, and a theme that ignores them still exports: the PDF falls back to the body appearance.

A rule applies to a run when **every** condition it names holds for that run. The order inside `when` is not part of the rule's identity, so `["strong", "code"]` and `["code", "strong"]` are the same rule, and a file that declares both is rejected as a duplicate. There are no selectors, variables, `inherit`, `unset`, imports, scripts, or remote resources.

Footnote links are clicks that move inside the document: a reference jumps to its note, and the note's number jumps back to the citation it was opened from. They carry `footnote_ref` instead of `link`, so a theme can mark them without recoloring every hyperlink; `["footnote_ref", "hover"]` styles the link under the pointer. Consecutive references share one bracket pair, as in `[1,2]`, and only their numbers stay click targets.

## Composition

Because a rule names a set of conditions, combinations need no new vocabulary. Inline code inside a heading, a quote, or strong text is written directly:

```toml
[[rule]]
when = ["code"]
background = "#EFF1F3"

[[rule]]
when = ["strong", "code"]
weight = 400

[[rule]]
when = ["h2", "code"]
background = "#E8EEF5"

[[rule]]
when = ["blockquote", "code"]
background = "#F3F0EA"
```

Conditions are entered as layout descends into the document, and a rule joins once its last condition is present. Later conditions therefore override earlier ones, in this order: containing blocks, the block itself, its part, inline markup, then state. Within one step, a rule that names more conditions overrides a rule that names fewer, so `["strong", "code"]` overrides both `["strong"]` and `["code"]`, and every field it leaves out still comes from them.

This means a theme only writes the exceptions it cares about. A field omitted by every matching rule falls back to the containing block, and ultimately to `["body"]`.

A block's own box is the exception. Its background, border, padding, spacing, and size come only from rules that name the element or a specialization of it, so `["p"]` styles a paragraph in any context and `["blockquote", "p"]` styles a paragraph in a quote, while a container's `["blockquote"] background` never paints its children. A quote's padding and border wrap its content alone: the leading space of its first child and the trailing space of its last stay outside the box, so the bar stays centered on the text. Inline runs keep the same split: a background must come from a rule that names inline markup, so `["code"]` and `["blockquote", "code"]` paint a code chip but `["blockquote"]` does not.

## Fields

Text conditions accept `color`, `font`, `weight`, `size`, `decoration`, and `background`. `padding` on `code` insets its chip: the left and right sides widen the run and push its neighbours along, and the top and bottom sides make the chip taller without changing the line height, so code beside CJK or punctuation is not cramped. Block conditions additionally accept `line_height`, `space_before`, `space_after`, and the container fields `padding`, `border_color`, `border_width`, and `radius`. Parts that are not containers—`label`, `marker`, `caption`, and `placeholder`—reject container geometry. A `task_marker` is a drawn box rather than a text part, so it also takes `background`, `accent`, `border_color`, `border_width`, and `radius`. `indent` styles `list` and `enum`; `align` places an image, positions a `marker` or `task_marker` in its column, and places an ordered list's numbers; `numbering` formats those numbers; `shape` picks a bullet's graphic; `source` belongs to image conditions; `show` belongs to `error`.

A list marker reserves a column before its item text. `marker` covers bullets, `task_marker` covers checkboxes, and `enum` covers ordered numbers, so each kind can be placed on its own with `align = "left"`, `"center"`, or `"right"`; the bundled styles center all three. A number without an `enum` alignment follows the `marker` one. A bullet is drawn rather than typed—`shape` is `disc`, `square`, `triangle`, `diamond`, `plus`, or `minus`—so bullets and checkboxes are never part of copied text, while ordered numbers stay text, written and copied exactly as the numbering pattern spells them. A checkbox is a rounded box centered on its item's first line: `background` fills a pending box, `accent` fills a completed one, `border_color` and `border_width` draw its outline, `radius` rounds it, and `color` draws the check. Box and mark are both vector geometry, so no font can substitute a different shape or size.

`shape` also takes a list, one entry per bullet nesting level and then repeating: `shape = ["plus", "minus"]` draws a plus on the first level and a minus on the second, and a plus again on the third. Ordered levels do not advance the cycle.

```toml
[[rule]]
when = ["marker"]
align = "center"
shape = ["plus", "minus"]

[[rule]]
when = ["enum"]
align = "right"
numbering = "1.a."
```

`numbering` is a pattern in Typst's notation: literal prefixes, one or more counting symbols, and one suffix. A counting symbol is the character a numeral system uses for one—`1`, `a`/`A`, `i`/`I`, `α`/`Α`, `א`, `一`/`壹`, `あ`/`ア`, `가`/`ㄱ`, `١`/`۱`/`१`/`১`/`ক`, `①` (up to fifty), `⓵` (up to ten), or `*` for note symbols—and everything else prints as it stands. The number of counting symbols is the number of nesting levels the pattern addresses, and the last one repeats for deeper lists, so `1.a.` numbers the first level `1.`, the second `a.`, and the third `a.` again. A system that cannot write a number—an alphabetic zero, a circled number past its range—falls back to decimal. The default is `1.`.

The column grows to the widest number a list actually renders, so a wide format such as `I.` or `一、` never runs into the item text.

`page` accepts only `background`. The furniture conditions accept the text fields, so a page number can be smaller or greyer than the header text beside it.

Special properties include `theme` on `["code_block"]` alone (`theme = "none"` disables syntax colors and uses the code block text color), scrollbar colors and thicknesses on `["scrollbar"]`, `muted`/`accent`/`error`/`shadow`/`scrim` on `["ui"]`, `accent` on `["task_marker"]`, and `hover_background`/`active_background`/`disabled_color`/`focus_color` on `["ui", "button"]`. The UI theme controls appearance, not widget layout or dimensions.

Colors are sRGB `#RRGGBB` or `#RRGGBBAA`; `body.background` must be opaque. Sizes and spacing are positive or non-negative finite values. `size` is relative to the reader's base size, `line_height` is a multiple of the condition's size, block spacing and padding use base-size units, and an inline code chip's padding scales with the text around it. Border width and radius use logical pixels. Unknown conditions, fields, types, and enum values are errors.

## Paper

The PDF export always starts from the bundled `print` stylesheet, and `--style` layers a named style supporting `pdf` on top of it. A style may also set the `[page]` table, which is the only table besides `fontdef`, `meta`, and `rule`:

```toml
[page]
size = "a4"                  # a3, a4, a5, a6, b5, letter, legal, tabloid, or WIDTHxHEIGHT in mm
landscape = false
margin = [22, 20, 22, 20]    # millimetres: one value, two (vertical, horizontal), or four (top, right, bottom, left)
header_left = "{title}"      # six slots; an empty string hides the slot
header_center = ""
header_right = ""
footer_left = ""
footer_center = "{page} / {pages}"
footer_right = ""
```

Slots are templates. `{page}`, `{pages}`, `{title}`, and `{path}` are the supported placeholders; any other name is rejected at parse time, and there is deliberately no date, so the same document always exports the same bytes. A slot holding a page number is styled by `page_number` and the rest by `page_header` or `page_footer`. `--paper`, `--landscape`, `--margin`, `--header*`, and `--footer*` override these fields for one run.

## Cascade and inheritance

The leftmost selected stylesheet has the highest priority. Implementation merges from the fallback upward, processing selected styles right to left, by condition set. For `--style personal --style dark`, the effective order is `personal → dark → builtin`. A field omitted by a higher-priority style remains from the lower-priority style; arrays replace the entire lower-priority array.

Text properties inherit from the containing block. Backgrounds, borders, padding, and spacing do not inherit.

## Fonts and fallback

Fonts are named by ordered candidates. A candidate must reference an installed family or one of the generic families `serif`, `sans-serif`, and `monospace`:

```toml
[[fontdef]]
id = "reading"
lookfor = ["Noto Serif", "Georgia"]

[[rule]]
when = ["body"]
font = [{ family = "reading" }]
```

Use `variant = "normal"`, `"italic"`, or `"oblique"`, and an optional weight from 1 to 1000. Markview skips a candidate when the face, requested style, or complete grapheme cluster is unavailable; it does not synthesize weight. A slanted candidate may set `synthetic_italic = true` to shear an upright face by 14° instead of being skipped, which is what CJK families—they rarely ship an italic—need:

```toml
[[rule]]
when = ["em"]
font = [
	{ family = "serif", variant = "italic" },
	{ family = "serif[cjk]", variant = "italic", synthetic_italic = true },
]
```

The flag applies only to `variant = "italic"` or `"oblique"`; a real italic or oblique face is still preferred when one exists. CJK variants may be defined with `type = "SC"`, `"TC"`, or `"JP"`. A user may override a definition with `[[fontdef-override]]`, but stylesheet files cannot bundle font files or download them.

An Emoji definition sets `emoji = true`, which makes the family the face for Emoji text rather than one candidate among the reading fonts:

```toml
[[fontdef]]
id = "emoji"
emoji = true
lookfor = ["Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji"]

[[rule]]
when = ["body"]
font = [{ family = "serif" }, { family = "emoji", weight = 400 }]
```

A grapheme cluster that Unicode presents as Emoji—a character with `Emoji_Presentation`, or any cluster carrying a `U+FE0F` selector—takes the Emoji face even when an earlier text candidate also covers it, which keeps check marks and warning signs colored instead of taking a symbol glyph from the CJK or symbol family that happens to hold one. A `U+FE0E` selector asks for the text presentation again. A text cluster never takes the Emoji face until the other candidates are exhausted, wherever the definition sits in the rule's list. Many Emoji families ship only a regular face, so an Emoji candidate is usually written with `weight = 400`.

Redefining a bundled `fontdef` id replaces its whole definition, so a style that redefines `emoji` repeats the flag; `[[fontdef-override]]` changes only the family names and keeps it.

## Heavier CJK UI labels

MVSS accepts a per-candidate `weight` from 1 to 1000. It is an **absolute** weight, not an offset from the inherited one. Markview requires an exact static weight or a variable font whose `wght` axis covers the requested value; it does not synthesize bold or round 450 to 500. An unavailable candidate is skipped. A `fontdef` chooses its first installed family before matching weight, so later `lookfor` entries do not rescue a missing Medium face in that family.

Bundled reader and PDF themes now prefer CJK weight 500 throughout, then fall back to the inherited weight when Medium is unavailable. [UI CJK Medium](../examples/ui-cjk-medium.mvss.toml) also provides this behavior as a focused overlay for custom themes. Latin retains its normal UI weight and Emoji stays at 400. Install it and place it before the reader theme:

```sh
markview ss install examples/ui-cjk-medium.mvss.toml
markview examples/themes.md --style ui-cjk-medium --style light
```

The overlay affects UI labels, not document typography. A fixed 500 candidate also replaces an inherited 700 for CJK when Medium exists; use it deliberately if a theme relies on bold UI hierarchy. It is not a general “add 100” setting.

The GPU comparison uses the host's installed fonts and draws 400, 450 with fallback, and 500 with fallback at 12/14/16 logical pixels on light/dark panels and at 1×, 1.25× and 2× scale:

```sh
cargo test -p markview cjk_ui_weight_comparison -- --ignored --nocapture
```

Images are written to `artifacts/cjk-weight/`. With the tested static Noto Sans CJK SC faces, 500 improves small-label stroke visibility, while 450 falls back to Regular. Other families and operating systems need their own check; the screenshot's Traditional/Japanese sample still uses the SC font convention for this controlled comparison.

## Images and captions

```toml
[[rule]]
when = ["img"]
align = "center"
padding = 0.3
border_width = 1.0
border_color = "#D8DEE3"

[[rule]]
when = ["img", "caption"]
source = "title_or_alt"
align = "center"
size = 0.8
color = "#69747E"
```

`align` affects image-only paragraphs. Images mixed with text remain inline and never create text wrapping on their sides. A single image paragraph may show a caption, using `title_or_alt`, `title`, `alt`, or `none`; multiple-image and mixed paragraphs do not show captions. `["img", "placeholder"]` styles loading and error text.

## Live updates and safe authoring

Markview watches installed styles and settings. A valid save applies automatically; an invalid stylesheet leaves the previous effective style active. Color-only changes can repaint cached layout, while font and geometry changes reflow it.

Keep a style focused on visual decisions, name the conditions a run really has rather than trying to imitate CSS, and test it with both Latin and CJK text, formulas, code, tables, links, selections, and missing images. Do not rely on a font that is unavailable on the target machine; provide an ordered fallback list.

## Authoring workflow

1. Declare `targets` for the intended destinations, then choose the reading use case and a small palette: paper, ink, raised surface, muted ink, accent, and border. Coordinate document colors with `ui`, panels, toolbar/statusbar and button states. Keep small labels readable; aim for at least 4.5:1 text contrast.
2. Change only the properties your theme owns. Shared font definitions and geometry already come from `builtin`; do not copy them wholesale. For a full dark palette, cover code, labels, markers, task boxes, tables, image placeholders, selection and scrollbar as well as body and UI colors. Choose a compatible syntax-highlighting `theme` on `code_block`.
3. Set typography intentionally: heading scale, spacing, line height and one distinguishing device such as quote treatment or bullet shapes. If changing body font roles, review `em` too: the fallback explicitly uses italic serif candidates. Font arrays replace the complete fallback array; retain CJK and regular-weight Emoji candidates.
4. Validate and install under a new ID, then open the fixture below. Installed files hot-reload when saved. Increment `version` when distributing an update; `--force` also permits reinstalling an equal or older revision.

```sh
cargo run -- ss validate path/to/my-theme.mvss.toml
cargo run -- ss install path/to/my-theme.mvss.toml
cargo run -- examples/themes.md --style my-theme
cargo run -- --render examples/themes.md --style my-theme --output /tmp/my-theme.png
```

Review the same content in every theme at narrow and wide reading measures, including Latin/CJK, italic/bold, code, math, nested lists, tables, captions and unavailable images. In the window also check hover, selection, keyboard focus, settings/export panels and scrolling. A static document render does not exercise those interactive states.

For a repository-bundled theme, add the file to `Stylesheet::named_rules` and its ID to `Stylesheet::READER_THEMES` or `Stylesheet::PDF_THEMES` according to its destination in `crates/markview-core/src/style.rs`; discovery and reserved-ID checks use that registry. Run `cargo fmt --all` and `cargo test --workspace`, then render the fixture. A new theme usually needs no parser or renderer changes. Extend MVSS only for a concrete visual requirement that existing fields cannot express, with parser and rendering tests plus documentation.
