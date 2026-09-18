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
use crate::{
	document,
	layout::{LayoutEngine, LayoutOptions},
};
fn layout(source: &str, width: f32) -> LayoutSnapshot {
	LayoutEngine::new().layout(
		&document::parse(source),
		&LayoutOptions {
			width,
			..Default::default()
		},
	)
}
#[test]
fn copies_reading_text_code_tables_and_atomic_math() {
	let snapshot = layout(
		"# Title\n\nA **bold** [link](https://example.com) $x^2$.\n\n```rust\n\tlet x = 1;\n\n```\n\n| A | B |\n|---|---|\n| 中 | 文 |\n\n- one\n- [x] two",
		300.0,
	);
	let text = snapshot.extract_text(snapshot.select_all(9).unwrap(), 9);
	assert_eq!(
		text,
		"Title\n\nA bold link x^2.\n\n\tlet x = 1;\n\n\n\nA\tB\n中\t文\none\ntwo"
	);
	assert!(
		snapshot
			.extract_text(snapshot.select_all(9).unwrap(), 10)
			.is_empty()
	);
}
#[test]
fn double_and_triple_click_ranges_follow_reading_text() {
	let snapshot = layout("First word here.\n\nSecond paragraph.", 300.0);
	let word = snapshot
		.select_word_at(TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: 8,
			affinity: Affinity::Before,
		})
		.unwrap();
	assert_eq!(snapshot.extract_text(word, 1), "word");
	let block = snapshot
		.select_block_at(TextPosition {
			revision: 1,
			block: 1,
			node: 0,
			offset: 3,
			affinity: Affinity::Before,
		})
		.unwrap();
	assert_eq!(snapshot.extract_text(block, 1), "Second paragraph.");
}
/// Extracts the double-click selection at `offset` in the first block.
fn word_at(source: &str, offset: usize, affinity: Affinity) -> String {
	let snapshot = layout(source, 400.0);
	let selection = snapshot
		.select_word_at(TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset,
			affinity,
		})
		.expect("a word selection");
	snapshot.extract_text(selection, 1)
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
		.select_word_at(TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: 4,
			affinity: Affinity::Before,
		})
		.unwrap();
	assert_eq!(snapshot.extract_text(selection, 1), "Cafe\u{301}");
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
		.hit_test_text(
			cluster.rect.x + cluster.rect.w * 0.75,
			block.y + cluster.rect.y + cluster.rect.h * 0.5,
			&horizontal,
			1,
		)
		.unwrap();
	let selection = snapshot.select_word_at(position).unwrap();
	assert_eq!(snapshot.extract_text(selection, 1), "文字");
	// The highlight covers exactly the two glyphs of the word.
	let word_clusters: Vec<_> = block.layout.text[0]
		.clusters
		.iter()
		.filter(|c| c.range.start >= 6 && c.range.end <= 12)
		.collect();
	let rects = snapshot.selection_rects(selection, &horizontal, 1);
	assert_eq!(rects.len(), word_clusters.len());
	for (rect, cluster) in rects.iter().zip(word_clusters) {
		assert!((rect.x - cluster.rect.x).abs() < 0.01);
		assert!((rect.w - cluster.rect.w).abs() < 0.01);
	}
}
#[test]
fn selection_survives_reflow_and_repeated_blocks_are_distinct() {
	let doc = document::parse(
		"A repeated paragraph with internationalization and 中文文字.\n\nA repeated paragraph with internationalization and 中文文字.",
	);
	let mut engine = LayoutEngine::new();
	let wide = engine.layout(&doc, &LayoutOptions::default());
	let selection = TextSelection {
		anchor: TextPosition {
			revision: 1,
			block: 0,
			node: 0,
			offset: 2,
			affinity: Affinity::Before,
		},
		focus: TextPosition {
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
		wide.extract_text(selection, 1),
		narrow.extract_text(selection, 1)
	);
	assert_eq!(
		wide.extract_text(wide.select_all(1).unwrap(), 1),
		narrow.extract_text(narrow.select_all(1).unwrap(), 1)
	);
	assert!(
		!narrow
			.selection_rects(selection, &HashMap::new(), 1)
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
	let tab = &snapshot.blocks[1].layout.text[0].clusters[0];
	assert_eq!(tab.range, 0..1);
	let mut horizontal = HashMap::new();
	horizontal.insert((1, 0), 80.0);
	let all = snapshot.select_all(1).unwrap();
	let overflow = &snapshot.blocks[1].layout.overflow[0];
	for rect in snapshot
		.selection_rects(all, &horizontal, 1)
		.iter()
		.filter(|r| r.y >= snapshot.blocks[1].y)
	{
		assert!(
			rect.x >= overflow.rect.x
				&& rect.x + rect.w <= overflow.rect.x + overflow.rect.w + 0.01
		);
	}
	let c = &node.clusters[0];
	let hit = snapshot
		.hit_test_text(
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
	let cluster = &first.layout.text[0].clusters[0];
	let hit = snapshot
		.hit_test_text(
			cluster.rect.x + 0.5,
			first.y + cluster.rect.y + cluster.rect.h * 0.5,
			&none,
			1,
		)
		.unwrap();
	assert_eq!(hit.block, 0);
	let index = snapshot.blocks.len() - 1;
	let last = &snapshot.blocks[index];
	let cluster = &last.layout.text[0].clusters[0];
	let hit = snapshot
		.hit_test_text(
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
		snapshot.extract_text(snapshot.select_all(1).unwrap(), 1),
		"x^2 \\notacommand{x} [Math error: ParseError at position 0: Undefined control sequence: \\notacommand] internationalization representation"
	);
	assert!(node.clusters.iter().any(|c| c.range == (0..3)));
	for cluster in &node.clusters {
		assert!(cluster.range.end <= node.text.len());
	}
}
