use super::*;
use crate::{document::TextStyle, microtype, style::Stylesheet};
use proptest::prelude::*;
use std::sync::Arc;

fn compare(shaper: &mut TextShaper, text: &str, spans: &[Span], size: f32) {
	let mut measured = shaper.measure(text, spans, size);
	let mut painted = shaper.shape(text, spans, size, false);
	assert_eq!(measured.len(), painted.len());
	for (a, b) in measured.iter().zip(&painted) {
		assert_eq!(a.range, b.range);
		assert_eq!(a.width.to_bits(), b.width.to_bits());
		assert_eq!(a.glyph_count, b.glyphs.len());
	}
	measured.sort_by_key(|c| c.range.start);
	painted.sort_by_key(|c| c.range.start);
	microtype::space_mixed_measurements(&mut measured, text, spans, size);
	microtype::space_mixed_scripts(&mut painted, text, spans, size);
	for (i, (a, b)) in measured.iter().zip(&painted).enumerate() {
		assert_eq!(a.width.to_bits(), b.width.to_bits());
		assert_eq!(a.mixed, b.mixed);
		let adjust = |c: &ClusterMetrics| {
			microtype::adjust(
				&text[c.range.clone()],
				c.width,
				size,
				c.mixed,
				c.glyph_count,
				microtype::Typography::default(),
			)
		};
		assert_eq!(adjust(a), adjust(b));
		assert_eq!(
			microtype::quote_edge_break(&measured, text, i, size),
			microtype::quote_edge_break(&painted, text, i, size)
		);
	}
}

#[test]
fn measurements_match_full_shaping_without_retaining_ink_bounds() {
	let mut shaper = TextShaper::new();
	for css in [
		"",
		"[[rule]]\nwhen=['body']\ntop_edge='bounds'\nbottom_edge='bounds'\nbackground_top_edge='bounds'\nbackground_bottom_edge='bounds'",
	] {
		let mut sheet = (*Stylesheet::bundled(false)).clone();
		if !css.is_empty() {
			sheet.merge(
				&Stylesheet::parse(&format!(
					"format_version=3\nversion=1\n{css}"
				))
				.unwrap(),
			);
		}
		shaper.set_stylesheet(Arc::new(sheet));
		for text in [
			"",
			"office ffi fi  ",
			"e\u{301} a\u{308}",
			"中文 typography，“测试”！",
			"\u{202e}office ab\u{202c}",
			"مرحبا العربية",
			"👩‍💻 ✈️ 🇨🇳",
			"soft\u{ad}hyphen\nnext\tline",
		] {
			for size in [9., 18., 32.] {
				compare(&mut shaper, text, &[], size);
			}
		}
	}
	shaper.bounds.clear();
	shaper.measure("office 中文", &[], 18.);
	assert!(shaper.bounds.is_empty());
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(128))]
	#[test]
	fn measurements_preserve_styled_unicode_clusters(
		parts in prop::collection::vec((prop::sample::select(vec!["office", "ffi", "e\u{301}", "中文", "“测试”，", "\u{202e}ab\u{202c}", "مرحبا", "👩‍💻", "\u{ad}", "\n", " ", ""]), any::<u8>()), 0..24),
		size in 8u16..60,
		spacing in -2i8..6,
	) {
		let mut shaper = TextShaper::new();
		shaper.appearance.letter_spacing = f32::from(spacing) * 0.01;
		let mut text = String::new();
		let mut spans = Vec::new();
		for (part, flags) in parts {
			let start = text.len(); text.push_str(part);
			if flags & 128 != 0 && start < text.len() {
				spans.push(Span { range: start..text.len(), style: TextStyle { bold: flags & 1 != 0, italic: flags & 2 != 0, code: flags & 4 != 0, superscript: flags & 8 != 0, subscript: flags & 16 != 0, ..Default::default() }, link: None, math_error: flags & 32 != 0, footnote_ref: flags & 64 != 0 });
			}
		}
		compare(&mut shaper, &text, &spans, f32::from(size));
	}
}
