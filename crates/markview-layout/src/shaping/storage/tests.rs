use super::*;
use crate::document::TextStyle;
use crate::shaping::{Span, TextShaper};
use proptest::prelude::*;

#[test]
fn glyphs_share_style_by_run_and_keep_native_sized_ranges() {
	let mut shaper = TextShaper::new();
	let shaped = shaper.shape(&"office 中文 ".repeat(100), &[], 18., false);
	assert!(shaped.glyphs.positions.len() > 500);
	assert!(shaped.glyphs.runs.len() < shaped.glyphs.positions.len() / 2);
	for (_, style) in &shaped.glyphs.runs {
		let owners = shaped
			.glyphs
			.runs
			.iter()
			.filter(|(_, s)| Arc::ptr_eq(s, style))
			.count();
		assert_eq!(Arc::strong_count(style), owners);
	}
	// Native indices have no narrowing boundary or overflow representation.
	let empty = shaped.glyphs.positions.len()..shaped.glyphs.positions.len();
	assert_eq!(shaped.glyphs.iter(empty).count(), 0);
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(96))]
	#[test]
	fn shared_storage_preserves_each_glyph_and_the_order_of_float_shifts(
		parts in prop::collection::vec(prop::sample::select(vec!["office", "e\u{301}", "中文", "\u{202e}ab\u{202c}", "👩‍💻", " "]), 1..20),
		shifts in prop::collection::vec(-1000i16..1000, 0..8),
	) {
		let text = parts.concat();
		let spans = [Span { range: 0..text.len(), style: TextStyle { italic: true, ..Default::default() }, link: None, math_error: false, footnote_ref: false }];
		let mut shaped = TextShaper::new().shape(&text, &spans, 18., false);
		let mut expanded: Vec<Vec<_>> = shaped.iter().map(|c| shaped.glyphs.iter(c.glyphs.clone()).collect()).collect();
		for shift in shifts {
			let amount = f32::from(shift) / 19.;
			for (i, c) in shaped.clusters.iter().enumerate() {
				shaped.glyphs.shift(c.glyphs.clone(), amount);
				for g in &mut expanded[i] { g.x += amount; }
			}
		}
		for (c, expected) in shaped.iter().zip(expanded) {
			let actual: Vec<_> = shaped.glyphs.iter(c.glyphs.clone()).collect();
			prop_assert_eq!(actual.len(), expected.len());
			for (a, b) in actual.iter().zip(expected) {
				prop_assert_eq!((a.id, a.x.to_bits(), a.y.to_bits()), (b.id, b.x.to_bits(), b.y.to_bits()));
				prop_assert!(Arc::ptr_eq(&a.style, &b.style));
			}
		}
	}
}
