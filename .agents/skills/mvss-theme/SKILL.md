---
name: mvss-theme
description: Create or refine Markview MVSS reader and PDF themes, including palettes, typography, bundled registration and visual validation. Use for .mvss.toml theme authoring, not general UI layout changes.
---

# MVSS theme authoring

Read [the stylesheet guide](../../../docs/stylesheets.md) for syntax, cascade and the authoring workflow. Inspect a nearby [bundled theme](../../../crates/markview-core/styles/) and `builtin.mvss.toml` before editing.

Choose a reading context and deliberate visual identity. Define paper, ink, surface, muted ink, accent and border colors; pair heading and body roles. Give the theme one recognizable typographic or structural detail. Keep long-form reading comfortable, including mixed Latin/CJK text. Respect the user's requested direction and scope.

## Repository invariants

- `builtin` is hidden and always last. Themes contain explicit differences, never a copy of all fallback rules. Reader style order is highest priority first; named themes must be merged as raw declarations.
- Declare top-level `targets = ["ui"]`, `["pdf"]`, or `["ui", "pdf"]` for the intended destinations. Omission supports both; empty arrays, duplicates and unknown names are invalid. Targets apply to each file before merging. The export panel uses `pdf` for PDF and PNG; `--render` can preview either.
- Use format version 2, a revision `version`, and useful `meta.name`/`meta.description`. Do not put personal reading width, base size or alignment preferences in the theme.
- A `when` array is an unordered conjunction, not a CSS selector. Each condition set may occur only once per file. More specific rules and later-entered conditions can override broad rules independently of file order.
- Font arrays replace inherited arrays. Prefer CJK candidates at weight 500, followed by an inherited-weight candidate for fonts without Medium; preserve Emoji at weight 400. Changing body font does not change the fallback's explicit serif italic `em` rule.
- Complete palettes cover body, inline/block code and its syntax theme, quotes, lists/tasks, tables, captions/placeholders, selection, scrollbar and all UI surfaces/states. Keep foreground/background contrast readable, especially muted labels and focus indicators.
- Register bundled themes in `Stylesheet::named_rules` and `Stylesheet::READER_THEMES` (reader) or `Stylesheet::PDF_THEMES` (paper) in `crates/markview-core/src/style.rs`. For user-installed themes choose a non-reserved filename. Do not install into the user's configuration unless requested.

For paper themes, inherit Print geometry and 0.75em page furniture, and inspect actual multi-page PDF exports with running titles and page counts. Use `theme = "none"` on `code_block` when syntax colors would violate a monochrome palette; embedded images and Emoji keep their own colors.

Validate through `markview ss validate`. Render [examples/themes.md](../../../examples/themes.md) through the real reader pipeline, inspect the image and revise. Check interactive states in a window when available; state any unverified coverage. Follow repository build-backup, changelog and test instructions. Prefer existing MVSS fields; if a required effect needs new capabilities, implement parsing and rendering together, test observable behavior and update the guide.
