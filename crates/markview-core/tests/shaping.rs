use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	scene::{Draw, Paint},
	shaping::TextShaper,
	style::Stylesheet,
};
use std::sync::Arc;

fn fonts() -> FontConfig {
	FontConfig::from_faces(
		0x7f685f40,
		[
			include_bytes!("fonts/NotoSans-Regular-subset.otf").as_slice(),
			include_bytes!("fonts/NotoSans-Bold-subset.otf").as_slice(),
			include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
			include_bytes!("fonts/NotoSerif-Bold-subset.otf").as_slice(),
			include_bytes!("fonts/NotoSansMono-Regular-subset.otf").as_slice(),
		]
		.into_iter()
		.map(|data| parley::fontique::Blob::new(Arc::new(data)))
		.collect(),
	)
}

#[test]
fn overflowing_line_height_does_not_stall_label_shaping() {
	let text = String::from_utf8_lossy(&[0x01, 0xb3, 0xc1, 0, 0, 0x01]);
	let mut shaper = TextShaper::with_fonts(fonts());
	for size in [f32::from_bits(0x7f685f40), f32::MAX] {
		let (draws, width) =
			shaper.label_measured(&text, size, 0.0, 10.0, Paint::Text);
		assert!(!draws.is_empty());
		assert!(!width.is_nan());
		let fitted = shaper.fit(&text, size, 100.0);
		assert!(shaper.text_width(&fitted, size) <= 100.0);
	}
}

#[test]
fn markdown_cannot_set_a_font_size() {
	let raw = String::from_utf8_lossy(&[0x01, 0xb3, 0xc1, 0, 0, 0x01]);
	let mut engine = LayoutEngine::new();
	let options = LayoutOptions {
		fonts: fonts(),
		..Default::default()
	};
	for source in [
		raw.as_ref(),
		"---\nfont_size: 3.0887546e38\n---\n\nab",
		"<p style='font-size:3.0887546e38px'>ab</p>",
	] {
		let snapshot = engine.layout(&document::parse(source), &options);
		assert!(snapshot.height.is_finite());
		let glyphs: Vec<_> = snapshot
			.blocks
			.iter()
			.flat_map(|block| &block.layout.draws)
			.filter_map(|draw| match draw {
				Draw::Glyph(glyph) => Some(glyph),
				_ => None,
			})
			.collect();
		assert!(!glyphs.is_empty());
		assert!(glyphs.iter().all(|g| {
			g.size < 1000.0 && g.x.is_finite() && g.y.is_finite()
		}));
	}
}

#[test]
fn a_valid_extreme_stylesheet_does_not_stall_document_shaping() {
	let mut engine = LayoutEngine::new();
	for (condition, source) in [("p", "ab"), ("strong", "**ab**")] {
		let mut stylesheet = (*Stylesheet::bundled(false)).clone();
		stylesheet.merge(
			&Stylesheet::parse(&format!(
				"format_version=2\nversion=1\n[[rule]]\nwhen=['{condition}']\nsize=1.716e37"
			))
			.unwrap(),
		);
		let options = LayoutOptions {
			fonts: fonts(),
			stylesheet: Arc::new(stylesheet),
			..Default::default()
		};
		let snapshot = engine.layout(&document::parse(source), &options);
		assert!(
			snapshot.blocks.iter().any(|block| {
				block
					.layout
					.draws
					.iter()
					.any(|draw| matches!(draw, Draw::Glyph(g) if g.size > 1e38))
			}),
			"{condition}"
		);
	}
}

#[test]
fn bundled_themes_shape_cjk_strong_with_a_real_bold_face() {
	let config = FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts"),
		],
		..Default::default()
	};
	let doc =
		document::parse("中\n\n**中**\n\n***中***\n\n# **中**\n\n**`中`**");
	for theme in Stylesheet::READER_THEMES
		.iter()
		.chain(Stylesheet::PDF_THEMES)
	{
		let mut sheet = (*Stylesheet::builtin()).clone();
		if Stylesheet::PDF_THEMES.contains(theme) {
			sheet.merge(&Stylesheet::named_rules("print").unwrap());
		}
		sheet.merge(&Stylesheet::named_rules(theme).unwrap());
		sheet.set_cjk_type(markview_core::style::CjkType::Sc);
		let options = LayoutOptions {
			fonts: config.clone(),
			stylesheet: Arc::new(sheet),
			..Default::default()
		};
		let snapshot = LayoutEngine::new().layout(&doc, &options);
		let weights: Vec<_> = snapshot
			.blocks
			.iter()
			.flat_map(|block| &block.layout.draws)
			.filter_map(|draw| match draw {
				Draw::Glyph(glyph) => Some(
					swash::FontRef::from_index(
						glyph.font.data.data(),
						glyph.font.index as usize,
					)
					.unwrap()
					.attributes()
					.weight()
					.0,
				),
				_ => None,
			})
			.collect();
		assert_eq!(weights.len(), 5, "{theme}: {weights:?}");
		assert!(matches!(weights[0], 400 | 500), "{theme}: {weights:?}");
		assert_eq!(&weights[1..], &[700; 4], "{theme}");
	}
}
