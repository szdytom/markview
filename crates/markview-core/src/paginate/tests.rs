use super::*;
use crate::{
	document,
	image::{ImageInfo, ImageSnapshot},
	layout::{LayoutEngine, LayoutOptions},
	shaping::TextShaper,
	style::Stylesheet,
};

fn layout(source: &str, width: f32) -> (Document, LayoutSnapshot) {
	layout_with(source, width, &Default::default())
}

fn layout_with(
	source: &str,
	width: f32,
	images: &ImageSnapshot,
) -> (Document, LayoutSnapshot) {
	let document = document::parse(source);
	let options = LayoutOptions {
		width,
		codeblock_wrap: true,
		..Default::default()
	};
	let snapshot =
		LayoutEngine::new().layout_with_images(&document, &options, images);
	(document, snapshot)
}

/// A page whose text area is exactly `height` pixels tall.
fn geometry(height: f32) -> PageGeometry {
	let margin = 40.0;
	PageGeometry {
		width_pt: 500.0,
		height_pt: height * PT_PER_PX + margin * 2.0,
		margin_pt: [margin; 4],
	}
}

/// The items one block occupies, page by page, in reading order.
fn items_of(pagination: &Pagination, block: usize) -> Vec<(usize, PageItem)> {
	pagination
		.pages
		.iter()
		.enumerate()
		.flat_map(|(page, items)| {
			items
				.iter()
				.filter(|item| item.block == block)
				.map(move |item| (page, item.clone()))
		})
		.collect()
}

fn assert_fragments_are_sound(
	pagination: &Pagination,
	geometry: &PageGeometry,
) {
	let (_, content) = geometry.text_px();
	for (page, items) in pagination.pages.iter().enumerate() {
		for item in items {
			assert!(item.bands.start < item.bands.end, "empty fragment");
			let height = (item.bottom - item.top) * item.scale;
			assert!(
				item.y + height <= content + 0.5,
				"page {page} fragment overflows: {:.1} + {:.1} > {content}",
				item.y,
				height
			);
		}
	}
}

#[test]
fn a_long_paragraph_splits_between_lines_without_losing_any() {
	let (document, snapshot) = layout(&"word ".repeat(1_200), 300.0);
	let geometry = geometry(200.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	assert!(pagination.pages.len() > 2, "{}", pagination.pages.len());
	assert_fragments_are_sound(&pagination, &geometry);
	let items = items_of(&pagination, 0);
	let mut expected = 0;
	for (_, item) in &items {
		assert_eq!(item.bands.start, expected, "bands must stay contiguous");
		expected = item.bands.end;
		// Two lines stay together on each side of a break.
		assert!(item.bands.len() >= 2, "{:?}", item.bands);
	}
	let firsts = items.iter().filter(|(_, item)| item.first).count();
	let lasts = items.iter().filter(|(_, item)| item.last).count();
	assert_eq!(firsts, 1, "one fragment starts the block");
	assert_eq!(lasts, 1, "one fragment ends the block");
	let total: usize = items.iter().map(|(_, item)| item.bands.len()).sum();
	assert!(total > 10, "the paragraph should span many lines: {total}");
}

#[test]
fn a_split_paragraph_keeps_two_lines_on_each_side() {
	let source = format!(
		"{}\n\n{}",
		"filler ".repeat(600),
		"one two three four five six seven eight nine ten eleven twelve \
		 thirteen fourteen fifteen sixteen seventeen eighteen"
	);
	let (document, snapshot) = layout(&source, 300.0);
	let geometry = geometry(200.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	assert_fragments_are_sound(&pagination, &geometry);
	for block in 0..document.blocks.len() {
		let items = items_of(&pagination, block);
		if items.len() > 1 {
			assert!(items[0].1.bands.len() >= 2, "{:?}", items[0].1.bands);
			assert!(
				items.last().unwrap().1.bands.len() >= 2,
				"{:?}",
				items.last().unwrap().1.bands
			);
		}
	}
}

#[test]
fn a_heading_reserves_the_spacing_between_it_and_its_paragraph() {
	// The heading's trailing space and the paragraph's leading space belong to
	// the group. Leaving them out lets the heading fit while its paragraph
	// moves on, which is the orphan this rule exists to prevent.
	let source = format!(
		"{}\n\n## Section\n\nA paragraph under the heading.",
		"filler ".repeat(400)
	);
	let (document, snapshot) = layout(&source, 300.0);
	let geometry = geometry(113.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	let heading = items_of(&pagination, 1);
	let body = items_of(&pagination, 2);
	assert_eq!(heading.len(), 1);
	assert!(!body.is_empty());
	assert_eq!(
		heading[0].0, body[0].0,
		"the heading and the paragraph must share a page"
	);
}

#[test]
fn a_padded_heading_keeps_its_anchor() {
	// A heading's anchor is its box's top edge, above the heading's own
	// padding, so it can precede every drawn line of the block.
	let mut sheet = (*Stylesheet::bundled_print()).clone();
	sheet.merge(
		&Stylesheet::parse(
			"format_version=2\nversion=1\n[[rule]]\nwhen=['h2']\npadding=2.0",
		)
		.unwrap(),
	);
	let source = format!("{}\n\n## Section\n\nBody.", "filler ".repeat(200));
	let document = document::parse(source);
	let options = LayoutOptions {
		width: 300.0,
		codeblock_wrap: true,
		stylesheet: std::sync::Arc::new(sheet),
		..Default::default()
	};
	let snapshot = LayoutEngine::new().layout(&document, &options);
	let geometry = geometry(200.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	let (page, y) = pagination.anchors["section"];
	assert!(page < pagination.pages.len());
	assert!(y >= 0.0);
}

#[test]
fn a_heading_travels_with_the_block_it_introduces() {
	let source = format!(
		"{}\n\n## Section\n\nA paragraph under the heading.",
		"filler ".repeat(600)
	);
	let (document, snapshot) = layout(&source, 300.0);
	let geometry = geometry(200.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	let heading = items_of(&pagination, 1);
	let body = items_of(&pagination, 2);
	assert_eq!(heading.len(), 1);
	assert!(!body.is_empty());
	assert_eq!(
		heading[0].0, body[0].0,
		"the heading and the paragraph must share a page"
	);
}

#[test]
fn a_code_block_splits_at_line_boundaries() {
	let code: String = (0..120).map(|i| format!("line {i}\n")).collect();
	let source = format!("```\n{code}```\n");
	let (document, snapshot) = layout(&source, 400.0);
	let geometry = geometry(150.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	assert!(pagination.pages.len() > 2, "{}", pagination.pages.len());
	assert_fragments_are_sound(&pagination, &geometry);
	let items = items_of(&pagination, 0);
	let mut expected = 0;
	for (_, item) in &items {
		assert_eq!(item.bands.start, expected);
		expected = item.bands.end;
	}
	let drawn: f32 = items
		.iter()
		.map(|(_, item)| (item.bottom - item.top) * item.scale)
		.sum();
	assert!(drawn > 0.0);
}

#[test]
fn a_table_wider_than_the_page_is_scaled() {
	// Digits cannot be hyphenated, so the columns keep a minimum that does not
	// fit the narrow page.
	let digits = "1234567890".repeat(8);
	let source = format!(
		"| head a | head b |\n|---|---|\n| {digits} | {digits} |\n| {digits} | {digits} |\n"
	);
	let (document, snapshot) = layout(&source, 240.0);
	assert!(
		!snapshot.blocks[0].layout.overflow.is_empty(),
		"the fixture must overflow"
	);
	let geometry = geometry(400.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	let items = items_of(&pagination, 0);
	assert!(items[0].1.scale < 1.0, "{:?}", items[0].1);
}

#[test]
fn an_image_taller_than_the_page_is_scaled_to_fit() {
	let mut images = ImageSnapshot::default();
	images.entries.insert(
		"big.png".into(),
		ImageInfo {
			version: 1,
			size: Some((200, 600)),
			error: None,
		},
	);
	let (document, snapshot) = layout_with("![](big.png)\n", 300.0, &images);
	let geometry = geometry(300.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	let items = items_of(&pagination, 0);
	assert_eq!(items.len(), 1);
	let item = &items[0].1;
	assert!(item.scale < 1.0, "{:?}", item.scale);
	assert!(item.first && item.last);
	assert!((item.bottom - item.top) * item.scale <= 300.5);
}

#[test]
fn pagination_is_deterministic() {
	let source = format!(
		"{}\n\n## H\n\n{}",
		"prose ".repeat(300),
		"code ".repeat(300)
	);
	let (document, snapshot) = layout(&source, 320.0);
	let geometry = geometry(180.0);
	let first = paginate(&document, &snapshot, &geometry);
	let second = paginate(&document, &snapshot, &geometry);
	assert_eq!(first.pages, second.pages);
	assert_eq!(first.anchors, second.anchors);
}

#[test]
fn every_heading_anchor_lands_on_a_page() {
	let source =
		format!("# Title\n\n{}\n\n## Later\n\nEnd.", "prose ".repeat(400));
	let (document, snapshot) = layout(&source, 300.0);
	let geometry = geometry(200.0);
	let pagination = paginate(&document, &snapshot, &geometry);
	let (page, y) = pagination.anchors["title"];
	assert!(page < pagination.pages.len());
	assert!(y >= 0.0);
	let (later, _) = pagination.anchors["later"];
	assert!(later >= page);
}

#[test]
fn geometry_rejects_margins_that_leave_no_room() {
	let mut style = PageStyle::default();
	assert!(PageGeometry::from_style(&style).is_ok());
	style.margin = Some(vec![400.0, 400.0, 400.0, 400.0]);
	assert!(PageGeometry::from_style(&style).is_err());
}

#[test]
fn page_geometry_converts_millimetres() {
	let geometry = PageGeometry::from_style(&PageStyle::default()).unwrap();
	// A4 is 210 by 297 millimetres.
	assert!((geometry.width_pt - 210.0 * MM_TO_PT).abs() < 0.01);
	assert!((geometry.height_pt - 297.0 * MM_TO_PT).abs() < 0.01);
	let expected = (210.0 - 40.0) * MM_TO_PT / PT_PER_PX;
	assert!((geometry.text_px().0 - expected).abs() < 0.01);
}

#[test]
fn templates_accept_only_known_placeholders() {
	assert!(template_is_valid("{page} / {pages}"));
	assert!(template_is_valid("{title}"));
	assert!(!template_is_valid("{date}"));
	assert!(!template_is_valid("{page"));
	let segments = expand_template("{page}/{pages}", 3, 9, "T", "p.md");
	assert_eq!(
		segments,
		vec![
			SlotSegment::Number("3".into()),
			SlotSegment::Text("/".into()),
			SlotSegment::Number("9".into()),
		]
	);
	let segments = expand_template("{title} — {path}", 1, 1, "Doc", "a.md");
	assert_eq!(segments, vec![SlotSegment::Text("Doc — a.md".into())]);
}

#[test]
fn furniture_draws_the_page_number_centred_in_the_margin() {
	let sheet = Stylesheet::bundled_print();
	let mut shaper = TextShaper::new();
	shaper.set_stylesheet(sheet.clone());
	let geometry = PageGeometry::from_style(sheet.page()).unwrap();
	let text = FurnitureText {
		title: "T",
		path: "a.md",
	};
	let draws =
		page_furniture(&sheet, &mut shaper, &geometry, 18.0, 3, 9, &text);
	assert!(!draws.is_empty());
	let draws: Vec<&Draw> =
		draws.iter().flat_map(|piece| piece.draws.iter()).collect();
	let [left, top, width, _] = geometry.text_pt();
	let (min_x, max_x, baseline) = draws.iter().fold(
		(f32::MAX, f32::MIN, f32::MIN),
		|(min_x, max_x, baseline), draw| match draw {
			Draw::Glyph(glyph) => (
				min_x.min(glyph.x),
				max_x.max(glyph.x),
				baseline.max(glyph.y),
			),
			_ => (min_x, max_x, baseline),
		},
	);
	let center = (min_x + max_x) * 0.5;
	assert!(
		(center - (left + width * 0.5)).abs() < 4.0,
		"{center} vs {}",
		left + width * 0.5
	);
	assert!(baseline > geometry.height_pt - geometry.margin_pt[2]);
	assert!(baseline > top);

	let mut plain = (*sheet).clone();
	plain.page.footer_center = Some(String::new());
	let empty =
		page_furniture(&plain, &mut shaper, &geometry, 18.0, 3, 9, &text);
	assert!(empty.is_empty());
}
