use markview_core::layout::{
	BlockLayout, LayoutSnapshot, PlacedBlock, anchored_scroll, scroll_limit,
};
use std::sync::Arc;

fn snapshot(ids: impl IntoIterator<Item = u64>) -> LayoutSnapshot {
	let layout = Arc::new(BlockLayout {
		height: 120.0,
		..Default::default()
	});
	let blocks: Vec<_> = ids
		.into_iter()
		.enumerate()
		.map(|(i, id)| PlacedBlock {
			flow: Default::default(),
			id,
			source: 0..0,
			y: i as f32 * 120.0,
			layout: layout.clone(),
		})
		.collect();
	LayoutSnapshot {
		height: blocks.len() as f32 * 120.0,
		blocks,
		..Default::default()
	}
}

#[test]
fn repeated_anchor_keeps_its_occurrence_and_local_offset() {
	let old = snapshot([1, 2, 1, 3, 4]);
	let new = snapshot([0, 1, 2, 0, 1, 3, 4]);
	assert_eq!(anchored_scroll(&old, &new, 250.0, 200.0, false), 490.0);
	// If that occurrence disappears, the surviving previous block anchors us.
	let new = snapshot([0, 1, 2, 3, 4]);
	assert_eq!(anchored_scroll(&old, &new, 250.0, 200.0, false), 370.0);
}

#[test]
fn missing_anchor_uses_the_nearest_neighbor_and_its_first_occurrence() {
	let old = snapshot([1, 2, 3, 4, 5]);
	for (ids, expected) in [
		// Equidistant neighbors prefer the preceding block.
		(vec![0, 4, 2, 5, 1], 370.0),
		// A following neighbor wins over a more distant preceding block.
		(vec![0, 0, 4, 0, 1], 130.0),
		// Fallback retains the first occurrence, even for a repeated neighbor.
		(vec![0, 2, 0, 2, 4], 250.0),
		// A surviving block can be several positions away.
		(vec![0, 0, 0, 5, 0], 130.0),
		// Neighbor offsets still clamp to the new document's scroll bounds.
		(vec![4, 0, 0], 0.0),
		(vec![0, 0, 1], scroll_limit(360.0, 200.0)),
		(vec![], 0.0),
	] {
		let new = snapshot(ids.clone());
		assert_eq!(
			anchored_scroll(&old, &new, 250.0, 200.0, false),
			expected,
			"{ids:?}"
		);
	}
}

#[test]
fn large_replacements_without_shared_blocks_preserve_or_clamp_scroll() {
	let old = snapshot(0..50_000);
	let new = snapshot(50_000..100_000);
	let scroll = 3_000_010.0;
	assert_eq!(anchored_scroll(&old, &new, scroll, 200.0, false), scroll);
	let shorter = snapshot(50_000..70_000);
	assert_eq!(
		anchored_scroll(&old, &shorter, scroll, 200.0, false),
		scroll_limit(shorter.height, 200.0)
	);
}
