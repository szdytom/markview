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
| 标注框 `> [!type] 标题`：内置类型与别名、可嵌套、`+`/`-` 折叠 | Obsidian 嵌入 `![[笔记]]` 及完整笔记库语义 |
| `==高亮==`（强制开启） | `#标签` 与块引用 `^id` |
| 内联 HTML：`sub`、`mark` 有样式，`u`、`ins`、`small` 按纯文本阅读；另有通用的 `sup`、`b`、`i`、`code` 等 | 其他仅 OFM 的语法 |
| 隐藏 `%%行内注释%%` 与内容行首 `%%` 块注释 | 标注框标题格式（标题仍为纯文本） |

基础阅读器已在两种预设中支持 `[[笔记|标签]]` 等 wikilinks 及标题目标，打开文件夹后也可解析相对笔记链接。

注释不进入阅读、选择复制、搜索和阅读器导出。同一内容行内成对的标记隐藏行内文字；
内容行首（包括列表、引用内）的 `%%` 可跨空行隐藏到下一个未转义的 `%%`，未闭合时隐藏到文件末尾。
行中未配对标记保留原样；`word%%note%%word` 显示为 `wordword`。
代码、独立解析的数学内容、支持的 HTML code/pre 元素及 front matter 元数据保持原样；
`\%%` 转义标记。GFM 保留注释标记。

与 Obsidian 一致，原始 HTML 块和 `<summary>` 内容保留 `%%` 标记。
用空行与 HTML 块分隔的 Markdown 段落仍会隐藏注释；Markdown 段落中的内联 HTML 标签不会阻止注释解析。

标注框标题为纯文本；被转义、代码包裹或加样式的 `[!type]` 标记仍按普通引用阅读。
标注框识别、类型规范化与默认标题直接遵循 Comrak 上游的 OFM 扩展。

```markdown
> [!warning]- 默认折叠
> 正文含 ==高亮== 与 <sub>下标</sub>。
> > [!note] 标注框可以嵌套。
```

导出跟随阅读器：应用内 PDF/PNG 导出按所选方言解析并展开标注框正文；命令行 `markview export` 使用默认 GFM 方言，不读取阅读器的方言设置。
