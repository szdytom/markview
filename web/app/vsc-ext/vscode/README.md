# Markview Integration

Preview Markdown with Markview's native typography engine and export PDF or PNG without installing Markview separately. Previously published as **Better markdown PDF**, this extension keeps the same Marketplace identity so existing installations upgrade in place.

## Preview Markdown

Open a Markdown file and click the **M** icon at the top right of the editor, or run **Markview: 打开侧边预览**. The preview uses Markview's Rust engine compiled to WebAssembly, drawing text onto a canvas. It supports unsaved edits, selection and copy, images, links, synchronized scrolling and restored reading positions after Reload Window.

## Export PDF and PNG

Right-click a Markdown editor and choose **Markview: 导出 PDF…**, **导出 PNG…**, or **使用模板导出…**. Choose a destination; the suggested filename matches the source document. Export uses the current editor buffer, including unsaved changes, and the native engine included in the platform-specific extension.

The template command lets you select a bundled design or a custom `.mvss.toml` file, then choose PDF or PNG. No template installation is required. Export progress supports cancellation.

## Templates and settings

Choose a reading template with **Markview: 选择阅读模板…**. Reading and export can use independent defaults:

```jsonc
{
  "markview.previewTemplate": "celadon",
  "markview.exportTemplate": "vangogh",
  "markview.fontSize": 18,
  "markview.width": 760
}
```

For a custom template, set `markview.previewStyleFile` or `markview.exportStyleFile`. In a directory's `.vscode/settings.json`, paths are relative to the directory containing `.vscode`:

```jsonc
{
  "markview.previewStyleFile": "styles/reader.mvss.toml",
  "markview.exportStyleFile": "styles/paper.mvss.toml"
}
```

Settings inherit from the nearest directory with a template choice, including parent directories outside the open workspace. A child preset replaces an inherited custom file; `"default"` explicitly resets that destination. User settings should use absolute custom-file paths. Selected configuration errors are reported rather than silently ignored.

[Write an MVSS template](https://github.com/szdytom/markview/blob/main/docs/users/stylesheets.md): MVSS is a TOML stylesheet that defines font families, colors, spacing and element rules. Declare the intended destination (`ui` for reading, `pdf` for export); a template used in both places must support both. The engine resolves typography, so CSS is not used for document layout.

## Fonts

Run **Markview: 字体管理…** to inspect the fonts required by a template, add a font directory, or download missing families. Available sources and license information appear in the panel. Downloads stay in extension storage and do not install fonts system-wide. Family availability does not guarantee coverage of every character.

## Upgrading from Better markdown PDF or Markview4vsc

- The extension ID remains `Stevvven.markview-export`; you can keep existing keybindings for `markviewExport.exportPdf`, `markviewExport.exportPng`, and `markviewExport.exportWithTemplate`. The later `markview.exportPdf`, `markview.exportPng`, `markview.exportWithTemplate` and `markview.openPreview` names also remain aliases.
- `markview.template` remains a shared fallback when new settings are unset; `markviewExport.template` remains an export-only fallback, so older PDF designs do not break the new reading preview. An explicitly empty `markview.template` disables the older fallback. Legacy relative paths retain their document-relative meaning; for untitled files they use the workspace folder.
- New template settings take priority. No settings are rewritten automatically. Custom reading templates need a `ui` destination supported by the current engine.
- `markview.columnWidth` and `markview.codeblockWrap` remain fallbacks for `markview.width` and `markview.codeWrap`.

## Requirements

A local, trusted VS Code workspace is required. Choose the extension package matching your operating system and CPU: macOS, Windows or Linux, each in x64 and ARM64 variants. Linux requires glibc 2.35 or newer, fontconfig and a working Vulkan driver for native PNG export. Browser-only VS Code and remote/virtual workspaces are not supported in this release.

[Report an issue](https://github.com/szdytom/markview/issues) · [Markview documentation](https://github.com/szdytom/markview/tree/main/docs)
