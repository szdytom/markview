# A reader's field notes

Markview · Typography study / 排版札记

A good reading surface makes room for the argument. **Strong ideas**, *quiet emphasis*, and [a useful reference](https://example.com) each need a distinct voice. 行文之间，留白与标点同样重要。**清晰的层次**与*轻微的强调*，让长文保持自然的节奏。 🌿

## Structure and texture

> The page is a place to think. A quotation should feel connected to the argument while keeping its own voice.
>
> 阅读是一种缓慢的发现：字形、行距与色彩共同构成纸面的呼吸。

- A readable hierarchy
  - A quieter nested detail
- Consistent accents and `inline_code()`
- [x] Latin, 中文 and Emoji
- [ ] Review focus, hover and selection

### Numbers worth keeping

| Material | Character | 使用场景 |
| --- | --- | --- |
| Porcelain | Light and open | 日间阅读 |
| Blueprint | Precise and structured | 技术笔记 |
| Rosewood | Quiet and intimate | 夜间长文 |

```rust
fn reading_time(words: usize) -> usize {
    // Leave a little room to think.
    words.div_ceil(240)
}
```

A compact formula: $e^{i\pi}+1=0$. A note for later.[^note]

<details>
<summary>More on this example / 展开更多</summary>

A collapsible element keeps a long aside out of the way, and its body is
ordinary Markdown: **emphasis**, `inline_code()`, and lists all work.

- first
- second

</details>

<details open>
<summary>An open element starts expanded</summary>

The reader can collapse it again. The choice survives a font-size or column
change, and a reload starts from the source again.

</details>

---

![A deliberately unavailable image](theme-preview-missing.png "Image caption / 图注")

[^note]: Small text deserves the same care as a title.
