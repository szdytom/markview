# Structured reading

Markview is not only prose. Tables keep their alignment, task lists stay
readable, fenced code is highlighted, and everything that can be clicked is.

| Feature | Syntax | Reading behaviour |
|:--|:--:|--:|
| Emphasis | `**bold**` and `*italic*` | Native text |
| Mathematics | `$\sum_{i=1}^{n} i$` | Shared baseline |
| Tables | GFM | Column alignment |
| Footnotes | `[^1]` | Click to jump |
| Images | PNG, JPEG, GIF, WebP, SVG | Inline or centred |

- Native window and GPU drawing
- Paragraph optimisation and English hyphenation
- Inline and display mathematics
- Tables, footnotes, alerts and task lists

```rust
fn read_document(path: &Path) -> Result<Document> {
    let source = std::fs::read_to_string(path)?;
    Ok(parse(&source))
}
```

> [!NOTE]
> Edit the file in your own editor. Markview watches it and repaints in place,
> keeping your position unless you were already at the end.
