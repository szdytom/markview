# Stylesheet guide

Markview stylesheets are UTF-8 TOML files with the `.mvss.toml` suffix. They define portable visual themes. Personal reading preferences—font size, column width, alignment, hyphenation, and first-line paragraph indent—belong in `settings.toml`, not in a stylesheet.

## Install and select a style

```sh
markview ss install paper.mvss.toml
markview document.md --style paper
markview document.md --style paper --style dark
```

Installation validates the complete file and copies it to the user stylesheet directory. It does not install fonts or enable the style. Use `--force` to replace an installed style whose `version` is equal to or lower than the incoming version.

The directory is next to `settings.toml`:

| Platform | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/styles/` or `~/.config/markview/styles/` |
| macOS | `~/Library/Application Support/markview/styles/` |
| Windows | `%APPDATA%/markview/styles/` |

The filename without `.mvss.toml` is the style ID. Only the first directory level is scanned. The built-in `light` and `dark` IDs are reserved.

In the Settings panel, **Styles…** lets you enable, disable, and reorder styles. The leftmost selected style has the highest priority. `--style` replaces the session's selected list and is not saved. It cannot be combined with `--light` or `--dark`.

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
font = [{ family = "serif" }]
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

Text conditions accept `color`, `font`, `weight`, `size`, `decoration`, and `background`. Block conditions additionally accept `line_height`, `space_before`, `space_after`, and the container fields `padding`, `border_color`, `border_width`, and `radius`. Parts that are not containers—`label`, `marker`, `task_marker`, `caption`, and `placeholder`—reject container geometry. `indent` styles `list` and `enum`; `align` places an image and also positions a `marker` or `task_marker` in its column; `shape` picks a bullet's graphic; `source` belongs to image conditions; `show` belongs to `error`.

A list marker reserves a column before its item text. `marker` covers bullets and numbers, `task_marker` covers checkboxes, and each takes `align = "left"`, `"center"`, or `"right"` to place the marker in that column; the bundled styles center it. A bullet is drawn rather than typed—`shape` is `disc`, `square`, `triangle`, or `diamond`—so bullets and checkboxes are never part of copied text, while ordered numbers stay text.

```toml
[[rule]]
when = ["marker"]
align = "center"
shape = "disc"
```

`page` accepts only `background`. The furniture conditions accept the text fields, so a page number can be smaller or greyer than the header text beside it.

Special properties include `theme` on `["code_block"]` alone, scrollbar colors and thicknesses on `["scrollbar"]`, `muted`/`accent`/`error`/`shadow`/`scrim` on `["ui"]`, and `hover_background`/`active_background`/`disabled_color`/`focus_color` on `["ui", "button"]`. The UI theme controls appearance, not widget layout or dimensions.

Colors are sRGB `#RRGGBB` or `#RRGGBBAA`; `body.background` must be opaque. Sizes and spacing are positive or non-negative finite values. `size` is relative to the reader's base size, `line_height` is a multiple of the condition's size, and spacing/padding use base-size units. Border width and radius use logical pixels. Unknown conditions, fields, types, and enum values are errors.

## Paper

The PDF export always starts from the bundled `print` stylesheet, and `--style` layers a named style on top of it. A style may also set the `[page]` table, which is the only table besides `fontdef`, `meta`, and `rule`:

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

Stylesheets are merged from left to right by condition set. A field omitted by a higher-priority style remains from the lower-priority style; arrays replace the entire lower-priority array.

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
