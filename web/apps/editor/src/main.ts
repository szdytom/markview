import { loadFontSet } from "@markview/fonts";
import { Editor } from "@markview/editor";
import { browserResources } from "@markview/resources";
import { fonts } from "../../assets/fonts.js";

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

## A static illustration

<svg width="320" height="96" viewBox="0 0 320 96">
<rect width="320" height="96" rx="12" fill="#e2e8f0"/>
<path d="M24 72 Q84 8 144 48 T296 24" fill="none" stroke="#0f766e" stroke-width="4"/>
<circle cx="144" cy="48" r="6" fill="#0f766e"/>
</svg>

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
	const fontSet = await loadFontSet({ sources: fonts });
	const editor = await Editor.mount(
		document.querySelector("#desk") as HTMLElement,
		{
			markdown: source,
			viewer: {
				fonts: fontSet,
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
