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

A block's own box is the exception. Its background, border, padding, spacing, and size come only from rules that name the element or a specialization of it, so `["p"]` styles a paragraph in any context and `["blockquote", "p"]` styles a paragraph in a quote, while a container's `["blockquote"] background` never paints its children. Inline runs keep the same split: a background must come from a rule that names inline markup, so `["code"]` and `["blockquote", "code"]` paint a code chip but `["blockquote"]` does not.

## Fields

Text conditions accept `color`, `font`, `weight`, `size`, `decoration`, and `background`. Block conditions additionally accept `line_height`, `space_before`, `space_after`, and the container fields `padding`, `border_color`, `border_width`, and `radius`. Parts that are not containers—`label`, `marker`, `task_marker`, `caption`, and `placeholder`—reject container geometry. `indent` styles `list` and `enum`; `align` and `source` belong to image conditions; `show` belongs to `error`.

Special properties include `theme` on `["code_block"]` alone, scrollbar colors and thicknesses on `["scrollbar"]`, `muted`/`accent`/`error`/`shadow`/`scrim` on `["ui"]`, and `hover_background`/`active_background`/`disabled_color`/`focus_color` on `["ui", "button"]`. The UI theme controls appearance, not widget layout or dimensions.

Colors are sRGB `#RRGGBB` or `#RRGGBBAA`; `body.background` must be opaque. Sizes and spacing are positive or non-negative finite values. `size` is relative to the reader's base size, `line_height` is a multiple of the condition's size, and spacing/padding use base-size units. Border width and radius use logical pixels. Unknown conditions, fields, types, and enum values are errors.

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

Use `variant = "normal"`, `"italic"`, or `"oblique"`, and an optional weight from 1 to 1000. Markview skips a candidate when the face, requested style, or complete grapheme cluster is unavailable; it does not synthesize slant or weight. CJK variants may be defined with `type = "SC"`, `"TC"`, or `"JP"`. A user may override a definition with `[[fontdef-override]]`, but stylesheet files cannot bundle font files or download them.

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
