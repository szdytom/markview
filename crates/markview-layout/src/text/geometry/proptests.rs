use super::*;
use crate::{
	document::BlockId,
	scene::{BlockLayout, Draw, Overflow, Paint, PlacedBlock},
};
use proptest::prelude::*;
use std::sync::Arc;

fn text(tokens: &[usize]) -> (String, Vec<usize>) {
	let alphabet = ["a", "β", "中", "😀", "e\u{301}", "👩‍💻"];
	let mut text = String::new();
	let mut boundaries = vec![0];
	for &token in tokens {
		text.push_str(alphabet[token]);
		boundaries.push(text.len());
	}
	(text, boundaries)
}

fn fixture(
	text: String,
	parts: usize,
	rtl: bool,
	atomic: bool,
	visible: f32,
	clip_left: f32,
	top: f32,
) -> LayoutSnapshot {
	let width = parts as f32 * 16.0;
	let rect = Rect {
		x: 0.0,
		y: 3.0,
		w: width,
		h: 8.0,
	};
	let mut node = TextNode::new(text, "\n");
	node.push(TextCluster {
		mixed_spacing: (0.0, 0.0),
		range: 0..node.text.len(),
		rect,
		rtl,
		atomic,
		command: 0,
	});
	LayoutSnapshot {
		blocks: vec![PlacedBlock {
			id: BlockId::new(1).unwrap(),
			content: Default::default(),
			y: top,
			flow: Arc::default(),
			layout: Arc::new(BlockLayout {
				text: vec![node],
				draws: vec![Draw::Rect(rect, Paint::Text)],
				height: 16.0,
				overflow: vec![Overflow {
					rect: Rect {
						x: clip_left,
						y: 0.0,
						w: visible,
						h: 16.0,
					},
					content_width: width,
					commands: 0..1,
					gutter: 0.0,
				}],
				..Default::default()
			}),
		}],
		..Default::default()
	}
}

proptest! {
	#[test]
	fn text_hits_choose_the_nearest_declared_grapheme_edge_under_rtl_atomicity_and_pan(
		tokens in prop::collection::vec(0usize..6, 1..7), rtl in any::<bool>(), atomic in any::<bool>(), raw_visible in any::<u16>(), raw_pan in 0u16..129, fraction in 0u16..257, top in 0u16..129,
	) {
		let (text, boundaries) = text(&tokens);
		let width = tokens.len() as u16 * 16;
		let visible = 1 + raw_visible % width;
		let pan = raw_pan.min(width - visible);
		let snapshot = fixture(text, tokens.len(), rtl, atomic, f32::from(visible), 0.0, f32::from(top));
		let horizontal = HashMap::from([((0, 0), f32::from(raw_pan))]);
		let x = f32::from(visible) * f32::from(fraction) / 256.0;
		let y = f32::from(top) + 7.0;
		let edges: Vec<_> = if atomic { vec![(0, 0), (1, *boundaries.last().unwrap())] } else { boundaries.iter().copied().enumerate().collect() };
		let count = edges.len() - 1;
		let distance = |part: usize| { let along = part as f32 * f32::from(width) / count as f32; let visual = if rtl { f32::from(width) - along } else { along }; (visual - f32::from(pan) - x).abs() };
		let nearest_distance = edges.iter().map(|&(part, _)| distance(part)).fold(f32::INFINITY, f32::min);
		let hit = snapshot.geometry_hit_test_text(x, y, &horizontal, 55).unwrap();
		let edge = edges.iter().find(|&&(_, byte)| byte == hit.offset).unwrap();
		prop_assert_eq!((hit.revision, hit.block, hit.node), (55, 0, 0));
		prop_assert!(distance(edge.0) <= nearest_distance + 0.0001);
		prop_assert_eq!(hit.affinity, if edge.0 == count { Affinity::After } else { Affinity::Before });
		prop_assert!(snapshot.contains_text(x, y, &horizontal));
		prop_assert!(!snapshot.contains_text(-0.5, y, &horizontal));
		prop_assert!(!snapshot.contains_text(f32::from(visible) + 0.5, y, &horizontal));
	}

	#[test]
	fn selection_rectangles_cover_logical_parts_before_pan_and_clipping(
		tokens in prop::collection::vec(0usize..6, 1..7), rtl in any::<bool>(), atomic in any::<bool>(), raw_a in any::<usize>(), raw_b in any::<usize>(), raw_visible in any::<u16>(), raw_pan in 0u16..129, stale in any::<bool>(), top in 0u16..129,
	) {
		let (text, boundaries) = text(&tokens);
		let (a, b) = (raw_a % boundaries.len(), raw_b % boundaries.len());
		let width = tokens.len() as u16 * 16;
		let visible = 1 + raw_visible % width;
		let pan = raw_pan.min(width - visible);
		let snapshot = fixture(text, tokens.len(), rtl, atomic, f32::from(visible), 0.5, f32::from(top));
		let horizontal = HashMap::from([((0, 0), f32::from(raw_pan))]);
		let position = |index, revision| GeometryPosition { revision, block: 0, node: 0, offset: boundaries[index], affinity: Affinity::Before };
		let selection = GeometrySelection { anchor: position(a, if stale { 54 } else { 55 }), focus: position(b, 55) };
		let mut expected = Vec::new();
		if a != b && !stale {
			let (start, end) = if atomic { (0, tokens.len()) } else { (a.min(b), a.max(b)) };
			let (left, right) = if rtl { (tokens.len() - end, tokens.len() - start) } else { (start, end) };
			let left = (left as f32 * 16.0 - f32::from(pan)).max(0.5);
			let right = (right as f32 * 16.0 - f32::from(pan)).min(f32::from(visible) + 0.5);
			if right > left { expected.push(Rect { x: left, y: f32::from(top) + 3.0, w: right - left, h: 8.0 }); }
		}
		for actual in [snapshot.geometry_selection_rects(selection, &horizontal, 55), snapshot.geometry_selection_rects(GeometrySelection { anchor: selection.focus, focus: selection.anchor }, &horizontal, 55)] {
			prop_assert_eq!(actual.len(), expected.len());
			for (actual, expected) in actual.iter().zip(&expected) {
				// Splitting at most six parts adds less than 1e-4 layout pixel error.
				prop_assert!((actual.x - expected.x).abs() <= 0.0001);
				prop_assert!((actual.w - expected.w).abs() <= 0.0001);
				prop_assert_eq!((actual.y, actual.h), (expected.y, expected.h));
			}
		}
	}
}
