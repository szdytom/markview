# Upgrade to MVSS 3

Change `format_version = 2` to `format_version = 3` and rename every existing `preview` condition to `tab_preview` (for example, `when = ["ui", "preview"]` becomes `when = ["ui", "tab_preview"]`). Keep the theme's own `version` revision unchanged unless you are publishing an updated theme.

In format 3, `preview` is reserved for reflowed hover popups, such as footnotes. Use `["preview"]` for their surface and `["preview", "p"]` for their paragraph styles. Tab page thumbnails retain their original layout and do not match these rules. Other format 2 declarations remain unchanged.

Markview rejects earlier format versions rather than interpreting an old tab-card rule as a footnote-popup rule. See the [stylesheet reference](stylesheets.md) for examples of the new selectors.
