use super::*;
use std::collections::HashMap;
#[test]
fn counts_use_graphemes_and_dictionary_words() {
	assert_eq!(TextCounts::of(""), TextCounts::default());
	assert_eq!(
		TextCounts::of("Hello world!"),
		TextCounts {
			chars: 12,
			words: 2
		}
	);
	assert_eq!(
		TextCounts::of("e\u{301} 👩‍💻 中文"),
		TextCounts { chars: 6, words: 2 }
	);
	assert_eq!(TextCounts::of(" \n\t"), TextCounts { chars: 3, words: 0 });
	// Dictionary segmentation counts 中文文字 as two words, not four.
	assert_eq!(
		TextCounts::of("中文文字"),
		TextCounts { chars: 4, words: 2 }
	);
}
use crate::layout::{LayoutEngine, LayoutOptions};
fn layout(source: &str, width: f32) -> LayoutSnapshot {
	LayoutEngine::new().layout(
		&markview_commonmark::parse(source),
		&LayoutOptions {
			width,
			..Default::default()
		},
	)
}
/// Laid out over the committed subsets, so the clusters do not depend on
/// whatever faces the host happens to have.
fn pinned(source: &str) -> LayoutSnapshot {
	LayoutEngine::new().layout(
		&markview_commonmark::parse(source),
		&LayoutOptions {
			width: 400.0,
			fonts: crate::fonts::FontConfig {
				ignore_system_fonts: true,
				directories: vec![
					std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
						.join("tests/fonts"),
				],
				..Default::default()
			},
			..Default::default()
		},
	)
}

/// Every offset a pointer can land on is a grapheme boundary, so a caret never
/// comes to rest inside a character. A ligature is several graphemes set as
/// one glyph and its letters have to be tellable apart; `e` followed by a
/// combining acute is one grapheme and is not.
#[test]
fn hit_testing_only_lands_on_grapheme_boundaries() {
	for source in ["office", "difficult", "Cafe\u{301}", "e\u{301}x", "中文"]
	{
		let snapshot = pinned(source);
		let node = &snapshot.blocks[0].layout.text[0];
		let mut boundaries: Vec<usize> =
			node.text.grapheme_indices(true).map(|(i, _)| i).collect();
		boundaries.push(node.text.len());
		let block = &snapshot.blocks[0];
		let left = node
			.clusters
			.iter()
			.map(|c| c.rect.x)
			.fold(f32::INFINITY, f32::min);
		let right = node
			.clusters
			.iter()
			.map(|c| c.rect.x + c.rect.w)
			.fold(f32::NEG_INFINITY, f32::max);
		for step in 0..=200 {
			let x = left + (right - left) * (step as f32 / 200.0);
			let y = block.y
				+ node.clusters.at(0).rect.y
				+ node.clusters.at(0).rect.h * 0.5;
			let Some(hit) =
				snapshot.geometry_hit_test_text(x, y, &HashMap::new(), 1)
			else {
				continue;
			};
			assert_eq!((hit.block, hit.node), (0, 0), "{source:?} at {x}");
			assert!(
				boundaries.contains(&hit.offset),
				"{source:?}: a hit at {x} landed on {}, inside a grapheme",
				hit.offset
			);
		}
	}
}
#[test]
fn copies_reading_text_code_tables_and_atomic_math() {
	let snapshot = layout(
		"# Title\n\nA **bold** [link](https://example.com) $x^2$.\n\n```rust\n\tlet x = 1;\n\n```\n\n| A | B |\n|---|---|\n| 中 | 文 |\n\n- one\n- [x] two",
		300.0,
	);
	let text = snapshot
		.geometry_extract_text(snapshot.geometry_select_all(9).unwrap(), 9);
	assert_eq!(
		text,
		"Title\n\nA bold link x^2.\n\n\tlet x = 1;\n\n\n\nA\tB\n中\t文\none\ntwo"
	);
	assert!(
		snapshot
			.geometry_extract_text(snapshot.geometry_select_all(9).unwrap(), 10)
			.is_empty()
	);
}
#[test]
fn double_and_triple_click_ranges_follow_reading_text() {
	let snapshot = layout("First word here.\n\nSecond paragraph.", 300.0);
	let word = snapshot
		.geometry_select_word_at(GeometryPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: 8,
			affinity: Affinity::Before,
		})
		.unwrap();
	assert_eq!(snapshot.geometry_extract_text(word, 1), "word");
	let block = snapshot
		.geometry_select_block_at(GeometryPosition {
			revision: 1,
			block: 1,
			node: 0,
			offset: 3,
			affinity: Affinity::Before,
		})
		.unwrap();
	assert_eq!(
		snapshot.geometry_extract_text(block, 1),
		"Second paragraph."
	);
}
/// Extracts the double-click selection at `offset` in the first block.
fn word_at(source: &str, offset: usize, affinity: Affinity) -> String {
	let snapshot = layout(source, 400.0);
	let selection = snapshot
		.geometry_select_word_at(GeometryPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset,
			affinity,
		})
		.expect("a word selection");
	snapshot.geometry_extract_text(selection, 1)
}
#[test]
fn double_click_uses_dictionary_segmentation_for_cjk() {
	// 中文文字: dictionary words are 中文 and 文字, not four characters.
	assert_eq!(word_at("中文文字", 3, Affinity::Before), "中文");
	assert_eq!(word_at("中文文字", 6, Affinity::Before), "文字");
	assert_eq!(word_at("中文文字", 9, Affinity::After), "文字");
	// The half of the character under the pointer picks the boundary side.
	assert_eq!(word_at("中文文字", 6, Affinity::After), "中文");
	// Japanese mixes dictionary words with single-character particles.
	assert_eq!(word_at("国際化と日本語", 6, Affinity::Before), "化");
	assert_eq!(word_at("国際化と日本語", 12, Affinity::Before), "日本語");
}
#[test]
fn double_click_prefers_adjacent_word_over_whitespace() {
	// 中文 测试: a hit inside the gap takes the word before it.
	assert_eq!(word_at("中文 测试", 6, Affinity::Before), "中文");
	assert_eq!(word_at("中文 测试", 7, Affinity::Before), "测试");
	assert_eq!(word_at("中文 测试", 7, Affinity::After), "中文");
	// Punctuation and emoji are their own selectable cluster.
	assert_eq!(word_at("Hello, world!", 5, Affinity::After), "Hello");
	assert_eq!(word_at("Hello, world!", 5, Affinity::Before), ",");
	assert_eq!(word_at("Hello 👩‍💻 world", 6, Affinity::Before), "👩‍💻");
}
#[test]
fn double_click_never_splits_a_grapheme() {
	let snapshot = layout("Cafe\u{301} shop", 400.0);
	let selection = snapshot
		.geometry_select_word_at(GeometryPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: 4,
			affinity: Affinity::Before,
		})
		.unwrap();
	assert_eq!(snapshot.geometry_extract_text(selection, 1), "Cafe\u{301}");
}
#[test]
fn pointer_hit_inside_a_cjk_word_selects_that_word() {
	let snapshot = layout("中文文字", 400.0);
	let block = &snapshot.blocks[0];
	let horizontal = HashMap::new();
	// Click the right half of the first glyph of 文字.
	let cluster = block.layout.text[0]
		.clusters
		.iter()
		.find(|c| c.range.start >= 6)
		.unwrap();
	let position = snapshot
		.geometry_hit_test_text(
			cluster.rect.x + cluster.rect.w * 0.75,
			block.y + cluster.rect.y + cluster.rect.h * 0.5,
			&horizontal,
			1,
		)
		.unwrap();
	let selection = snapshot.geometry_select_word_at(position).unwrap();
	assert_eq!(snapshot.geometry_extract_text(selection, 1), "文字");
	// The highlight covers exactly the two glyphs of the word.
	let word_clusters: Vec<_> = block.layout.text[0]
		.clusters
		.iter()
		.filter(|c| c.range.start >= 6 && c.range.end <= 12)
		.collect();
	let rects = snapshot.geometry_selection_rects(selection, &horizontal, 1);
	assert_eq!(rects.len(), word_clusters.len());
	for (rect, cluster) in rects.iter().zip(word_clusters) {
		assert!((rect.x - cluster.rect.x).abs() < 0.01);
		assert!((rect.w - cluster.rect.w).abs() < 0.01);
	}
}
#[test]
fn selection_survives_reflow_and_repeated_blocks_are_distinct() {
	let doc = markview_commonmark::parse(
		"A repeated paragraph with internationalization and 中文文字.\n\nA repeated paragraph with internationalization and 中文文字.",
	);
	let mut engine = LayoutEngine::new();
	let wide = engine.layout(&doc, &LayoutOptions::default());
	let selection = GeometrySelection {
		anchor: GeometryPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: 2,
			affinity: Affinity::Before,
		},
		focus: GeometryPosition {
			revision: 1,
			block: 1,
			node: 0,
			offset: 10,
			affinity: Affinity::After,
		},
	};
	let narrow = engine.layout(
		&doc,
		&LayoutOptions {
			width: 120.0,
			font_size: 23.0,
			..Default::default()
		},
	);
	assert_eq!(
		wide.geometry_extract_text(selection, 1),
		narrow.geometry_extract_text(selection, 1)
	);
	assert_eq!(
		wide.geometry_extract_text(wide.geometry_select_all(1).unwrap(), 1),
		narrow.geometry_extract_text(narrow.geometry_select_all(1).unwrap(), 1)
	);
	assert!(
		!narrow
			.geometry_selection_rects(selection, &HashMap::new(), 1)
			.is_empty()
	);
}
#[test]
fn hit_testing_respects_graphemes_tabs_and_overflow_clip() {
	let snapshot = layout(
		"Cafe\u{301} 👨‍👩‍👧 中文 office\n\n```\n\t012345678901234567890123456789012345678901234567890\n```",
		130.0,
	);
	let node = &snapshot.blocks[0].layout.text[0];
	let boundaries: Vec<_> = node
		.text
		.grapheme_indices(true)
		.map(|(i, _)| i)
		.chain([node.text.len()])
		.collect();
	for cluster in &node.clusters {
		assert!(boundaries.contains(&cluster.range.start));
		assert!(boundaries.contains(&cluster.range.end));
	}
	let tab = &snapshot.blocks[1].layout.text[0].clusters.at(0);
	assert_eq!(tab.range, 0..1);
	let mut horizontal = HashMap::new();
	horizontal.insert((1, 0), 80.0);
	let all = snapshot.geometry_select_all(1).unwrap();
	let overflow = &snapshot.blocks[1].layout.overflow[0];
	for rect in snapshot
		.geometry_selection_rects(all, &horizontal, 1)
		.iter()
		.filter(|r| r.y >= snapshot.blocks[1].y)
	{
		assert!(
			rect.x >= overflow.rect.x
				&& rect.x + rect.w <= overflow.rect.x + overflow.rect.w + 0.01
		);
	}
	let c = &node.clusters.at(0);
	let hit = snapshot
		.geometry_hit_test_text(
			c.rect.x + 0.1,
			c.rect.y + c.rect.h / 2.0,
			&horizontal,
			1,
		)
		.unwrap();
	assert_eq!(hit.offset, 0);
}
#[test]
fn hit_testing_prunes_far_blocks_but_keeps_the_nearest_cluster() {
	let source: String = (0..400)
		.map(|i| format!("Paragraph {i} with some words.\n\n"))
		.collect();
	let snapshot = layout(&source, 400.0);
	assert!(snapshot.blocks.len() > 300);
	let none = HashMap::new();
	let first = &snapshot.blocks[0];
	let cluster = &first.layout.text[0].clusters.at(0);
	let hit = snapshot
		.geometry_hit_test_text(
			cluster.rect.x + 0.5,
			first.y + cluster.rect.y + cluster.rect.h * 0.5,
			&none,
			1,
		)
		.unwrap();
	assert_eq!(hit.block, 0);
	let index = snapshot.blocks.len() - 1;
	let last = &snapshot.blocks[index];
	let cluster = &last.layout.text[0].clusters.at(0);
	let hit = snapshot
		.geometry_hit_test_text(
			cluster.rect.x + 0.5,
			last.y + cluster.rect.y + cluster.rect.h * 0.5,
			&none,
			1,
		)
		.unwrap();
	assert_eq!(hit.block, index);
	assert_eq!(hit.offset, 0);
}
#[test]
fn formula_mapping_does_not_depend_on_success_and_hyphens_are_visual_only() {
	let snapshot = layout(
		"$x^2$ $\\notacommand{x}$ internationalization representation",
		90.0,
	);
	let node = &snapshot.blocks[0].layout.text[0];
	assert_eq!(
		snapshot
			.geometry_extract_text(snapshot.geometry_select_all(1).unwrap(), 1),
		"x^2 \\notacommand{x} [Math error: ParseError at position 0: Undefined control sequence: \\notacommand] internationalization representation"
	);
	assert!(node.clusters.iter().any(|c| c.range == (0..3)));
	for cluster in &node.clusters {
		assert!(cluster.range.end <= node.text.len());
	}
}

#[test]
fn compact_boundaries_preserve_crlf_and_unicode_graphemes() {
	for text in ["", "a\r\nb", "Cafe\u{301}", "👩‍💻", "中文\u{00ad}abc", "אבג"]
	{
		let boundaries = super::GraphemeBoundaries::new(text);
		let expected: Vec<_> = text
			.grapheme_indices(true)
			.map(|(i, _)| i)
			.chain(std::iter::once(text.len()))
			.collect();
		assert_eq!(
			(0..boundaries.len())
				.map(|i| boundaries.at(i))
				.collect::<Vec<_>>(),
			expected
		);
		assert!(matches!(boundaries, super::GraphemeBoundaries::Compact(_)));
	}
	let last = (u32::MAX as usize).saturating_add(1);
	let wide = super::GraphemeBoundaries::Wide(vec![0, last].into());
	assert_eq!(wide.at(1), last);
	assert_eq!(wide.partition_point(|n| n < last), 1);
}

#[test]
fn coalesced_search_maps_preserve_affine_and_generated_boundaries() {
	let mut node = super::TextNode::new("abcdefghijklmnopqrst".into(), "");
	node.search_ranges = vec![
		(0..2, 0..2),
		(2..4, 2..4),
		(4..4, 4..5),
		(4..7, 5..7),
		(7..9, 7..9),
		(9..11, 9..11),
	]
	.into();
	let before = node.clone();
	node.seal();
	assert_eq!(
		&**node.search_ranges,
		&[(0..4, 0..4), (4..4, 4..5), (4..7, 5..7), (7..11, 7..11)]
	);
	for offset in 0..=20 {
		assert_eq!(
			super::semantic_to_reading(&node, offset),
			super::semantic_to_reading(&before, offset)
		);
		assert_eq!(
			super::reading_to_semantic(&node, offset),
			super::reading_to_semantic(&before, offset)
		);
	}
}
