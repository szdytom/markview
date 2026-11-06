//! Page fragments must cover each block's bands once and in reading order.
use markview_layout::paginate::Pagination;
use std::collections::HashMap;

pub fn assert_pagination(pagination: &Pagination) {
	assert!(!pagination.pages.is_empty(), "pagination produced no page");
	for (index, (anchor, (page, y))) in pagination.anchors.iter().enumerate() {
		assert!(
			*page < pagination.pages.len() && y.is_finite(),
			"anchor {anchor:?} at {index} points outside the pages"
		);
	}
	for (target, (page, y)) in &pagination.targets {
		assert!(
			*page < pagination.pages.len() && y.is_finite(),
			"target {target:?} points outside the pages"
		);
	}
	let mut fragments: HashMap<usize, Vec<std::ops::Range<usize>>> =
		HashMap::new();
	for page in &pagination.pages {
		let mut last_y = f32::NEG_INFINITY;
		for item in page {
			assert!(
				item.y.is_finite()
					&& item.y >= 0. && item.scale.is_finite()
					&& item.scale > 0.
					&& item.top.is_finite()
					&& item.bottom.is_finite()
					&& item.bottom >= item.top,
				"page item is not a finite fragment: {item:?}"
			);
			assert!(!item.bands.is_empty(), "page item has no band: {item:?}");
			assert!(
				item.y >= last_y,
				"page items are not in y order: {item:?}"
			);
			last_y = item.y;
			fragments
				.entry(item.block)
				.or_default()
				.push(item.bands.clone());
		}
	}
	for (block, items) in fragments {
		assert_eq!(
			items[0].start, 0,
			"block {block} starts at band {} instead of 0",
			items[0].start
		);
		for pair in items.windows(2) {
			assert_eq!(
				pair[0].end, pair[1].start,
				"block {block} lost or repeated bands between {:?} and {:?}",
				pair[0], pair[1]
			);
		}
	}
	// `first` marks the first fragment and `last` the last one.
	let mut flags: HashMap<usize, Vec<(bool, bool)>> = HashMap::new();
	for page in &pagination.pages {
		for item in page {
			flags
				.entry(item.block)
				.or_default()
				.push((item.first, item.last));
		}
	}
	for (block, flags) in flags {
		for (index, (first, last)) in flags.iter().enumerate() {
			assert_eq!(
				*first,
				index == 0,
				"block {block} fragment {index} has the wrong first flag"
			);
			assert_eq!(
				*last,
				index + 1 == flags.len(),
				"block {block} fragment {index} has the wrong last flag"
			);
		}
	}
}
