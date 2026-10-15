# 设置

[终端用户文档](README.md) · [文档导航](../README.md) · [English](settings.md)

打开 **设置** 用 `Ctrl+,`（macOS 上为 Command）。阅读偏好写在
`settings.toml`；视觉主题写在 [样式表](stylesheets.md)。

完整的设置说明见英文版 [Settings](settings.md)。本节说明 **Markdown 方言**。

## Markdown dialect

**设置 → 通用 → 语言与代码** 决定 Markview 如何解析 Markdown。
方言预设旁的信息按钮会打开本页。

- **GFM（GitHub 风格 Markdown）。** 软换行仍为空格。在此预设下可开关「单个换行视为硬换行」与 `==高亮==`。详见
  [GFM 规范](https://github.github.com/gfm/)。
- **OFM lite（类 Obsidian，仅高亮）。** 强制开启高亮；软换行仍为空格。硬换行与高亮开关会保留在界面中但呈禁用（灰色）：OFM lite 强制高亮开启、硬换行关闭。这**不是**完整的
  [Obsidian Flavored Markdown](https://help.obsidian.md/obsidian-flavored-markdown)：
  Markview 不支持双向链接（wikilinks）及其他仅 OFM 的特性。
