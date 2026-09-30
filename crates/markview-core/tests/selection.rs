//! Selection geometry over the committed subset faces: a cluster is a unit of
//! shaping, not of reading, so a ligature setting several letters as one glyph
//! still lets the pointer land between them, while a single grapheme is never
//! parted.
// These shape with the committed subset faces, so they need a filesystem to
// read them from.
#![cfg(feature = "font-directories")]

use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
	text::Affinity,
};

/// The pinned serif substitutes an `fi` ligature, and pinning the faces keeps
/// the geometry independent of the host's fonts.
fn fonts() -> FontConfig {
	FontConfig {
		ignore_system_fonts: true,
		directories: vec![
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts"),
		],
		..Default::default()
	}
}

fn layout(source: &str) -> LayoutSnapshot {
	LayoutEngine::new().layout(
		&document::parse(source),
		&LayoutOptions {
			width: 400.0,
			fonts: fonts(),
			..Default::default()
		},
	)
}

/// A selection marks only the reading text it names. Every cluster the
/// selection never reaches draws nothing, which is what keeps a partial
/// selection from lighting up the rest of the document.
#[test]
fn a_partial_selection_marks_nothing_it_does_not_cover() {
	let snapshot =
		layout("First block here.\n\nSecond block here.\n\nThird block here.");
	assert_eq!(snapshot.blocks.len(), 3, "three paragraphs");
	let mut partial = snapshot.select_all(1).unwrap();
	partial.focus.block = 0;
	partial.focus.node = 0;
	partial.focus.offset = 5;
	assert_eq!(snapshot.extract_text(partial, 1), "First");

	let rects = snapshot.selection_rects(partial, &Default::default(), 1);
	assert!(!rects.is_empty(), "the covered text is marked");
	let first = &snapshot.blocks[0];
	let band = first.y..first.y + first.layout.height;
	for rect in &rects {
		assert!(
			band.contains(&rect.y),
			"a highlight at y {} falls outside the first block, which spans {band:?}",
			rect.y
		);
	}
}

#[test]
fn a_ligature_is_picked_apart_at_its_letters() {
	let snapshot = layout("The file");
	let node = &snapshot.blocks[0].layout.text[0];
	let ligature = node
		.clusters
		.iter()
		.find(|c| c.range == (4..6))
		.expect("the pinned serif sets `fi` as one cluster");
	assert_eq!(node.text.get(ligature.range.clone()), Some("fi"));
	// The clusters of a line still tile it over their whole advance. What
	// gets cut short is the selection, not the ink.
	for pair in node.clusters.windows(2) {
		let (a, b) = (&pair[0], &pair[1]);
		assert!(
			(a.rect.x + a.rect.w - b.rect.x).abs() < 0.01,
			"{:?} ends at {} but {:?} starts at {}",
			a.range,
			a.rect.x + a.rect.w,
			b.range,
			b.rect.x
		);
	}
	// A pointer inside the ligature lands on the letter it is nearest, so the
	// `f` can be taken without the `i`.
	let line = snapshot.blocks[0].y + ligature.rect.y + ligature.rect.h * 0.5;
	let hit = |fraction: f32| {
		snapshot
			.hit_test_text(
				ligature.rect.x + ligature.rect.w * fraction,
				line,
				&Default::default(),
				1,
			)
			.unwrap()
			.offset
	};
	assert_eq!(hit(0.1), 4, "before the letters, the ligature's own start");
	assert_eq!(hit(0.5), 5, "the two letters meet in the middle");
	assert_eq!(hit(0.9), 6, "past the letters, the next cluster");
	// Stopping in the middle highlights only the left letter's share of the
	// advance, and copies only the letters it reached.
	let mut partial = snapshot.select_all(1).unwrap();
	partial.focus.offset = 5;
	assert_eq!(snapshot.extract_text(partial, 1), "The f");
	let rects = snapshot.selection_rects(partial, &Default::default(), 1);
	let last = rects.last().expect("the half-covered ligature is drawn");
	assert!(
		(last.w - ligature.rect.w * 0.5).abs() < 0.01,
		"half the ligature's advance, not {} of {}",
		last.w,
		ligature.rect.w
	);
	// Selecting the line whole is unchanged: every cluster over its whole
	// advance, the ligature included.
	let all = snapshot.select_all(1).unwrap();
	assert_eq!(snapshot.extract_text(all, 1), "The file");
	let rects = snapshot.selection_rects(all, &Default::default(), 1);
	assert_eq!(rects.len(), node.clusters.len());
	for (rect, cluster) in rects.iter().zip(&node.clusters) {
		assert!((rect.x - cluster.rect.x).abs() < 0.01);
		assert!((rect.w - cluster.rect.w).abs() < 0.01);
	}
}

/// A formula reads as its source but draws as one box, so the pointer only
/// ever lands on either of its edges, never on a fragment of the LaTeX.
#[test]
fn a_formula_is_one_atomic_box_the_pointer_cannot_split() {
	let snapshot = layout("$\\frac{a}{b}$");
	let node = &snapshot.blocks[0].layout.text[0];
	let formula = node
		.clusters
		.iter()
		.find(|c| c.atomic)
		.expect("a formula cluster");
	assert_eq!(node.text.get(formula.range.clone()), Some("\\frac{a}{b}"));
	let line = snapshot.blocks[0].y + formula.rect.y + formula.rect.h * 0.5;
	let ends = [formula.range.start, formula.range.end];
	for step in 0..=20 {
		let x = formula.rect.x + formula.rect.w * (step as f32 / 20.0);
		let hit = snapshot
			.hit_test_text(x, line, &Default::default(), 1)
			.expect("a hit inside the formula");
		assert!(
			ends.contains(&hit.offset),
			"a hit at {x} landed on {}, inside the formula",
			hit.offset
		);
	}
	// A double click takes the whole formula, not a word of its LaTeX.
	let hit = snapshot
		.hit_test_text(
			formula.rect.x + formula.rect.w * 0.5,
			line,
			&Default::default(),
			1,
		)
		.unwrap();
	let word = snapshot.select_word_at(hit).expect("a word selection");
	assert_eq!(snapshot.extract_text(word, 1), "\\frac{a}{b}");
}

/// Text pressed right against a formula keeps its own double click: the right
/// half of its last letter reports `After` at the formula's start, which is
/// the text's edge, not the formula's.
#[test]
fn double_click_before_a_formula_takes_the_preceding_word() {
	let snapshot = layout("hi$\\frac{a}{b}$");
	let node = &snapshot.blocks[0].layout.text[0];
	let last = node
		.clusters
		.iter()
		.find(|c| c.range == (1..2))
		.expect("the trailing `i`");
	// The right half of the `i`, so the hit snaps to its after edge.
	let hit = snapshot
		.hit_test_text(
			last.rect.x + last.rect.w * 0.75,
			snapshot.blocks[0].y + last.rect.y + last.rect.h * 0.5,
			&Default::default(),
			1,
		)
		.expect("a hit on the text");
	assert_eq!((hit.offset, hit.affinity), (2, Affinity::After));
	let word = snapshot.select_word_at(hit).expect("a word selection");
	assert_eq!(snapshot.extract_text(word, 1), "hi");
}

/// The selected share of a ligature is taken from the glyph's own advance,
/// not from whatever the overflow viewport left visible, so scrolling a table
/// sideways never moves the highlight onto the letter that is still onscreen.
#[test]
fn a_partially_selected_ligature_is_subdivided_before_it_is_clipped() {
	// One unbreakable word, wider than its column, so the line scrolls.
	let snapshot = LayoutEngine::new().layout(
		&document::parse("fileoffifi"),
		&LayoutOptions {
			width: 40.0,
			hyphenate: false,
			fonts: fonts(),
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0];
	let node = &block.layout.text[0];
	let ligature = node
		.clusters
		.iter()
		.find(|c| c.range == (0..2))
		.expect("the pinned serif sets `fi` as one cluster");
	let overflow = block
		.layout
		.overflow
		.first()
		.expect("the word overflows its column");
	let max = (overflow.content_width - overflow.rect.w).max(0.0);
	// Scroll until the clip's leading edge sits past the middle of the
	// ligature, so only its second letter is still visible.
	let scroll = (ligature.rect.x + ligature.rect.w * 0.6 - overflow.rect.x)
		.clamp(0.0, max);
	let mut horizontal = std::collections::HashMap::new();
	horizontal.insert((0, 0), scroll);
	// A selection that reaches only the `f`, whose ink is now offscreen.
	let mut partial = snapshot.select_all(1).unwrap();
	partial.focus.offset = 1;
	assert_eq!(snapshot.extract_text(partial, 1), "f");
	let rects = snapshot.selection_rects(partial, &horizontal, 1);
	assert!(
		rects.is_empty(),
		"the selected letter is offscreen, so nothing is marked: {rects:?}"
	);
}
