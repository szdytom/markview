# 设置

[终端用户文档](README.md) · [文档导航](../README.md) · [English](settings.md)

打开 **设置** 用 `Ctrl+,`（macOS 上为 Command）。阅读偏好写在
`settings.toml`；视觉主题写在 [样式表](stylesheets.md)。

完整的设置说明见英文版 [Settings](settings.md)。本节说明 **Markdown 方言**。

## Markdown dialect

**设置 → 通用 → 语言与代码** 决定 Markview 如何解析 Markdown。
方言预设旁的信息按钮会打开本页。

- **GFM（GitHub 风格 Markdown，默认）。** 软换行仍为空格。在此预设下可分别开关「单个换行视为硬换行」与 `==高亮==`。详见
  [GFM 规范](https://github.github.com/gfm/)。
- **OFM lite（类 Obsidian，有限子集）。** 对
  [Obsidian Flavored Markdown](https://help.obsidian.md/obsidian-flavored-markdown)
  的受限阅读：强制开启高亮、软换行仍为空格，因此两个开关会保留在界面中但呈禁用（灰色）。

### OFM lite 支持的范围

| 支持 | 不支持 |
| --- | --- |
| 标注框 `> [!type] 标题`：内置类型与别名、可嵌套、`+`/`-` 折叠 | Wiki 风格内部链接 `[[笔记]]`、嵌入 `![[笔记]]` 及笔记库功能 |
| `==高亮==`（强制开启） | `#标签` 与块引用 `^id` |
| 内联 HTML：`sub`、`mark` 有样式，`u`、`ins`、`small` 按纯文本阅读；另有通用的 `sup`、`b`、`i`、`code` 等 | `%%` 注释及其他仅 OFM 的语法 |

标注框标题为纯文本；被转义、代码包裹或加样式的 `[!type]` 标记仍按普通引用阅读。

```markdown
> [!warning]- 默认折叠
> 正文含 ==高亮== 与 <sub>下标</sub>。
> > [!note] 标注框可以嵌套。
```

导出跟随阅读器：应用内 PDF/PNG 导出按所选方言解析并展开标注框正文；命令行 `markview export` 使用默认 GFM 方言，不读取阅读器的方言设置。
