# Better markdown PDF

Export Markdown to **PDF** or **PNG** with Markview's native typesetting engine.
Choose a bundled template or your own `.mvss.toml` file. The engine is included;
you do not need to install Markview, Rust, a browser, or TeX.

## Export a document

To use your default template, **right-click inside an open Markdown editor**
and choose **Better markdown PDF: Export to PDF** or **Better markdown PDF: Export to
PNG**. Only the save dialog is shown. Both commands use the current buffer,
including unsaved edits, and resolve `markviewExport.template` for that document.
These entries appear in the editor's context menu, not the Explorer or tab menu.

To choose a template for one export:

1. Open a local Markdown document in VS Code.
2. Run **Better markdown PDF: Export with Template…** from the command palette.
3. Choose a bundled template, **None** for the default print sheet, or
   **Choose a template file…** for your own MVSS file.
4. Choose **PDF** or **PNG**, then choose where to save it.

The result is revealed in your system file manager. PDF exports are paginated;
PNG exports contain the whole document in one image. Exports use the current
editor buffer, including unsaved changes. Relative document resources resolve
beside the Markdown file. For an untitled document they resolve in its workspace
folder, or beside the chosen output when no folder is open.

**Better markdown PDF: Export to PDF** and **Better markdown PDF: Export to PNG** skip the
template picker and use the `markviewExport.template` setting. Set it to a bundled
name such as `mondrian`, an absolute `.mvss.toml` path, or a path relative to the
document. Workspace-folder and `[markdown]` overrides are supported. Selecting
**None** explicitly bypasses the configured template. Custom templates do not
need to be installed.

## Scope and requirements

This extension provides export commands only, with no preview panel. It never
reads the standalone reader's settings or templates.

- VS Code 1.95 or newer, running locally in a trusted workspace.
- Install the VSIX matching your operating system and CPU. The current local
  candidate is for Apple Silicon macOS (`darwin-arm64`).
- PNG rendering needs a compatible GPU. Browser editors, remote documents,
  Remote-SSH, and dev containers are outside this release's supported scope.
- Linux runtime details are in the repository's
  [packaging guide](https://github.com/szdytom/markview/blob/main/docs/packaging.md).

The engine starts on first use and exits with the VS Code window. Remote images
referenced by the document may be downloaded by the engine for export.

## Publisher

Published under the personal publisher **Stevvven** as
`Stevvven.markview-export`. This extension shares the upstream Markview engine.

## Install a local VSIX

In VS Code, run **Extensions: Install from VSIX…** and select the platform package.
Then open a Markdown document and run an export command.

## Build from this repository

Run `npm ci` in `editors/vscode-export`, then run `./package-vsix.sh`. On Apple
Silicon macOS it packages the existing `target/release/markview` binary after
checking its architecture. Rust is needed only to rebuild that binary. Build
artifacts are written to `dist/`. Run `./run-tests.sh` to test the packaged
extension in an isolated VS Code profile.

Source and issue tracker: [szdytom/markview](https://github.com/szdytom/markview).
Licensed under MIT.
