//! Real layout, then pagination driven by the laid-out content height.
use crate::{coverage, oracle, page_oracle, pipeline};
use markview_layout::{
	layout::LayoutOptions,
	paginate::{PT_PER_PX, PageGeometry, paginate},
};

/// Markdown up to 16 KiB; the page height is derived from the real snapshot so
/// a short document still reaches several pages when the input chooses so.
pub fn check(data: &[u8]) {
	if data.len() > 16 * 1024 {
		return;
	}
	let md = String::from_utf8_lossy(data);
	let document = markview_commonmark::parse(md.to_string());
	let mut geometry = PageGeometry {
		width_pt: 300.,
		height_pt: 400.,
		margin_pt: [10.; 4],
	};
	let options = LayoutOptions {
		width: geometry.text_px().0,
		force_open: data.first().copied().unwrap_or(0) % 3 != 0,
		codeblock_wrap: data.get(1).copied().unwrap_or(0) % 2 == 0,
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut engine = pipeline::differential_engine();
	let snapshot = engine.layout(&document, &options).flattened();
	let pages = 2 + usize::from(data.get(2).copied().unwrap_or(0) % 3);
	let text_px = (snapshot.height / pages as f32).max(80.);
	geometry.height_pt = text_px * PT_PER_PX + 20.;
	let pagination = paginate(&document, &snapshot, &geometry);
	page_oracle::assert_pagination(&pagination);
	// A document shorter than one page may legitimately stay on one page.
	let visible: std::collections::HashSet<_> = snapshot
		.blocks
		.iter()
		.filter(|block| {
			block
				.layout
				.text
				.iter()
				.any(|node| !node.text.trim().is_empty())
		})
		.enumerate()
		.map(|(index, _)| index)
		.collect();
	let placed: std::collections::HashSet<_> = pagination
		.pages
		.iter()
		.flatten()
		.map(|item| item.block)
		.collect();
	assert!(
		visible.is_subset(&placed),
		"a visible block produced no page fragment"
	);
	coverage::hit(coverage::Hit::PaginatePages);
	coverage::hit(coverage::Hit::WebMultipage);
	oracle::assert_snapshot_finite(&snapshot);
}
