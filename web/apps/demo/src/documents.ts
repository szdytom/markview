import componentGuide from "../../../../docs/mvaac.md";

export const documents = {
	"component-guide": {
		name: "mvaac.md",
		markdown: componentGuide,
	},
	welcome: {
		name: "typography.md",
		markdown: `# A note on typography

A good page makes room for its words. The line has a rhythm, the paragraph has a shape, and the space between them gives the eye somewhere to rest.

Markview takes Markdown from source to a typeset page. Try selecting a sentence here, follow a heading in the contents, or switch to **Edit** and change a few words.

## The shape of a paragraph

Typography begins with the ordinary: letters, spaces, and the length of a line. Justification and hyphenation work together to make an even texture, while careful line breaking keeps the reading comfortable. Resize the window and watch the page find its shape again.

> The detail is there to help you read. The best page lets you forget it.

## Two writing systems, one page

中文排版：标点、换行、字距与字体都由 Markview 处理。中英文混排同样可以排版。

A little **emphasis**, a quieter *aside*, and an inline expression: $e^{i\\pi} + 1 = 0$.

## A few things to try

- **Read:** navigate this document using the contents panel.
- **Edit:** change the source and see the preview follow your place.
- **Explore:** choose another document above, or open your own Markdown file.

## A folded thought

<details>
<summary>There is more to a page than its first impression</summary>

### Inside the margin

Long passages, small notes, tables, and formulas all belong to the same document. Expand this note, or jump here from the contents panel.

</details>

---

*Every page starts with a line.*
`,
	},
	"field-notes": {
		name: "field-notes.md",
		markdown: `# Field notes

An afternoon walk, a pocket notebook, and a few things worth keeping.

## Along the water

The path followed the river through a stand of trees. Light moved over the water in small patches. On the opposite bank, a row of windows caught the last of the afternoon sun.

> Stop long enough, and the place begins to tell you what you missed.

## In the notebook

| Place | Observation |
| --- | --- |
| River bend | Reeds leaning into the current |
| Footbridge | A bright line of reflected sky |
| Garden wall | New leaves above the old stone |

## A sketch of the route

<svg width="480" height="140" viewBox="0 0 480 140">
<rect width="480" height="140" rx="8" fill="#eef0f8"/>
<path d="M28 104 Q100 20 180 72 T340 60 T452 30" fill="none" stroke="#5355c9" stroke-width="3"/>
<circle cx="28" cy="104" r="5" fill="#5355c9"/>
<circle cx="452" cy="30" r="5" fill="#5355c9"/>
</svg>

## Before heading home

- [x] Walk to the footbridge
- [x] Make a sketch
- [ ] Return when the trees have turned

中文排版：标点、换行、字距与字体都由 Markview 处理。
`,
	},
	technical: {
		name: "technical.md",
		markdown: `# A technical page

A short reference with equations, code, and structured information.

## From source to page

The host supplies Markdown, fonts, and image resources. Markview handles parsing, text shaping, line breaking, and drawing.

| Input | Result |
| --- | --- |
| Markdown source | Document structure |
| Font faces | Shaped text |
| Available width | Broken lines |
| Document layout | Canvas rendering |

## A small program

\`\`\`rust
fn main() {
    let words = ["source", "layout", "page"];
    for word in words {
        println!("{word}");
    }
}
\`\`\`

## The language of mathematics

An inline identity, $a^2 + b^2 = c^2$, sits alongside ordinary text. Display equations get their own space:

$$
\\int_0^1 x^2\\,dx = \\frac{1}{3}
$$

## Keeping your place

The editor and preview follow the same source position. Scroll either pane in **Edit** mode to see the other follow, then switch to **Read** for an uninterrupted view.

<details>
<summary>A note on source positions</summary>

### Unicode text

中文、emoji 😀 and combining characters é remain part of the source as you edit.

</details>
`,
	},
} as const;
