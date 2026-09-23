# Stylesheet guide

Markview stylesheets are UTF-8 TOML files with the `.mvss.toml` suffix. They define portable visual themes. Personal reading preferences—font size, column width, alignment, hyphenation, and first-line paragraph indent—belong in `settings.toml`, not in a stylesheet.

## Install and select a style

```sh
markview ss list
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

The filename without `.mvss.toml` is the style ID. Only the first directory level is scanned. The bundled IDs `light`, `dark`, `celadon`, `blueprint`, `rosewood`, `8-bit`, `print`, `monochrome`, `qibaishi`, `vangogh`, `mondrian`, and `builtin` are reserved (including case variants).

In the Settings panel's **Styles** tab (**Ctrl+T**) you can enable, disable, and reorder styles. The leftmost selected style has the highest priority. `--style` replaces the session's selected list and is not saved. It cannot be combined with `--light` or `--dark`.

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

The export panel shares paper layout and stylesheet selection between PDF and PNG, so both use the `pdf` destination. The diagnostic `render` subcommand can preview either destination, including `--style print`; it does not select a theme for the reader window. `ss validate` reports the declared targets.

## Choose a starting point

| Theme | Direction | Typography and signature |
| --- | --- | --- |
| `light` | Graphite on cool white; mineral blue accents | Serif reading text, sans-serif headings, quiet blue-grey chrome |
| `dark` | Soft slate with glacier blue accents | The same reading rhythm with subdued surfaces and silver text |
| `celadon` | Porcelain green and botanical ink | Spacious serif headings, diamond bullets, green inset quotations |
| `blueprint` | Chalk blue on drafting-paper navy | Sans-serif text and headings, square/minus bullets, blue quotation panels |
| `rosewood` | Plum shadows and rose accents | Literary serif headings, diamond bullets, plum quotation panels |
| `8-bit` | Green phosphor on near-black glass | Fusion Pixel document and UI text, square markers and terminal frames |

The `8-bit` theme uses Fusion Pixel 12px Monospaced (Simplified Chinese, Traditional Chinese and Japanese variants) throughout the document and UI, including headings, emphasis and code. Download its fonts from the Fonts panel or with `markview fonts download --style 8-bit`. Missing pixel fonts fall back to system monospace fonts. Emphasis uses an underline and strong text uses brighter phosphor, preserving the regular pixel face. Math retains its mathematical fonts.

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
markview pdf examples/themes.md --style monochrome --output monochrome.pdf
markview pdf examples/themes.md --style vangogh --output vangogh.pdf
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
| Blocks | `body`, `p`, `h1`–`h6`, `blockquote`, `list`, `enum`, `list_item`, `footnote`, `details`, `front_matter`, `code_block`, `table`, `hr` |
| Block parts | `label`, `cell`, `header`, `marker`, `task_marker`, `caption`, `placeholder`, `summary` |
| Inline | `em`, `strong`, `link`, `del`, `sup`, `footnote_ref`, `code`, `math` |
| State | `hover`, `error` |
| Surfaces and UI | `img`, `selection`, `scrollbar`, `ui`, `toolbar`, `statusbar`, `panel`, `button` |
| Paper | `page`, `page_header`, `page_footer`, `page_number` |

`page` paints the exported sheet; the other three style page furniture. They never apply to the reader window, and a theme that ignores them still exports: the PDF falls back to the body appearance.

A rule applies to a run when **every** condition it names holds for that run. The order inside `when` is not part of the rule's identity, so `["strong", "code"]` and `["code", "strong"]` are the same rule, and a file that declares both is rejected as a duplicate. There are no selectors, variables, `inherit`, `unset`, imports, or scripts. The only remote resource a stylesheet can name is a font family, declared under [`[[font-family]]`](#downloadable-fonts), and even that is never fetched until the reader asks for it.

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

Text conditions accept `color`, `font`, `weight`, `size`, `decoration`, and `background`. `padding` on `code` insets its chip: the left and right sides widen the run and push its neighbours along, and the top and bottom sides make the chip taller without changing the line height, so code beside CJK or punctuation is not cramped. Block conditions additionally accept `line_height`, `space_before`, `space_after`, and the container fields `padding`, `border_color`, `border_width`, and `radius`. Parts that are not containers—`label`, `marker`, `caption`, `placeholder`, and `summary`—reject container geometry. A `task_marker` is a drawn box rather than a text part, so it also takes `background`, `accent`, `border_color`, `border_width`, and `radius`. `indent` styles `list` and `enum`; `align` places an image, positions a `marker` or `task_marker` in its column, and places an ordered list's numbers; `numbering` formats those numbers; `shape` picks a bullet's graphic; `source` belongs to image conditions; `show` belongs to `error`.

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

A raw `<details>` block becomes a collapsible element: `summary` is its heading line and the body keeps ordinary Markdown. The source's `open` attribute sets the initial state, and the reader's own choice of state lives in interaction state rather than the document, so it survives a reflow and a reload starts from the source again. `details` owns the container surface and `summary` the line. The disclosure marker is vector geometry drawn in the summary's `color`, so no font changes it and `["summary", "hover"]` colors both the line and the marker under the pointer. Both exports show every body expanded.

A document may open with `---` fenced YAML front matter. It is rendered as a two-column table when every value is a scalar or a flat sequence of scalars, and as a `yaml` code block when any value nests, so a structure a table cannot carry keeps its source. `front_matter` owns the frame around either shape — its padding, background, corner radius and spacing, and `show = false` takes the whole block off the page while the metadata stays parsed. The shape underneath composes with it: `["front_matter", "table"]` and its `cell` reach the table alone, `["front_matter", "code_block"]` and its `label` the listing alone, and a field set on `["front_matter"]` alone never reaches either box. Keys are bold, so `["front_matter", "table", "cell", "strong"]` styles the key column. A metadata table has no header row, so `header` never applies to it.

`page` accepts only `background`. The furniture conditions accept the text fields, so a page number can be smaller or greyer than the header text beside it.

Special properties include `theme` on `["code_block"]` alone (`theme = "none"` disables syntax colors and uses the code block text color), scrollbar colors and thicknesses on `["scrollbar"]`, `muted`/`accent`/`error`/`shadow`/`scrim` on `["ui"]`, `accent` on `["task_marker"]`, and `hover_background`/`active_background`/`disabled_color`/`focus_color` on `["ui", "button"]`. The UI theme controls appearance, not widget layout or dimensions.

Colors are sRGB `#RRGGBB` or `#RRGGBBAA`; `body.background` must be opaque. Sizes and spacing are positive or non-negative finite values. `size` is relative to the reader's base size, `line_height` is a multiple of the condition's size, block spacing and padding use base-size units, and an inline code chip's padding scales with the text around it. Border width and radius use logical pixels. Unknown conditions, fields, types, and enum values are errors.

## Native document decorations

These properties work in both reader and export themes. They are resolved by the shared layout engine, so PDF and GPU output use the same geometry. Existing themes keep their original border behavior when these fields are absent.

| Property | Meaning |
| --- | --- |
| `border_edges = [top, right, bottom, left]` | Four nonnegative widths in logical pixels. Overrides `border_width`, draws inward, and reserves space in block/cell layout. Use zero for an absent edge. |
| `corner_radii = [top_left, top_right, bottom_right, bottom_left]` | Four nonnegative circular radii in logical pixels. Overrides `radius`; adjacent corners scale together to fit the box. |
| `heading_marker = [width, height, gap]` | A rectangular decoration before `h1`–`h6`, in base-font-size units. Reserves text width and aligns with the first line; a tall marker also reserves height. Zero width or height disables it. |
| `marker_color` | The heading marker's fill, using the usual hex color syntax. Omitted color is transparent. |
| `letter_spacing` | Extra advance in em, inherited by text; finite negative values tighten tracking. Applied during shaping, so wrapping, selection and PDF text positions agree. |
| `orphans`, `widows` | Positive line/band counts required on each side of a page break. The existing default is two. |
| `keep_together` | Prefer keeping a block on one page. A block taller than the page is allowed to split. |
| `wrap` | On `code_block`, override the destination's default line wrapping. |
| `show` | On `code_block` + `label`, show or hide the language label. |

`first_child` and `last_child` describe the immediate block's position among its siblings; on table cells they describe the row's position. They do not describe arbitrary descendants or individual characters. Position is replaced when entering another child container. A one-child container has both conditions. Add a block condition when targeting a particular element:

```toml
[[rule]]
when = ["h2"]
border_edges = [0, 0, 2, 0]
border_color = "#232323"
heading_marker = [0.78, 0.78, 0.65]
marker_color = "#F1CE46"

[[rule]]
when = ["blockquote", "p", "first_child", "strong"]
size = 0.825
letter_spacing = 0.09

[[rule]]
when = ["table", "cell"]
border_edges = [0, 0, 1, 0]

[[rule]]
when = ["table", "cell", "last_child"]
border_edges = [0, 0, 2, 0]
```

For horizontal-only tables, assign each shared edge to one row (normally its bottom edge) rather than drawing both adjacent edges. Header and last-row rules can set their own border colors; a one-row table can use a combined `header` + `last_child` rule. PDF fragments retain the top edge/corners only on the opening fragment and the bottom edge/corners only on the closing fragment.

Geometric and tracking changes invalidate affected layout caches. Color changes reuse geometry. Introducing the first positional rule also rebuilds layout to record positions; subsequent color changes to that rule do not. Pagination hints are recorded with the block ranges but used only by the PDF paginator, not by the reading window or continuous PNG export.

## Paper

The PDF export always starts from the bundled `print` stylesheet, and `--style` layers a named style supporting `pdf` on top of it. A style may also set the `[page]` table, which is the only table besides `fontdef`, `font-family`, `meta`, `mermaid`, and `rule`:

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

[page.header]
rule_width = 3.0           # Stroke thickness in points; zero disables it
rule_color = "#244C80"     # #RRGGBB or #RRGGBBAA; defaults to black

[page.footer]
rule_width = 1.5
rule_color = "#343C35"
```

Slots are templates. `{page}`, `{pages}`, `{title}`, and `{path}` are the supported placeholders; any other name is rejected at parse time, and there is deliberately no date, so the same document always exports the same bytes. A slot holding a page number is styled by `page_number` and the rest by `page_header` or `page_footer`. `--paper`, `--landscape`, `--margin`, `--header*`, and `--footer*` override these fields for one run.

The optional header/footer rules are paper decorations, not layout boxes. `rule_width` means thickness in points (CSS pixels × 0.75), not horizontal length; both span the final sheet width at the top/bottom edge of every PDF page. They are painted after the paper background and before content and page furniture, header first and footer second if they overlap. They never reserve space or change line breaks, pagination or margins. A rule thicker than its margin may extend behind content; thickness is clipped to the sheet height. Negative or non-finite widths are rejected. Zero or omitted width draws nothing; color and width cascade independently within each section. Empty strings are not colors or widths. Existing text slots (`header_left`, `footer_center`, etc.) remain under `[page]`.

PNG export produces one continuous image, so header/footer rules appear only at its top/bottom, clipped to the image height, even when rendering uses multiple tiles. Reader windows and diagnostic `--render` views have no sheet decoration.

## Diagrams

A `mermaid` fence renders as an image, and the `[mermaid]` table draws those images in the theme's own palette instead of the renderer's default light one. SVG text uses the shared generic-family map:

```toml
[svg.generic_font_family]
serif = ["serif"]
sans-serif = ["sans-serif"]
monospace = ["monospace"]
```

The keys are SVG generic names. Values are ordered literal family names or `fontdef` ids, and a renderer-generated request such as `font-family="serif"` uses the first available configured candidate. The map applies to Mermaid SVG output as well as other SVG text; omitted keys keep the bundled mapping.

```toml
[mermaid]
theme = "dark"              # default, dark, forest, neutral, or modern
font_family = ["reading"]   # fontdef ids or literal families, in priority order
background = "#202630"      # the diagram's own paper
primary_color = "#2B3441"
primary_text_color = "#DCE3ED"
line_color = "#A5B3C5"
```

`theme` names a built-in palette and decides every field the table leaves out, so a theme that only sets `background` keeps the preset's nodes, edges and text; a stylesheet with no table at all keeps the renderer's light default. Every other field is the renderer's, one for one: `font_family`, `font_size`, `background`, `text_color`, `primary_color`, `primary_text_color`, `primary_border_color`, `line_color`, `secondary_color`, `tertiary_color`, `edge_label_background`, `cluster_background`, `cluster_border`, `sequence_actor_fill`, `sequence_actor_border`, `sequence_actor_line`, `sequence_note_fill`, `sequence_note_border`, `sequence_activation_fill`, `sequence_activation_border`, `git_commit_label_color`, `git_commit_label_background`, `git_tag_label_color`, `git_tag_label_background`, `git_tag_label_border`, `pie_title_text_color`, `pie_section_text_color`, `pie_legend_text_color`, `pie_stroke_color`, `pie_outer_stroke_color`, `pie_title_text_size`, `pie_section_text_size`, `pie_legend_text_size`, `pie_stroke_width`, `pie_outer_stroke_width` and `pie_opacity`. The `git_colors`, `git_inv_colors` and `git_branch_label_colors` palettes take eight colors each and `pie_colors` takes twelve; each one replaces a whole derived palette instead of adjusting it.

`font_family` is an array in priority order, and a name that matches a `fontdef` id means that definition's families—exactly as it does in a rule's `font`—so a theme can write `font_family = ["reading", "emoji"]`. Any other name is a literal family. To see which family a diagram really used, put `一` in a label: a sans-serif face ends the stroke as a rectangle, while a serif face adds a small triangle at its right end.

A label is measured and drawn with the same faces: the theme's list draws what it covers, and a cluster it cannot draw falls back to the body text's Han faces—whichever faces its `font` candidates select for the reader's CJK convention—so a Chinese label comes out in the reader's own regional face rather than in whatever the system would fall back to. Latin labels keep the theme's own faces. Mermaid only materializes these configured candidates, the SVG generic candidates, and the selected CJK fallback candidates; it does not load every installed system face.

Colors are `#RRGGBB` or `#RRGGBBAA`, as everywhere else in a stylesheet. Sizes are finite and positive, and `pie_opacity` runs from 0 to 1. Changing this table redraws every diagram: the source is parsed once and kept, so only its layout and drawing run again. The reader uses the selected theme's table, and an export uses the exporting stylesheet's. Diagrams are measured and drawn with the same faces the reader's own text is shaped with, so a definition satisfied by `--fonts` or a downloaded file works in a diagram too. The renderer and the rasterizer resolve faces through one policy: the theme's list first, then the body text's selected Han faces for a cluster the list cannot draw. Mermaid does not materialize other installed faces. `--ignore-system-fonts` applies to diagrams as well, so an export with pinned fonts pins its diagrams.

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

Bundled reader and PDF themes prefer upright **LXGW WenKai** for Simplified Chinese `em` (`--cjk-type SC`), while Latin keeps a real italic. The SC-only `regularscript[cjk]` definition tries LXGW WenKai, KaiTi, STKaiti and Kaiti SC in order before the synthetic CJK fallback, which still handles missing fonts or glyphs; TC and JP keep their existing italic fallback. Download WenKai from the Fonts page or with `markview fonts download lxgw-wenkai`. Its full Light, Regular and Medium TTFs are available from GitHub releases, SourceForge, and archlinuxcn packages (European origin and TUNA mirror in China), pinned to v1.522. The architecture-independent archlinuxcn package is fetched from the `x86_64` directory on every platform; both sources pin the same SHA-256 and extract only the three font files. These files exceed jsDelivr’s 20 MB limit; CTAN’s GB Lite variant and Debian’s older package are not interchangeable mirrors of this release.

The flag applies only to `variant = "italic"` or `"oblique"`; a real italic or oblique face is still preferred when one exists. CJK variants may be defined with `type = "SC"`, `"TC"`, or `"JP"`. A user may override a definition with `[[fontdef-override]]`. A `fontdef` says what a short name means; it never names a file, and downloadable families live in their own table, described below.

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

## Downloadable fonts

`[[font-family]]` describes one concrete family and how to obtain it. Downloading is nothing more than another `--fonts` directory: the faces a rule can reach are the ones the font files themselves declare, so a family id is only a bookkeeping name for the reader and the command line, never a font name.

```toml
[[font-family]]
id = "noto-sans-cjk-sc"
lookfor = ["Noto Sans SC", "Noto Sans CJK SC", "Source Han Sans SC"]
description = "Simplified Chinese sans-serif, subset OTF"
license = "OFL-1.1"
license_url = "https://scripts.sil.org/OFL"
homepage = "https://github.com/notofonts/noto-cjk"
```

`lookfor` lists the names the family may report for itself. When any of them is already available—installed on the machine, in a `--fonts` directory, or in the download directory—the family is skipped. A file that holds several faces, such as a TTC or OTC collection, counts for every family it declares. A color emoji face counts too, whether it keeps outlines or the `CBDT` bitmap strikes `Noto Color Emoji` uses. `description`, `license` (an SPDX identifier), `license_url` and `homepage` are optional, and are what the reader shows in its list.

A family is obtained from one or more *sources*, which are mirrors of one another: the reader measures each distinct host once and tries them from the fastest to the slowest, with the declared order breaking ties, and the first that succeeds whole is the one used, so they may be laid out differently and even use different container formats.

```toml
[[font-family.source]]
name = "GitHub release"
[[font-family.source.archives]]
url = "https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/18_NotoSansSC.zip"
members = [
	"NotoSansSC-Thin.otf",
	"NotoSansSC-Light.otf",
	"NotoSansSC-DemiLight.otf",
	"NotoSansSC-Regular.otf",
	"NotoSansSC-Medium.otf",
	"NotoSansSC-Bold.otf",
	"NotoSansSC-Black.otf",
]

[[font-family.source]]
name = "jsDelivr"
files = [
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Thin.otf",
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Light.otf",
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-DemiLight.otf",
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf",
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Medium.otf",
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Bold.otf",
	"https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Black.otf",
]
```

`files` downloads each entry as it stands; an entry is a bare URL or a table with a `sha256`. `archives` downloads one container and extracts the members matching its patterns. The container is recognized from its own leading bytes—zip, tar, gzip and zstd—so one mirror may publish a zip while another publishes a tarball without either saying which. A pattern matches `/`-separated member paths: `*` stops at a separator, `**` crosses one, and `?` matches exactly one character. A source must yield at least one member, and every file it yields must parse as a font. Directories, symlinks and hard links are never taken, nothing is written outside the download directory, and one archive may not unpack more than 2 GiB or 4096 members.

A body is verified before it is stored: a font file is capped at 64 MiB, an archive at 2 GiB, a declared `sha256` must match, and the file is renamed into place so a partial transfer is never registered. A digest that does not match fails its source, and the next mirror is tried. A failed source's own files are removed before the next mirror runs, so a mirror never leaves half a family behind. The download directory is reported as large past 1 GiB, but a download is never refused for it.

The stored name is the file's own name at its origin — a URL's last segment, or an archive member's whole path — plus a short hash of the family id and that name. Two mirrors of one family therefore agree on where a file lands, two families never collide, and two members of one archive that share a basename but not a path stay apart. Extensionless members take the extension their outlines imply.

Nothing replaces an installed file until the whole source has been downloaded and verified, so a mirror that fails half way leaves the copies it would have replaced exactly where they were.

Files land in a `fonts/` directory beside `settings.toml`:

| Platform | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/fonts/` or `~/.config/markview/fonts/` |
| macOS | `~/Library/Application Support/markview/fonts/` |
| Windows | `%APPDATA%/markview/fonts/` |

The reader's **Fonts** page (**Ctrl+,**, then the Fonts tab) lists every family the builtin recommendations and the catalogued stylesheets declare: its name, description, license, size, the stylesheets that declare it, and whether it is in the system, downloaded, or missing. The status filters narrow the list to **All**, **Missing**, **Downloaded** or **In System**; a family downloads, redownloads or downloads a copy on its own, **Download Missing** fetches every shown family that is missing, **Download All** also fetches a stored copy of families the system already provides, and a running family can be cancelled by itself. **Open fonts folder** opens the directory.

`markview fonts` does the same from a shell:

```sh
markview fonts list                 # what still needs downloading
markview fonts list --all           # every declared family
markview fonts download             # everything missing
markview fonts download noto-sans-cjk-sc
markview fonts download --style paper --dry-run
markview fonts path                 # print the download directory
markview fonts verify               # check the directory against the declarations
```

`list`, `download` and `verify` take `--style ID` to work from one installed stylesheet, or `--file SHEET.mvss.toml` to work from a draft without installing it; either narrows the catalogue to the families that sheet itself declares, while naming nothing includes the builtin recommendations. `download` fetches only what nothing provides yet, `--force` re-downloads what is already there, `--dry-run` reports without fetching, and `--jobs N` (4 by default) bounds the transfers. `--offline` refuses the transfer while leaving `list`, `verify` and `--dry-run` working.

Reading a document, installing a stylesheet and `ss validate` never fetch anything; only the Fonts page and `markview fonts download` do. A downloaded font is a personal resource like the fonts installed on the machine: the reader and every export that draws from it — its own Export panel and the `pdf`, `render` and `smoke-test` subcommands — use it by default, so an export matches what the reader shows. A run that asks for reproducible output never sees it: `--ignore-system-fonts` excludes it in the window and in the subcommands alike, and the `bench` and `latency` subcommands never load it. `--offline` refuses the download and says so.

### Recommended Noto families

The bundled `builtin` stylesheet already declares Noto Serif, Noto Sans, Noto Sans Mono, Noto Serif CJK SC, Noto Sans CJK SC and LXGW WenKai, so they need no stylesheet of your own: open the Fonts page, or run `markview fonts download`. The Noto families ask for each static weight their mirrors publish—the nine Latin weights from Thin to Black, with the italics a family has, and the seven weights each Chinese subset carries—so a rule that names 300 or 600 finds a real face instead of the nearest one. Each Latin file is about 0.5 MiB and each Simplified Chinese subset OTF 8 to 12 MiB; the CTAN mirror serves the full CJK collection, about 16 to 25 MiB per face. The GitHub source is the official release archive, from which only the wanted members are extracted; jsDelivr serves the same faces as single files.

A full Noto CJK collection, rather than the subset faces, is **tens of MiB per file**; choose it only when the subset does not cover the text. Noto is licensed under the SIL Open Font License 1.1; the license ships with the upstream repository and is not bundled here. The reader never bundles font binaries, and the user trusts the URLs a stylesheet names.

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

`align` affects image-only paragraphs. Images mixed with text remain inline and never create text wrapping on their sides. A single image paragraph may show a caption, using `title_or_alt`, `title`, `alt`, or `none`; multiple-image and mixed paragraphs do not show captions. `["img", "placeholder"]` styles loading and error text. A `mermaid` fence becomes an image with an empty `title` and `alt`, so `img` rules style the diagram and no caption appears by default; the [`[mermaid]` table](#diagrams) colors the diagram itself.

## Live updates and safe authoring

Markview watches installed styles and settings. A valid save applies automatically; an invalid stylesheet leaves the previous effective style active. Color-only changes can repaint cached layout, while font and geometry changes reflow it. A diagram theme change is the exception among color changes: it redraws every diagram from its cached parse, without reflowing the text.

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
cargo run -- render examples/themes.md --style my-theme --output /tmp/my-theme.png
```

Review the same content in every theme at narrow and wide reading measures, including Latin/CJK, italic/bold, code, math, nested lists, tables, captions and unavailable images. In the window also check hover, selection, keyboard focus, settings/export panels and scrolling. A static document render does not exercise those interactive states.

For a repository-bundled theme, add the file to `Stylesheet::named_rules` and its ID to `Stylesheet::READER_THEMES` or `Stylesheet::PDF_THEMES` according to its destination in `crates/markview-core/src/style.rs`; discovery and reserved-ID checks use that registry. Run `cargo fmt --all` and `cargo test --workspace`, then render the fixture. A new theme usually needs no parser or renderer changes. Extend MVSS only for a concrete visual requirement that existing fields cannot express, with parser and rendering tests plus documentation.
