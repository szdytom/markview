//! Selection geometry over the committed subset faces: a ligature is one span
//! of ink, so it has to be one span of selection too.
use markview_core::{
	document,
	fonts::FontConfig,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
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

#[test]
fn a_ligature_is_one_selection_span() {
	let snapshot = layout("The file");
	let node = &snapshot.blocks[0].layout.text[0];
	let ligature = node
		.clusters
		.iter()
		.find(|c| c.range == (4..6))
		.expect("the pinned serif sets `fi` as one cluster");
	assert_eq!(node.text.get(ligature.range.clone()), Some("fi"));
	// The clusters of a line tile it: a rect covering only half the ligature
	// would leave a gap before the next cluster.
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
	// Selecting the line highlights every cluster over its whole advance, the
	// ligature included.
	let all = snapshot.select_all(1).unwrap();
	let rects = snapshot.selection_rects(all, &Default::default(), 1);
	assert_eq!(rects.len(), node.clusters.len());
	for (rect, cluster) in rects.iter().zip(&node.clusters) {
		assert!((rect.x - cluster.rect.x).abs() < 0.01);
		assert!((rect.w - cluster.rect.w).abs() < 0.01);
	}
	// A ligature stays one character span for copying and for the caret.
	assert_eq!(snapshot.extract_text(all, 1), "The file");
	let hit = snapshot
		.hit_test_text(
			ligature.rect.x + ligature.rect.w * 0.5,
			snapshot.blocks[0].y + ligature.rect.y + ligature.rect.h * 0.5,
			&Default::default(),
			1,
		)
		.unwrap();
	assert!(hit.offset == 4 || hit.offset == 6, "{:?}", hit);
}
