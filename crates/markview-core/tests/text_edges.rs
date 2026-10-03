use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	scene::{Draw, Glyph, LayoutSnapshot, Paint, Rect},
	shaping::TextShaper,
	style::{CjkType, ColorField, Condition, Stylesheet, chain_set},
};
use skrifa::{
	GlyphId, MetadataProvider,
	instance::{NormalizedCoord, Size},
};
use std::sync::Arc;

fn options(rules: &str, size: f32, width: f32) -> LayoutOptions {
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.set_cjk_type(CjkType::Sc);
	sheet.merge(
		&Stylesheet::parse(&format!("format_version=2\nversion=1\n{rules}"))
			.unwrap(),
	);
	LayoutOptions {
		font_size: size,
		width,
		stylesheet: Arc::new(sheet),
		fonts: FontConfig::from_faces(
			0x74657874,
			[
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
				include_bytes!("fonts/NotoSerifCJKsc-Regular-subset.otf")
					.as_slice(),
				include_bytes!("fonts/NotoSansMono-Regular-subset.otf")
					.as_slice(),
				include_bytes!("fonts/NotoSansMonoCJKsc-Regular-subset.otf")
					.as_slice(),
			]
			.into_iter()
			.map(|data| parley::fontique::Blob::new(Arc::new(data)))
			.collect(),
		),
		..Default::default()
	}
}

fn layout(source: &str, rules: &str, size: f32, width: f32) -> LayoutSnapshot {
	LayoutEngine::new()
		.layout(&document::parse(source), &options(rules, size, width))
}

fn code(paint: Paint) -> bool {
	matches!(paint, Paint::Cascade(chain, _) if chain_set(chain).contains(Condition::Code))
}

fn glyphs(snapshot: &LayoutSnapshot) -> Vec<&Glyph> {
	snapshot.blocks[0]
		.layout
		.draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Glyph(g) => Some(g),
			_ => None,
		})
		.collect()
}

fn chips(snapshot: &LayoutSnapshot) -> Vec<Rect> {
	snapshot.blocks[0]
		.layout
		.draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Rect(
				rect,
				paint @ Paint::Cascade(_, ColorField::Background),
			) if code(*paint) => Some(*rect),
			_ => None,
		})
		.collect()
}

fn ink(g: &Glyph) -> Option<(f32, f32)> {
	let font =
		skrifa::FontRef::from_index(g.font.data.data(), g.font.index).unwrap();
	let coords: Vec<_> = g
		.coords
		.iter()
		.map(|&v| NormalizedCoord::from_bits(v))
		.collect();
	let bounds = font
		.glyph_metrics(Size::new(g.size), &coords[..])
		.bounds(GlyphId::new(u32::from(g.id)))?;
	(bounds.y_max > bounds.y_min)
		.then_some((g.y - bounds.y_max, g.y - bounds.y_min))
}

#[test]
fn empty_font_collections_keep_blank_and_unshaped_rows() {
	let mut opts = options("", 18., 800.);
	opts.fonts = FontConfig::from_faces(0x656d707479, Vec::new());
	let mut engine = LayoutEngine::new();
	for source in [
		"```\n```",
		"```\n\n```",
		"```\nunavailable\n```",
		"unavailable",
	] {
		let snapshot = engine.layout(&document::parse(source), &opts);
		assert!(snapshot.height.is_finite() && snapshot.height > 0.);
		assert!(glyphs(&snapshot).is_empty());
	}
	// Font arrival invalidates the empty layout, as in the browser startup path.
	opts.fonts = options("", 18., 800.).fonts;
	let snapshot =
		engine.layout(&document::parse("```\nunavailable\n```"), &opts);
	assert!(!glyphs(&snapshot).is_empty());
}

#[test]
fn layout_edges_use_local_em_and_invalidate_cached_geometry() {
	let doc = document::parse("Ag");
	let mut engine = LayoutEngine::new();
	let mut opts = options("[[rule]]\nwhen=['body']\nline_height=4", 18., 800.);
	let cap = engine.layout(&doc, &opts);
	let g = glyphs(&cap)[0];
	let font =
		skrifa::FontRef::from_index(g.font.data.data(), g.font.index).unwrap();
	let cap_height =
		font.metrics(Size::new(g.size), &[][..]).cap_height.unwrap();
	let row = cap.blocks[0].layout.text[0].clusters[0].rect;
	assert!((g.y - (row.y + (row.h + cap_height) / 2.)).abs() < 0.01);
	assert!(Arc::ptr_eq(
		&cap.blocks[0].layout,
		&engine.layout(&doc, &opts).blocks[0].layout
	));
	let mut sheet = (*opts.stylesheet).clone();
	sheet.merge(&Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['body']\ntop_edge=0.4\nbottom_edge=-0.1").unwrap());
	opts.stylesheet = Arc::new(sheet);
	let numeric = engine.layout(&doc, &opts);
	assert!(!Arc::ptr_eq(
		&cap.blocks[0].layout,
		&numeric.blocks[0].layout
	));
	let g = glyphs(&numeric)[0];
	assert!((g.y - (row.y + (row.h + 0.3 * g.size) / 2.)).abs() < 0.01);
	assert_eq!(glyphs(&numeric).len(), glyphs(&cap).len());
}

#[test]
fn a_mixed_font_background_is_one_decoration_and_does_not_set_line_height() {
	let source = "中文`中文API gyp`测试";
	let bare = layout(source, "[[rule]]\nwhen=['code']\npadding=0", 18., 800.);
	let padded = layout(
		source,
		"[[rule]]\nwhen=['code']\npadding=[2,0,3,0]",
		18.,
		800.,
	);
	assert_eq!(chips(&bare).len(), 1);
	assert_eq!(chips(&padded).len(), 1);
	assert!((chips(&padded)[0].h - chips(&bare)[0].h - 5. * 18.).abs() < 0.01);
	assert_eq!(bare.height, padded.height);
	assert_eq!(
		glyphs(&bare).iter().map(|g| (g.x, g.y)).collect::<Vec<_>>(),
		glyphs(&padded)
			.iter()
			.map(|g| (g.x, g.y))
			.collect::<Vec<_>>()
	);
	let bounded = layout(
		source,
		"[[rule]]\nwhen=['code']\npadding=0\nbackground_top_edge=2.0\nbackground_bottom_edge=-1.0",
		18.,
		800.,
	);
	assert_eq!(bounded.height, bare.height);
	assert!((chips(&bounded)[0].h - 3. * 18. * 0.9).abs() < 0.01);
	let wrapped = layout(
		"`中文API gyp long_code_identifier_with_123 中文API gyp`",
		"",
		18.,
		120.,
	);
	assert!(chips(&wrapped).len() > 1);
	let block = &wrapped.blocks[0].layout;
	for cluster in &block.text[0].clusters {
		assert!(chips(&wrapped).iter().any(|r| r.x <= cluster.rect.x + 0.01
			&& r.x + r.w >= cluster.rect.x + cluster.rect.w - 0.01));
	}
}

#[test]
fn cjk_prose_keeps_the_same_external_gap_for_every_inline_code_script() {
	let gaps = |snapshot: &LayoutSnapshot| {
		let body: Vec<_> = glyphs(snapshot)
			.into_iter()
			.filter(|g| {
				!code(g.paint)
					&& g.font.data.data()
						== include_bytes!(
							"fonts/NotoSerifCJKsc-Regular-subset.otf"
						)
			})
			.collect();
		let chip = chips(snapshot)[0];
		assert_eq!(body.len(), 4);
		(
			chip.x - body[1].x - body[1].size,
			body[2].x - chip.x - chip.w,
		)
	};
	for size in [18., 24.] {
		for rules in ["", "[[rule]]\nwhen=['code']\npadding=0"] {
			for content in ["中文", "alphabet", "中文API", "_"] {
				let snapshot =
					layout(&format!("中文`{content}`中文"), rules, size, 800.);
				let (left, right) = gaps(&snapshot);
				assert!((left - size * 0.25).abs() < 0.01, "{content}: {left}");
				assert!(
					(right - size * 0.25).abs() < 0.01,
					"{content}: {right}"
				);
			}
		}
		let chinese = layout("中文 `中文` 中文", "", size, 800.);
		let latin = layout("中文 `alphabet` 中文", "", size, 800.);
		let (a, b) = gaps(&chinese);
		let (c, d) = gaps(&latin);
		assert!((a - c).abs() < 0.01 && (b - d).abs() < 0.01);
		let wrapped = layout("中文  \n`中文`  \n中文", "", size, 800.);
		assert!((chips(&wrapped)[0].x - glyphs(&wrapped)[0].x).abs() < 0.01);
		let code_glyphs: Vec<_> = glyphs(&wrapped)
			.into_iter()
			.filter(|g| code(g.paint))
			.collect();
		assert!(
			(code_glyphs[1].x - code_glyphs[0].x - size * 0.9).abs() < 0.01
		);
	}
}

#[test]
fn bounds_follow_ink_and_blank_fragments_fall_back_to_font_metrics() {
	let rules = "[[rule]]\nwhen=['code']\npadding=0\nbaseline=0\nbackground_top_edge='bounds'\nbackground_bottom_edge='bounds'";
	let snapshot = layout("X`  中文API gyp  `Y", rules, 18., 800.);
	let bounds: Vec<_> = glyphs(&snapshot)
		.into_iter()
		.filter(|g| code(g.paint))
		.filter_map(ink)
		.collect();
	let top = bounds.iter().map(|b| b.0).fold(f32::INFINITY, f32::min);
	let bottom = bounds.iter().map(|b| b.1).fold(f32::NEG_INFINITY, f32::max);
	let chip = chips(&snapshot)[0];
	assert!((chip.y - top).abs() < 0.05, "{chip:?} {top}");
	assert!((chip.y + chip.h - bottom).abs() < 0.05, "{chip:?} {bottom}");
	let blank = layout("X`   `Y", rules, 18., 800.);
	assert!(chips(&blank)[0].h > 0.);
	let raised_ink = layout("X`  ^ \u{ad}  `Y", rules, 18., 800.);
	let bounds: Vec<_> = glyphs(&raised_ink)
		.into_iter()
		.filter(|g| code(g.paint))
		.filter_map(ink)
		.collect();
	let chip = chips(&raised_ink)[0];
	assert!(
		(chip.y + chip.h
			- bounds.iter().map(|b| b.1).fold(f32::NEG_INFINITY, f32::max))
		.abs() < 0.05
	);
	let bounded = layout(
		"Ag\u{301}",
		"[[rule]]\nwhen=['body']\ntop_edge='bounds'\nbottom_edge='bounds'\nline_height=0.5",
		18.,
		800.,
	);
	let ink: Vec<_> = glyphs(&bounded).into_iter().filter_map(ink).collect();
	let row = bounded.blocks[0].layout.text[0].clusters[0].rect;
	assert!(
		ink.iter().all(|&(top, bottom)| top >= row.y - 0.05
			&& bottom <= row.y + row.h + 0.05)
	);
	for source in ["```\n\nA\n\n```", "A\n\nB"] {
		assert!(layout(source, "", 18., 800.).height.is_finite());
	}
}

#[test]
fn baseline_moves_glyphs_backgrounds_and_decorations_at_each_size() {
	for size in [18., 24.] {
		let base = "[[rule]]\nwhen=['body']\ntop_edge=2\nbottom_edge=-2\n[[rule]]\nwhen=['code']\ndecoration=['underline']\nbaseline=";
		let zero = layout("A `gyp`", &format!("{base}0"), size, 800.);
		for offset in [-0.1, 0.1] {
			let moved =
				layout("A `gyp`", &format!("{base}{offset}"), size, 800.);
			let delta = offset * size * 0.9;
			for (a, b) in glyphs(&zero).into_iter().zip(glyphs(&moved)) {
				assert!(
					(b.y - a.y - if code(a.paint) { delta } else { 0. }).abs()
						< 0.01
				);
			}
			for (a, b) in zero.blocks[0]
				.layout
				.draws
				.iter()
				.zip(&moved.blocks[0].layout.draws)
			{
				if let (Draw::Rect(a, p), Draw::Rect(b, _)) = (a, b)
					&& code(*p)
				{
					assert!((b.y - a.y - delta).abs() < 0.01);
				}
			}
		}
	}
	let source = "A <sup>g</sup> B";
	let base = "[[rule]]\nwhen=['body']\ntop_edge=2\nbottom_edge=-2\n[[rule]]\nwhen=['sup']\nbaseline=";
	let zero = layout(source, &format!("{base}0"), 18., 800.);
	let moved = layout(source, &format!("{base}0.1"), 18., 800.);
	let mut superscripts = 0;
	for (a, b) in glyphs(&zero).into_iter().zip(glyphs(&moved)) {
		let shifted = matches!(a.paint, Paint::Cascade(chain, _) if chain_set(chain).contains(Condition::Sup));
		superscripts += usize::from(shifted);
		assert!(
			(b.y - a.y - if shifted { 0.1 * a.size } else { 0. }).abs() < 0.01
		);
	}
	assert!(superscripts > 0);
}

#[test]
fn labels_share_the_background_edges_and_baseline_policy() {
	let opts = options(
		"[[rule]]\nwhen=['body']\nbaseline=0.1\nbackground_top_edge='bounds'\nbackground_bottom_edge='bounds'",
		18.,
		800.,
	);
	let mut shaper = TextShaper::with_fonts(opts.fonts);
	shaper.set_stylesheet(opts.stylesheet);
	let appearance = shaper.appearance.clone();
	let (draws, _, _) = shaper.label_runs(
		"gyp",
		18.,
		0.,
		100.,
		&appearance,
		Paint::Text,
		Some(Paint::Panel),
	);
	let Draw::Rect(rect, _) = &draws[0] else {
		panic!("the label has a background")
	};
	let bounds: Vec<_> = draws
		.iter()
		.filter_map(|d| match d {
			Draw::Glyph(g) => {
				assert!((g.y - 101.8).abs() < 0.01);
				ink(g)
			}
			_ => None,
		})
		.collect();
	assert!(
		(rect.y - bounds.iter().map(|b| b.0).fold(f32::INFINITY, f32::min))
			.abs() < 0.05
	);
	assert!(
		(rect.y + rect.h
			- bounds.iter().map(|b| b.1).fold(f32::NEG_INFINITY, f32::max))
		.abs() < 0.05
	);
	assert!(
		shaper
			.label_runs(
				"",
				18.,
				0.,
				100.,
				&appearance,
				Paint::Text,
				Some(Paint::Panel)
			)
			.0
			.is_empty()
	);
}

#[test]
fn the_issue_six_example_keeps_raised_ink_near_chinese_text() {
	let snapshot = layout("MarkView测试`Inline Code`", "", 18., 800.);
	let glyphs = glyphs(&snapshot);
	let center = |glyphs: &[&Glyph]| {
		let bounds: Vec<_> = glyphs.iter().filter_map(|g| ink(g)).collect();
		(bounds.iter().map(|b| b.0).fold(f32::INFINITY, f32::min)
			+ bounds.iter().map(|b| b.1).fold(f32::NEG_INFINITY, f32::max))
			/ 2.
	};
	let chinese: Vec<_> = glyphs
		.iter()
		.copied()
		.filter(|g| {
			!code(g.paint)
				&& g.font.data.data()
					== include_bytes!("fonts/NotoSerifCJKsc-Regular-subset.otf")
		})
		.collect();
	let inline: Vec<_> =
		glyphs.iter().copied().filter(|g| code(g.paint)).collect();
	assert!(!chinese.is_empty() && !inline.is_empty());
	// Ascenders extend above the optical center; the ink box sits slightly higher.
	let raised = center(&chinese) - center(&inline);
	assert!((0.0..1.0).contains(&raised), "raised by {raised}px");
}
