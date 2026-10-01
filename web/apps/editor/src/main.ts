import { Editor } from "@markview/editor";
import { browserResources } from "@markview/resources";
import { fonts } from "../../demo/src/fonts.js";

const markdown = `# The reading desk

Write Markdown on the left. The reading pane follows your place, and scrolling the page follows back to the source. Selection and keyboard focus stay with your work.

## A note on typography

Markview shapes and lays out Markdown directly, with publication-quality line breaking. This is a live document, so try adding a heading or widening the reading pane with the divider.

中文、emoji 😀 and combining characters é keep their source positions across edits.

## Structure and rhythm

> A quote can contain a paragraph, a list, and its own headings.
>
> - First observation
> - A second observation with **emphasis**

| Element | Reading behavior |
| --- | --- |
| Paragraph | Follows its wrapped lines |
| Code | Follows individual lines |
| Formula | Uses an atomic source range |

$$
E = mc^2
$$

## A small program

`;
const source =
	markdown +
	'```rust\nfn main() {\n    println!("Hello, reading desk");\n}\n```\n\n' +
	"<details>\n<summary>A folded note</summary>\n\n### Inside the note\n\nThe contents panel opens this section when you navigate to it.\n\n</details>\n\n" +
	Array.from(
		{ length: 24 },
		(_, i) =>
			`## Reading passage ${i + 1}\n\n${"A source position follows the reading line, even when the two panes have different heights. ".repeat(10)}\n\n`,
	).join("");
let dark = false;
try {
	const editor = await Editor.mount(
		document.querySelector("#desk") as HTMLElement,
		{
			markdown: source,
			viewer: {
				initialization: { fonts },
				resources: browserResources({ baseUrl: document.baseURI }),
			},
		},
	);
	Object.assign(window, { editor });
	document.querySelector("#theme")?.addEventListener("click", (event) => {
		dark = !dark;
		editor.setOptions({ theme: dark ? "dark" : "light" });
		(event.currentTarget as HTMLElement).textContent = dark
			? "Light theme"
			: "Dark theme";
	});
} catch (error) {
	const node = document.querySelector("#error") as HTMLElement;
	node.hidden = false;
	node.textContent = String(error);
}
