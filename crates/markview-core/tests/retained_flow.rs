use markview_core::{
	document::{self, BlockKind},
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions},
	search::{SearchIndex, SearchOptions},
	text::TextCounts,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 300.,
		fonts: FontConfig::from_faces(
			0x666c6f77,
			vec![parley::fontique::Blob::new(Arc::new(
				include_bytes!("fonts/NotoSerif-Regular-subset.otf").as_slice(),
			))],
		),
		..Default::default()
	}
}

#[test]
fn nested_toggles_keep_geometry_reading_positions_and_hidden_hits_isolated() {
	let document = document::parse(
		"<details open>\n<summary>Outer</summary>\n\n<details>\n<summary>Inner</summary>\n\nHidden [target](https://example.invalid).\n\n</details>\n\nAfter inner.\n\n</details>\n\n# Following\n",
	);
	let BlockKind::Details { blocks, .. } = &document.blocks[0].kind else {
		panic!("details");
	};
	let inner = blocks[0].id;
	let mut snapshot = LayoutEngine::new().layout(&document, &options());
	let geometry = snapshot.blocks[0].layout.clone();
	let full = snapshot.full_reading_text();
	assert!(full.contains("Hidden target."));
	let hit = SearchIndex::new(&document)
		.find("target", SearchOptions::default())
		.remove(0);
	assert!(snapshot.search_selection(&hit, 1).is_none());
	let closed_height = snapshot.height;
	let following = snapshot.anchor_y("following").unwrap();
	snapshot.set_disclosures(&BTreeMap::from([(inner, true)]), false);
	assert!(Arc::ptr_eq(&geometry, &snapshot.blocks[0].layout));
	assert!(snapshot.height > closed_height);
	assert!(snapshot.anchor_y("following").unwrap() > following);
	let selection = snapshot.search_selection(&hit, 1).unwrap();
	assert_eq!(snapshot.extract_text(selection, 1), "target");
	let rect = snapshot.selection_rects(selection, &Default::default(), 1)[0];
	assert_eq!(
		snapshot.link_at(
			rect.x + 1.,
			rect.y + rect.h * 0.5,
			&Default::default()
		),
		Some("https://example.invalid")
	);
	snapshot.set_disclosures(&BTreeMap::new(), false);
	assert!(
		snapshot
			.selection_rects(selection, &Default::default(), 1)
			.is_empty()
	);
	assert!(snapshot.extract_text(selection, 1).is_empty());
	assert_eq!(snapshot.full_reading_text(), full);
	assert_eq!(snapshot.height, closed_height);
	snapshot.set_disclosures(&BTreeMap::from([(inner, true)]), false);
	assert_eq!(snapshot.search_selection(&hit, 1), Some(selection));
}

#[test]
fn complete_counts_include_closed_bodies_but_selection_copies_visible_text() {
	let document = document::parse(
		"Before.\n\n<details>\n<summary>Summary</summary>\n\nHidden words.\n\n</details>\n\nAfter.\n",
	);
	let mut snapshot = LayoutEngine::new().layout(&document, &options());
	assert_eq!(TextCounts::of(&snapshot.full_reading_text()).words, 5);
	let visible = snapshot.extract_text(snapshot.select_all(1).unwrap(), 1);
	assert!(!visible.contains("Hidden"));
	snapshot.set_disclosures(&BTreeMap::new(), true);
	assert_eq!(TextCounts::of(&snapshot.full_reading_text()).words, 5);
	assert!(
		snapshot
			.extract_text(snapshot.select_all(1).unwrap(), 1)
			.contains("Hidden words.")
	);
}

#[test]
fn presentation_changes_continue_an_existing_progressive_pass() {
	let document = document::parse(
		"Before.\n\n<details>\n<summary>Summary</summary>\n\nHidden words.\n\n</details>\n\nAfter.\n",
	);
	let mut engine = LayoutEngine::new();
	let mut pass =
		engine.begin_layout(&document, &options(), &Default::default());
	engine.advance(&mut pass, &document, Duration::ZERO);
	let id = pass.pass_id();
	let geometry = pass.snapshot().blocks[0].layout.clone();
	pass.set_disclosures(
		Arc::new(BTreeMap::from([(document.blocks[1].id, true)])),
		false,
	);
	engine.advance(&mut pass, &document, Duration::MAX);
	assert_eq!(pass.pass_id(), id);
	assert!(Arc::ptr_eq(&geometry, &pass.snapshot().blocks[0].layout));
	let snapshot = pass.into_snapshot();
	assert!(
		snapshot
			.extract_text(snapshot.select_all(1).unwrap(), 1)
			.contains("Hidden words.")
	);
}

#[test]
fn footnote_markers_follow_visible_baselines_through_nested_disclosures() {
	for tail in ["", "\n    Visible paragraph.\n"] {
		let document = document::parse(format!(
			"Text[^a] and next[^b].\n\n[^a]: <details>\n    <summary></summary>\n\n    <details>\n    <summary></summary>\n\n    Hidden paragraph.\n\n    </details>\n\n    </details>\n{tail}\n[^b]: Next note.\n"
		));
		let BlockKind::Footnote { blocks, .. } = &document.blocks[1].kind
		else {
			panic!("footnote");
		};
		let outer = blocks[0].id;
		let BlockKind::Details { blocks, .. } = &blocks[0].kind else {
			panic!("outer disclosure");
		};
		let inner = blocks[0].id;
		let mut snapshot = LayoutEngine::new().layout(&document, &options());
		let geometry = snapshot.blocks[1].layout.clone();
		for (open, force) in [
			(BTreeMap::new(), false),
			(BTreeMap::from([(outer, true)]), false),
			(BTreeMap::from([(outer, true), (inner, true)]), false),
			(BTreeMap::new(), true),
			(BTreeMap::new(), false),
		] {
			snapshot.set_disclosures(&open, force);
			let note = &snapshot.blocks[1];
			assert!(Arc::ptr_eq(&geometry, &note.layout));
			let (backlink, rect) = note
				.links()
				.find(|(link, _)| link.url.starts_with("#fnback:"))
				.unwrap();
			let body_baseline =
				note.draws().find_map(|(command, draw, offset)| {
					if command < backlink.command
						&& let markview_core::scene::Draw::Glyph(glyph) = draw
					{
						Some(glyph.y + offset[1])
					} else {
						None
					}
				});
			let (label, offset) = note
				.draws()
				.find_map(|(command, draw, offset)| {
					if command >= backlink.command
						&& let markview_core::scene::Draw::Glyph(glyph) = draw
					{
						Some((glyph, offset))
					} else {
						None
					}
				})
				.unwrap();
			let expected = body_baseline.unwrap_or(label.size * 1.15);
			assert!((label.y + offset[1] - expected).abs() < 0.01);
			assert!(rect.y >= 0. && rect.y + rect.h <= note.height());
			assert_eq!(
				snapshot.link_at(
					rect.x + rect.w * 0.5,
					note.y + rect.y + rect.h * 0.5,
					&Default::default()
				),
				Some(backlink.url.as_ref())
			);
			let flat = snapshot.flattened();
			let flat_note = &flat.blocks[1];
			let (_, flat_rect) = flat_note
				.links()
				.find(|(link, _)| link.url == backlink.url)
				.unwrap();
			assert!((flat_rect.y - rect.y).abs() < 0.01);
		}
	}
}

#[test]
fn cancellation_interrupts_a_large_closed_container_before_publication() {
	let source = format!(
		"<details>\n<summary>Summary</summary>\n\n{}</details>\n",
		"Another paragraph.\n\n".repeat(100)
	);
	let document = document::parse(source);
	let checks = std::cell::Cell::new(0);
	let mut engine = LayoutEngine::new();
	let result = engine.layout_progressive_cancellable(
		&document,
		&options(),
		&Default::default(),
		|prefix| {
			assert!(prefix.blocks.is_empty());
			true
		},
		|| {
			checks.set(checks.get() + 1);
			checks.get() >= 20
		},
	);
	assert!(result.is_none());
	let snapshot = engine.layout(&document, &options());
	assert_eq!(snapshot.reused, 0);
	assert_eq!(
		snapshot
			.full_reading_text()
			.matches("Another paragraph.")
			.count(),
		100
	);
}
