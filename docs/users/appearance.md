# Use themes

Open **Settings → Styles** to enable, disable and reorder reader themes. The
leftmost selection has highest priority. Export selects paper themes separately.
Browse the [theme gallery](themes.md) before choosing a starting point.

Use [reading settings](settings.md) for size, measure and paragraph preferences,
and [Fonts](fonts.md) to select families. An MVSS theme controls visual roles,
colors, spacing and decorations.

## Install and select a style

```sh
markview ss list
markview ss validate paper.mvss.toml
markview ss install paper.mvss.toml
markview document.md --style paper
markview document.md --style paper --style dark
```

Installation validates the complete file and copies it to the user stylesheet directory. It does not install fonts or enable the style. Use `--force` to allow replacing an installed style with an equal or older incoming version. `ss validate` runs the same parse without copying anything, so a draft can be checked in place before it is installed; it reports the sheet's version and rule count on success.

The directory is next to `settings.toml`:

| Platform | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/styles/` or `~/.config/markview/styles/` |
| macOS | `~/Library/Application Support/markview/styles/` |
| Windows | `%APPDATA%/markview/styles/` |

The filename without `.mvss.toml` is the style ID. Only the first directory level is scanned. The bundled IDs `light`, `dark`, `celadon`, `blueprint`, `rosewood`, `8-bit`, `print`, `monochrome`, `qibaishi`, `vangogh`, `mondrian`, and `builtin` are reserved (including case variants).

In the Settings panel's **Styles** tab you can enable, disable, and reorder styles. The leftmost selected style has the highest priority. `--style` replaces the session's selected list and is not saved. It cannot be combined with `--light` or `--dark`.



## When a style does not apply

Installing does not enable a style or download its fonts. Select it in the correct
reader/export destination, then use Fonts for any missing family. A theme marked
only for `pdf` cannot style the reader; a `ui` theme cannot serve as a paper theme.

Installed styles hot-reload after a valid save. An invalid edit leaves the previous
appearance active; fix the error and validate again. Command-line `--style`
selection applies to the session and is not saved.

To create or modify a theme, follow the [MVSS reference and workflow](stylesheets.md).
