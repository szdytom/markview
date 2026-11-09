use super::*;

const SOURCES: &[&str] = &[
	"A paragraph with **bold**, 中文, $x^2$ and enough words to wrap.\n",
	"first<br><br>last\n",
	"before $$x^2 + y^2$$ after\n",
	"![caption](image.png \"Image title\")\n",
	"```rust\nlet value = 42;\n\nlet long_line = value + value + value;\n```\n",
	"> first paragraph\n>\n> second paragraph\n",
	"<details open>\n<summary>Summary</summary>\n\nfirst\n\n<details>\n<summary>Nested</summary>\n\nsecond\n\n</details>\n\n</details>\n",
	"- first item\n  - nested item\n- second item\n",
	"1. first\n2. second\n",
	"<dl><dt>term</dt><dd>first</dd><dd>second</dd></dl>\n",
	"<figure>\n\nfirst\n\nsecond\n\n<figcaption>Caption</figcaption>\n</figure>\n",
	"| first | second |\n| --- | --- |\n| alpha | beta |\n| gamma | delta |\n",
	"<table><caption>Caption</caption><tr><td rowspan=2><p>first</p><p>second</p></td><td>third</td></tr><tr><td>fourth</td></tr></table>\n",
];

fn options() -> LayoutOptions {
	let mut stylesheet = (*crate::style::Stylesheet::bundled(false)).clone();
	stylesheet.merge(&crate::style::Stylesheet::parse(
		"format_version = 2\nversion = 1\n[[rule]]\nwhen = ['img', 'caption']\nsource = 'title'\nspace_before = 0.35\nspace_after = 0.45\n[[rule]]\nwhen = ['list_item']\nspace_before = 0.35\nspace_after = 0.45\npadding = 0.3\n",
	).unwrap());
	LayoutOptions {
		stylesheet: Arc::new(stylesheet),
		..Default::default()
	}
}

fn heights(source: &str, y: f32, options: &LayoutOptions) -> Vec<f32> {
	let document = markview_commonmark::parse(source);
	let mut engine = LayoutEngine::new();
	engine.shaper.set_stylesheet(options.stylesheet.clone());
	engine.shaper.set_fonts(&options.fonts);
	let images = Default::default();
	let mut context = BlockContext {
		nested: None,
		cancelled: &|| false,
		fields: Default::default(),
		shaper: &mut engine.shaper,
		math: &mut engine.math,
		images: &images,
		highlight_cache: engine.highlights.results(),
		marker_depth: 0,
		enum_depth: 0,
		footnote_column: 1,
	};
	document
		.blocks()
		.iter()
		.map(|block| {
			context.fields = blocks::FieldCursor::new(block);
			context.block(
				block,
				0.,
				y,
				options.width,
				options,
				&mut BlockLayout::default(),
			)
		})
		.collect()
}

#[test]
fn block_heights_do_not_depend_on_their_vertical_position() {
	let options = LayoutOptions {
		width: 180.,
		codeblock_wrap: true,
		..options()
	};
	for source in SOURCES {
		let expected = heights(source, 0., &options);
		assert!(!expected.is_empty(), "{source}");
		for y in [44.1, 307.872, 1024.25, 1_000_000.] {
			assert_eq!(
				heights(source, y, &options),
				expected,
				"y={y}, {source}"
			);
		}
	}
}

mod proptests {
	use super::*;
	use proptest::prelude::*;

	proptest! {
		#[test]
		fn translated_blocks_preserve_height(
			case in 0..SOURCES.len(), y in -1_000_000f32..1_000_000.,
			font_size in 10f32..42., width in 80f32..760., wrap in any::<bool>(),
		) {
			let options = LayoutOptions { width, font_size, codeblock_wrap: wrap, ..options() };
			prop_assert_eq!(heights(SOURCES[case], y, &options), heights(SOURCES[case], 0., &options));
		}
	}
}
