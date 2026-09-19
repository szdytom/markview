#!/usr/bin/env python3
"""Generate reproducible, exactly 10 KiB UTF-8 benchmark inputs."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
ROOT.mkdir(parents=True, exist_ok=True)
PARAGRAPHS = [
    "中文阅读依赖合理的行宽与行距。标点应该遵守禁则，避免将右括号、逗号和句号放在行首。原生阅读器使用系统字体，也要处理 English words 与中文的混合排版。\n\n",
    "Typography balances the spaces between words throughout a paragraph. An extraordinarily complicated explanation becomes easier to follow when the reader can concentrate on its meaning. Hyphenation and optimal line breaking work together to keep the texture of the paragraph consistent.\n\n",
    "## A short section\n\nThis paragraph includes **strong emphasis**, *italic text*, `inline code`, and a [reference](https://example.com). The layout remains readable when the window changes width.\n\n",
]
for name, math in [("ordinary-10k.md", False), ("math-10k.md", True)]:
    text = "# Markview benchmark\n\n"
    if math:
        text += "公式与正文混排 $E=mc^2$、$x_1^2+x_2^2$、$\\frac{a+b}{c}$。\n\n"
        text += "$$\\int_0^1 x^2\\,dx=\\frac{1}{3}$$\n\n"
    i = 0
    while len((text + PARAGRAPHS[i % len(PARAGRAPHS)]).encode()) <= 10240:
        text += PARAGRAPHS[i % len(PARAGRAPHS)]
        i += 1
    remaining = 10240 - len(text.encode())
    # Padding is readable ASCII, with regular spaces and no unbreakable word.
    padding = ("Small words make a readable final line. " * 300)[:remaining]
    text += padding
    data = text.encode()
    assert len(data) == 10240
    (ROOT / name).write_bytes(data)


def exact_code_fixture(name: str, blocks: list[str], heading: str) -> None:
    """Write a 10 KiB fixture while keeping the final code fence complete."""
    text = f"# {heading}\n\n"
    prefix = "```text\n"
    suffix = "\n```\n"
    for block in blocks:
        if len((text + block + prefix + "x\n" + suffix).encode()) > 10240:
            break
        text += block
    remaining = 10240 - len(text.encode())
    body_length = remaining - len((prefix + suffix).encode())
    assert body_length >= 1
    body = "x" * (body_length - 1) + "\n"
    text += prefix + body + suffix
    data = text.encode()
    assert len(data) == 10240
    (ROOT / name).write_bytes(data)


small_block = """```rust
fn highlighted_{index}(value: usize) -> usize {{
    let doubled = value * 2;
    if doubled > 10 {{ doubled }} else {{ doubled + 1 }}
}}
```

"""
small_blocks = [small_block.format(index=i) for i in range(200)]
exact_code_fixture("code-10k.md", small_blocks, "Many code blocks")

long_prefix = """```rust
fn long_code_sample() {
"""
long_suffix = """}
```
"""
long_body_length = 10240 - len(("# Long code block\n\n" + long_prefix + long_suffix).encode())
long_lines = []
while True:
    index = len(long_lines)
    line = f'    let value_{index:04} = {index}; println!("{{}}", value_{index:04});\n'
    if sum(len(item.encode()) for item in long_lines) + len(line.encode()) + len("// filler\n".encode()) > long_body_length:
        break
    long_lines.append(line)
long_body = "".join(long_lines)
remaining = long_body_length - len(long_body.encode())
long_body += "// " + "x" * (remaining - len("// \n".encode())) + "\n"
long_text = "# Long code block\n\n" + long_prefix + long_body + long_suffix
long_data = long_text.encode()
assert len(long_data) == 10240
(ROOT / "long-code-10k.md").write_bytes(long_data)


def details_fixture(name: str) -> None:
    """A 10 KiB document of collapsible elements, half of them open."""
    text = "# Details benchmark\n\n"
    body = (
        "Native collapsible sections keep a long aside out of the reading flow "
        "while their bodies stay ordinary Markdown. 折叠正文仍然参与排版与选择。\n\n"
        "- first item\n- second item\n\n"
    )
    i = 0
    while True:
        attribute = " open" if i % 2 else ""
        block = (
            f"<details{attribute}>\n<summary>Section {i}</summary>\n\n"
            + body
            + "</details>\n\n"
        )
        if len((text + block).encode()) > 10240:
            break
        text += block
        i += 1
    remaining = 10240 - len(text.encode())
    text += ("Closing prose keeps the final line readable. " * 300)[:remaining]
    data = text.encode()
    assert len(data) == 10240
    (ROOT / name).write_bytes(data)


details_fixture("details-10k.md")
