# Fonts

[User documentation](README.md) · [Documentation](../README.md)

Markview uses the fonts already installed on your machine. Stylesheets can
declare additional downloadable families with `[[font-family]]`; the
[stylesheet reference](stylesheets.md#downloadable-fonts) defines that format.
Downloads are always explicit.

## Download and select

Files land in a `fonts/` directory beside `settings.toml`:

| Platform | Directory |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/markview/fonts/` or `~/.config/markview/fonts/` |
| macOS | `~/Library/Application Support/markview/fonts/` |
| Windows | `%APPDATA%/markview/fonts/` |

The reader's **Fonts** page (**Ctrl+,**, then the Fonts tab) lists every family the builtin recommendations and the catalogued stylesheets declare: its name, description, license, size, the stylesheets that declare it, and whether it is in the system, downloaded, or missing. The filter row narrows the list to **All**, **Missing**, **Downloaded** or **In System**; a family downloads, redownloads or downloads a copy on its own, **Download Missing** fetches every shown family that is missing, **Download All** also fetches a stored copy of families the system already provides, and a running family can be cancelled by itself. **Open fonts folder** opens the directory.

That page puts a family on disk; it does not choose which family a document is set in. Choosing is the same page's own job, one step beside: the filter row ends in **Set fonts**, which sets the catalogue aside and shows one chooser row per role — `serif`, `sans-serif`, `monospace` and the same three for Han text — listing the families the machine has with a **Default** entry first, where the default is the stylesheet's own candidate chain. A pick is stored as a `[[fontdef-override]]` for that role and reflows the document at once, and the default entry takes it back out. The three Han rows appear only while a CJK variant is in force, since without one the sheet resolves no `[cjk]` definition for a pick to shape.

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

Reading a document, installing a stylesheet and `ss validate` never fetch anything; only the Fonts page and `markview fonts download` do. A downloaded font is a personal resource like the fonts installed on the machine: the reader and every export that draws from it — its own Export panel and the `pdf`, `export`, `render` and `smoke-test` subcommands — use it by default, so an export matches what the reader shows. A run that asks for reproducible output never sees it: `--ignore-system-fonts` excludes it in the window and in the subcommands alike, and the `bench` and `latency` subcommands never load it. `--offline` refuses the download and says so.

## Recommended Noto families

The bundled `builtin` stylesheet already declares Noto Serif, Noto Sans, Noto Sans Mono, Noto Serif CJK SC, Noto Sans CJK SC and LXGW WenKai, so they need no stylesheet of your own: open the Fonts page, or run `markview fonts download`. The Noto families ask for each static weight their mirrors publish—the nine Latin weights from Thin to Black, with the italics a family has, and the seven weights each Chinese subset carries—so a rule that names 300 or 600 finds a real face instead of the nearest one. Each Latin file is about 0.5 MiB and each Simplified Chinese subset OTF 8 to 12 MiB; the CTAN mirror serves the full CJK collection, about 16 to 25 MiB per face. The GitHub source is the official release archive, from which only the wanted members are extracted; jsDelivr serves the same faces as single files.

A full Noto CJK collection, rather than the subset faces, is **tens of MiB per file**; choose it only when the subset does not cover the text. Noto is licensed under the SIL Open Font License 1.1; the license ships with the upstream repository and is not bundled here. The reader never bundles font binaries, and the user trusts the URLs a stylesheet names.

The Han font choosers list only families whose character maps cover Han text,
so a Latin-only face cannot be selected for those roles.

## Editor integrations

Editors need the effective template's font requirements, including families that
have no download source. `markview fonts catalog --destination ui|pdf` reports
those requirements as JSON, with template choices, family names, licenses and
source metadata. Use `--style ID` or `--file FILE.mvss.toml` to select a template;
custom files layer over the destination defaults and any selected named style.
This is distinct from `fonts list`, which reports declared downloadable families
and their local availability.

The existing download command also supports editor-managed storage and progress:

```sh
markview fonts download noto-serif --json --directory ./font-cache
markview fonts download --file custom.mvss.toml --destination pdf --json --directory ./font-cache
```

`--destination ui|pdf` downloads from the effective template instead of only the
selected file's declarations. Existing family selection, `--force`, `--dry-run`
and `--jobs` still apply. JSON mode emits `progress` followed by `done`, or a
`selection` for a dry run. Failures return a nonzero exit status with details on
stderr. A host may create the path given by `--cancel-file` to cancel a transfer.
Without these options, terminal output and the user font directory are unchanged.

Hosts should download to a staging directory and promote it after success, keeping
their existing cache on failure or cancellation. The VS Code client lives in
`web/app/vsc-ext/` and uses these same font commands.
