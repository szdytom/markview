# Markview4vsc

Preview Markdown and export to **PDF** or **PNG** from Visual Studio Code, with native
typography powered by [Markview](https://github.com/szdytom/markview).
Choose a ready-made template or create your own look with an MVSS stylesheet.

The native engine is included in the extension. No separate Markview, Chromium,
Rust, or TeX installation is required.

## Contents

- [Features](#features)
- [Usage](#usage)
- [Extension settings](#extension-settings)
- [Templates](#templates)
- [FAQ](#faq)
- [Requirements](#requirements)
- [Links and license](#links-and-license)

## Features

| Feature | What you can do |
| --- | --- |
| PDF export | Create a paginated document with selectable text. |
| PNG export | Export the whole document as one continuous image. |
| Native typesetting | Use the same Markdown layout engine as the Markview reader, without browser-based printing. |
| Built-in templates | Choose Print, Monochrome, Qi Baishi, Van Gogh, or Mondrian. |
| Custom templates | Load a local `.mvss.toml` file without installing it. |
| Current-buffer export | Include unsaved edits in the exported document. |
| Per-document defaults | Set a template through user, workspace, folder, or Markdown-specific settings. |
| Editor integration | Export from the command palette or the Markdown editor's right-click menu. |

## Usage

### Live preview

Click the **M document icon** in the Markdown editor title, or run **Markview4vsc: Open Preview to the Side**. The preview follows the active Markdown file and unsaved edits, synchronizes scrolling, and fits narrow panes without changing template layout. It restores after Reload Window.

### Command palette

Open a Markdown document, press **Cmd+Shift+P** on macOS, and choose a command:

| Command | Behavior |
| --- | --- |
| **Markview4vsc: Export to PDF** | Use the configured template and choose where to save the PDF. |
| **Markview4vsc: Export to PNG** | Use the configured template and choose where to save the PNG. |
| **Markview4vsc: Export with Template…** | Choose a template, choose PDF or PNG, then choose where to save. |

The exported file is revealed in your system file manager.

### Right-click menu

Right-click **inside the open Markdown editor**, then choose **Markview4vsc: Export to PDF** or **Markview4vsc: Export to PNG**. These commands use
your default template and go directly to the save dialog.

The menu entries are in the editor, not the Explorer or editor tab menu.

## Extension settings

Preview and export share `markview.template`. Existing `markviewExport.template` settings and command keybindings remain supported; an explicit `markview.template` takes precedence, including an empty value. This extension updates **Better markdown PDF** in place. Uninstall the separate development **Markview Preview** extension to avoid duplicate commands.


Open **Preferences: Open Workspace Settings (JSON)** and set your default template:

```json
{
  "markview.template": "mondrian"
}
```

| Setting | Values | Default |
| --- | --- | --- |
| `markview.template` | A built-in template ID or a path ending in `.mvss.toml` | `""` (Print) |

To use your own template next to the Markdown file:

```json
{
  "markview.template": "./report.mvss.toml"
}
```

Absolute paths are also supported. Relative template paths resolve from the
**Markdown file's folder**, not from the folder containing `settings.json`.
For an untitled document, they resolve from its workspace folder; without a
workspace folder, use an absolute template path.

The setting respects workspace-folder and `[markdown]` overrides. For example:

```json
{
  "[markdown]": {
    "markview.template": "monochrome"
  }
}
```

## Templates

### Built-in templates

| ID | Appearance |
| --- | --- |
| `print` | Neutral paper layout with restrained accents. |
| `monochrome` | Minimal black-and-white styling. |
| `qibaishi` | Ink tones with vermilion details. |
| `vangogh` | Indigo headings and wheat-gold accents. |
| `mondrian` | Primary colors, strong headings, and black table grids. |

Use **Export with Template…** to choose a template for one export. Selecting
**None** uses the default Print style and bypasses your configured template.

### Write your own template

Templates use **MVSS**, Markview's TOML-based stylesheet format. They describe
page setup, colors, fonts, headings, spacing, code blocks, tables, and other
Markdown elements. MVSS is not CSS; existing CSS themes must be translated.

See the **[MVSS stylesheet guide](https://github.com/szdytom/markview/blob/main/docs/stylesheets.md)**
for the complete format, supported properties, and examples. Useful sections:

- [Rules and conditions](https://github.com/szdytom/markview/blob/main/docs/stylesheets.md#rules-and-conditions): target headings, paragraphs, links, and other elements.
- [Fields](https://github.com/szdytom/markview/blob/main/docs/stylesheets.md#fields): colors, fonts, spacing, borders, and related properties.
- [Paper](https://github.com/szdytom/markview/blob/main/docs/stylesheets.md#paper): page size, margins, and PDF headers and footers.
- [Bundled template sources](https://github.com/szdytom/markview/tree/main/crates/markview-core/styles): copy a paper theme and adapt it.

Start by saving this as `report.mvss.toml`:

```toml
format_version = 2
version = 1
targets = ["pdf"]

[meta]
name = "My Report"
description = "Blue headings on A4 paper"

[page]
size = "a4"
margin = [22, 20, 22, 20]

[[rule]]
when = ["h1"]
color = "#244C80"

[[rule]]
when = ["link"]
color = "#315D86"
decoration = ["underline"]
```

- `format_version = 2` selects the MVSS format; `version` is your template's revision.
- `targets = ["pdf"]` is the paper destination used by **both PDF and PNG exports**.
- `[page]` sets paper geometry. Margins are in millimetres, ordered top, right, bottom, left.
- Each `[[rule]]` selects an element through `when` and changes only the properties you specify.

Your rules are layered over the built-in Print stylesheet, so you can start
with just a few changes. To try the file, run **Export with Template…** and select
**Choose a template file…**, or set `markview.template` to its path.
No template installation or separate Markview download is needed. The guide's
reader installation commands are optional and are not part of this workflow.

## FAQ

**Do I need to save my Markdown first?**

No. Exports include the current buffer, including unsaved edits. The save dialog
suggests the source document's name with a `.pdf` or `.png` extension.

**How are relative images resolved?**

Relative document resources resolve beside the Markdown file. For an untitled
document, the workspace folder is used, or the chosen output folder when no
workspace is open. Remote images may be downloaded during export.

**Can I use my standalone Markview settings or installed templates?**

The extension keeps its engine storage separate. Select your `.mvss.toml` file
explicitly or configure its path; the reader's personal settings are not imported.

**Is there a preview or automatic export on save?**

Live preview and manual export are included. There is no export-on-save option.

## Requirements

- Local desktop VS Code in a trusted workspace, with the VSIX matching your OS and CPU.
- A compatible GPU for preview and PNG export.
- Browser editors, remote documents, Remote-SSH, and dev containers are outside
  the current supported scope.

The engine starts on first use and exits with its VS Code window.

## Links and license

- [Markview source and issue tracker](https://github.com/szdytom/markview)
- [MVSS stylesheet guide](https://github.com/szdytom/markview/blob/main/docs/stylesheets.md)
- [Changelog](CHANGELOG.md)

Licensed under MIT. The extension bundles the Markview native engine;
third-party notices are included in the package.
