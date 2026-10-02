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

fn plain_options() -> LayoutOptions {
	LayoutOptions {
		width: 400.0,
		fonts: fonts(),
		..Default::default()
	}
}

fn layout(source: &str) -> LayoutSnapshot {
	LayoutEngine::new().layout(&document::parse(source), &plain_options())
}

#[test]
fn triple_click_selects_paragraphs_and_cells_inside_containers() {
	let cases: &[(&str, &[&str])] = &[
		(
			"- First paragraph.\n\n  Second paragraph.\n- Next item.",
			&["First paragraph.", "Second paragraph.", "Next item."],
		),
		(
			"- Outer paragraph.\n  - Nested paragraph.\n- Next item.",
			&["Outer paragraph.", "Nested paragraph.", "Next item."],
		),
		(
			"1. First paragraph.\n\n   Second paragraph.\n2. Next item.",
			&["1. First paragraph.", "Second paragraph.", "2. Next item."],
		),
		(
			"> First paragraph.\n>\n> Second paragraph.\n>\n> - Quoted item.\n> - Next item.",
			&[
				"First paragraph.",
				"Second paragraph.",
				"Quoted item.",
				"Next item.",
			],
		),
		(
			"| Header A | Header B |\n|---|---|\n| Cell **A** | Cell B |",
			&["Header A", "Header B", "Cell A", "Cell B"],
		),
		(
			"- [x] First item.\n- [ ] Next item.",
			&["First item.", "Next item."],
		),
		(
			"- First line.  \n  Second line.\n- Next item.",
			&["First line.\nSecond line.", "Next item."],
		),
		(
			"- Before \\[x+y\\] after.\n- Next item.",
			&["Before x+y after.", "Next item."],
		),
		(
			"| A | B |\n|---|---|\n| Before \\[x+y\\] after. | Next cell. |",
			&["A", "B", "Before x+y after.", "Next cell."],
		),
	];
	for &(source, expected) in cases {
		let snapshot = layout(source);
		assert_eq!(snapshot.blocks.len(), 1, "{source}");
		let placed = &snapshot.blocks[0];
		let mut selected = Vec::new();
		for node in &placed.layout.text {
			for cluster in &node.clusters {
				let hit = snapshot
					.hit_test_text(
						cluster.rect.x + cluster.rect.w * 0.5,
						placed.y + cluster.rect.y + cluster.rect.h * 0.5,
						&Default::default(),
						1,
					)
					.unwrap();
				let selection = snapshot.select_block_at(hit).unwrap();
				let text = snapshot.extract_text(selection, 1);
				assert!(
					expected.contains(&text.as_str()),
					"{source}: selected {text:?}"
				);
				if selected.last() != Some(&text) {
					selected.push(text);
				}
			}
		}
		assert_eq!(selected, expected, "{source}");
	}
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

/// Picking another family for the serif role moves the ink and nothing else:
/// the text, its cluster boundaries and so its selection and copy stay put.
#[test]
fn a_family_override_keeps_the_text_and_its_selection() {
	let source = "The quick brown fox\n\n- one\n- two\n";
	let plain = layout(source);
	let mut picked = (*plain_options().stylesheet).clone();
	picked
		.apply_font_overrides(&[("serif".to_owned(), "Noto Sans".to_owned())])
		.unwrap();
	let picked = LayoutEngine::new().layout(
		&document::parse(source),
		&LayoutOptions {
			stylesheet: std::sync::Arc::new(picked),
			..plain_options()
		},
	);
	// List markers are not part of the reading text, so both copies of the
	// document read the same either way.
	let text = "The quick brown fox\none\ntwo";
	assert_eq!(picked.extract_text(picked.select_all(1).unwrap(), 1), text);
	assert_eq!(plain.extract_text(plain.select_all(1).unwrap(), 1), text);
	let (plain_lines, picked_lines): (Vec<_>, Vec<_>) =
		(lines(&plain).collect(), lines(&picked).collect());
	assert_eq!(plain_lines.len(), picked_lines.len());
	let mut moved = 0;
	for (a, b) in plain_lines.iter().zip(&picked_lines) {
		assert_eq!(a.0, b.0, "the text of a line");
		assert_eq!(a.1, b.1, "the character spans a line breaks into");
		moved += a.2.iter().zip(&b.2).filter(|(x, y)| x != y).count();
	}
	assert!(moved > 0, "the family in force changed the geometry");
}

/// Every text node as its text, its cluster ranges and each cluster's x.
fn lines(
	snapshot: &LayoutSnapshot,
) -> impl Iterator<Item = (&str, Vec<std::ops::Range<usize>>, Vec<f32>)> {
	snapshot.blocks.iter().flat_map(move |block| {
		block.layout.text.iter().map(move |node| {
			(
				node.text.as_ref(),
				node.clusters.iter().map(|c| c.range.clone()).collect(),
				node.clusters.iter().map(|c| c.rect.x).collect(),
			)
		})
	})
}

#[test]
fn selection_endpoints_exclude_automatic_mixed_script_spacing() {
	for (source, selected, left, right) in [
		("MarkView测试文本 `Inline Code`", "测试", true, false),
		("测试MarkView", "测试", false, true),
		("a测b", "测", true, true),
		("MarkView 测试文本", " 测试", false, false),
	] {
		let options = LayoutOptions {
			justify: false,
			..plain_options()
		};
		let snapshot =
			LayoutEngine::new().layout(&document::parse(source), &options);
		let node = &snapshot.blocks[0].layout.text[0];
		let start = node.text.find(selected).unwrap();
		let end = start + selected.len();
		let mut selection = snapshot.select_all(1).unwrap();
		selection.anchor.offset = start;
		selection.focus.offset = end;
		assert_eq!(snapshot.extract_text(selection, 1), selected);
		let clusters: Vec<_> = node
			.clusters
			.iter()
			.filter(|c| c.range.start >= start && c.range.end <= end)
			.collect();
		let gap = options.font_size * 0.25;
		for reversed in [false, true] {
			let mut selection = selection;
			if reversed {
				std::mem::swap(&mut selection.anchor, &mut selection.focus);
			}
			let rects =
				snapshot.selection_rects(selection, &Default::default(), 1);
			let first = rects.first().unwrap();
			let last = rects.last().unwrap();
			let expected_start =
				clusters[0].rect.x + if left { gap } else { 0.0 };
			let final_cluster = clusters.last().unwrap();
			let expected_end = final_cluster.rect.x + final_cluster.rect.w
				- if right { gap } else { 0.0 };
			assert!(
				(first.x - expected_start).abs() < 0.01,
				"{source}: {rects:?}"
			);
			assert!(
				(last.x + last.w - expected_end).abs() < 0.01,
				"{source}: {rects:?}"
			);
		}
		// Selecting both scripts keeps the intervening spacing covered.
		let rects = snapshot.selection_rects(
			snapshot.select_all(1).unwrap(),
			&Default::default(),
			1,
		);
		for pair in rects.windows(2) {
			assert!(
				(pair[0].x + pair[0].w - pair[1].x).abs() < 0.01,
				"{source}"
			);
		}
	}
}
