# 结构化阅读

Markview 不只处理正文。表格保留对齐方式，任务列表依然易读，代码块带语法高亮，
一切可以点击的东西都真的可以点。

| 内容 | 写法 | 阅读时的表现 |
|:--|:--:|--:|
| 强调 | `**粗体**` 与 `*斜体*` | 原生文字 |
| 数学 | `$\sum_{i=1}^{n} i$` | 共享基线 |
| 表格 | GFM | 列对齐 |
| 脚注 | `[^1]` | 点击跳转 |
| 图片 | PNG、JPEG、GIF、WebP、SVG | 行内或居中 |

- 原生窗口与 GPU 绘制
- 整段优化与英文断字
- 行内与行间数学公式
- 表格、脚注、提示块与任务列表

```rust
fn read_document(path: &Path) -> Result<Document> {
    let source = std::fs::read_to_string(path)?;
    Ok(parse(&source))
}
```

> [!NOTE]
> 在外部编辑器里修改文件即可。Markview 会监视文件并原地重绘；除非你已经滚到底部，
> 否则阅读位置保持不变。
