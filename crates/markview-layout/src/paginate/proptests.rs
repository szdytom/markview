use super::*;
use crate::{
	document::{AnchorId, DocumentBuilder, DocumentRevision},
	scene::{HeadingAnchor, Paint, PlacedBlock, Rect},
};
use proptest::prelude::*;
use std::sync::Arc;

fn geometry(height: f32) -> PageGeometry {
	PageGeometry {
		width_pt: 300.0,
		height_pt: height * PT_PER_PX,
		margin_pt: [0.0; 4],
	}
}

fn place(id: crate::document::BlockId, layout: BlockLayout) -> PlacedBlock {
	PlacedBlock {
		id,
		content: Default::default(),
		y: 0.0,
		layout: Arc::new(layout),
		flow: Arc::default(),
	}
}

fn lines(
	spec: &[(u16, u16)],
	lead: u16,
	trail: u16,
) -> (BlockLayout, Vec<(f32, f32)>) {
	let mut layout = BlockLayout::default();
	let mut declared = Vec::new();
	let mut y = f32::from(lead);
	for &(height, gap) in spec {
		let bottom = y + f32::from(height);
		declared.push((y, bottom));
		layout.draws.push(Draw::Rect(
			Rect {
				x: 0.0,
				y,
				w: 20.0,
				h: f32::from(height),
			},
			Paint::Text,
		));
		y = bottom + f32::from(gap);
	}
	layout.height = declared
		.last()
		.map_or(0.0, |(_, bottom)| bottom + f32::from(trail));
	(layout, declared)
}

proptest! {
	#[test]
	fn impossible_widow_and_orphan_constraints_make_progress_until_the_suffix_fits(count in 3usize..9) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let block = builder.block(Vec::new(), BlockKind::Code { language: String::new(), text: String::new() });
		let (mut layout, _) = lines(&vec![(16, 4); count], 0, 0);
		layout.page_constraints.push(crate::scene::PageConstraint { top: 0.0, bottom: layout.height, orphans: Some(count as u16), widows: Some(count as u16), keep_together: true });
		let snapshot = LayoutSnapshot { blocks: vec![place(block.id(), layout)], ..Default::default() };
		let pagination = paginate(&builder.finish(vec![block]), &snapshot, &geometry(36.0));
		// Two bands fit; an unsatisfiable suffix advances by one band on an empty page.
		let expected: Vec<_> = (0..count - 2).map(|i| i..i + 1).chain(std::iter::once(count - 2..count)).collect();
		let actual: Vec<_> = pagination.pages.iter().map(|page| {
			assert_eq!(page.len(), 1);
			assert_eq!(page[0].scale, 1.0);
			assert!(page[0].y + page[0].bottom - page[0].top <= 36.0);
			page[0].bands.clone()
		}).collect();
		prop_assert_eq!(actual, expected);
	}

	#[test]
	fn indivisible_images_at_and_beyond_the_scale_floor_keep_one_fragment(
		height in 80u16..161, delta in -2i16..3, beyond in any::<bool>(),
	) {
		let height = f32::from(height);
		let image_height = height * if beyond { 4.0 } else { 1.0 } + f32::from(delta) / 4.0;
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let image = builder.block(Vec::new(), BlockKind::Paragraph(vec![crate::document::Inline {
			kind: InlineKind::Image(Box::new(crate::document::ImageSpec { source: crate::document::ImageSource::Resource(crate::document::ResourceRef::document("image")), alt: String::new(), title: String::new(), width: None, height: None })), style: Default::default(), link: None,
		}]));
		let layout = BlockLayout { draws: vec![Draw::Image { src: "image".to_owned(), version: 1, rect: Rect { x: 0.0, y: 0.0, w: 8.0, h: image_height }, title: String::new() }].into(), height: image_height, ..Default::default() };
		let snapshot = LayoutSnapshot { blocks: vec![place(image.id(), layout)], ..Default::default() };
		let pagination = paginate(&builder.finish(vec![image]), &snapshot, &geometry(height));
		prop_assert_eq!(pagination.pages.len(), 1);
		prop_assert_eq!(pagination.pages[0].len(), 1);
		let item = &pagination.pages[0][0];
		prop_assert_eq!(item.bands.clone(), 0..1);
		prop_assert!(item.first && item.last);
		prop_assert_eq!((item.y, item.top, item.bottom), (0.0, 0.0, image_height));
		let expected = (f64::from(height) / f64::from(image_height)).clamp(0.25, 1.0);
		prop_assert!((f64::from(item.scale) - expected).abs() < 0.000001);
		// At the floor, an oversized band is retained and the page painter clips it.
		if image_height > height * 4.0 { prop_assert_eq!(item.scale, 0.25); }
	}

	#[test]
	fn lone_images_scale_as_one_band_and_keep_their_anchor_on_the_same_page(
		height in 80u16..161, raw_image in any::<u16>(), lead in 0u16..8, trail in 0u16..8, filler in 1u16..80, width in 1u16..301,
	) {
		let image_height = 1 + raw_image % (height * 4 - lead);
		let bottom = f32::from(lead + image_height);
		let expected_scale = if bottom > f32::from(height) { f32::from(height) / bottom } else { 1.0 };
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let first = builder.block(Vec::new(), BlockKind::Rule);
		let image = builder.block(Vec::new(), BlockKind::Paragraph(vec![crate::document::Inline {
			kind: InlineKind::Image(Box::new(crate::document::ImageSpec { source: crate::document::ImageSource::Resource(crate::document::ResourceRef::document("image")), alt: String::new(), title: String::new(), width: None, height: None })), style: Default::default(), link: None,
		}]));
		let first_layout = lines(&[(filler, 1)], 0, 0).0;
		let image_layout = BlockLayout { draws: vec![Draw::Image { src: "image".to_owned(), version: 1, rect: Rect { x: 0.0, y: f32::from(lead), w: f32::from(width), h: f32::from(image_height) }, title: String::new() }].into(), height: bottom + f32::from(trail), anchors: vec![HeadingAnchor { anchor: AnchorId::Fragment("picture".to_owned()), y: f32::from(lead) }], ..Default::default() };
		let snapshot = LayoutSnapshot { blocks: vec![place(first.id(), first_layout), place(image.id(), image_layout)], ..Default::default() };
		let pagination = paginate(&builder.finish(vec![first, image]), &snapshot, &geometry(f32::from(height)));
		let items: Vec<_> = pagination.pages.iter().enumerate().flat_map(|(page, items)| items.iter().filter(|item| item.block == 1).map(move |item| (page, item))).collect();
		prop_assert_eq!(items.len(), 1);
		let (page, item) = items[0];
		prop_assert_eq!(item.bands.clone(), 0..1);
		prop_assert!(item.first && item.last);
		prop_assert_eq!((item.top, item.bottom), (f32::from(lead), bottom));
		prop_assert!((item.scale - expected_scale).abs() <= 0.000001);
		prop_assert!(item.scale >= 0.25 && item.scale <= 1.0);
		prop_assert!(item.y + (item.bottom - item.top) * item.scale <= f32::from(height) + 0.001);
		prop_assert_eq!(pagination.anchors["picture"], (page, item.y));
	}

	#[test]
	fn wide_tables_apply_one_uniform_scale_and_conserve_every_row_band(
		spec in prop::collection::vec((1u16..17, 1u16..5), 1..20),
		overflows in prop::collection::vec((1u16..257, 0u16..1025), 0..5), height in 80u16..161,
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let table = builder.block(Vec::new(), BlockKind::Table(crate::document::Table { caption: None, columns: Vec::new(), groups: Vec::new() }));
		let (mut layout, declared) = lines(&spec, 0, 0);
		let mut stretch = 1.0f64;
		for &(visible, extra) in &overflows {
			stretch = stretch.max(f64::from(visible + extra) / f64::from(visible));
			layout.overflow.push(crate::scene::Overflow { rect: Rect { x: 0.0, y: 0.0, w: f32::from(visible), h: layout.height }, content_width: f32::from(visible + extra), commands: 0..layout.draws.len(), gutter: 0.0 });
		}
		let expected_scale = if stretch > 4.0 { 0.25 } else { 1.0 / stretch };
		for (row, &(top, _)) in declared.iter().enumerate() { layout.anchors.push(HeadingAnchor { anchor: AnchorId::Fragment(format!("row-{row}")), y: top }); }
		let snapshot = LayoutSnapshot { blocks: vec![place(table.id(), layout)], ..Default::default() };
		let pagination = paginate(&builder.finish(vec![table]), &snapshot, &geometry(f32::from(height)));
		let mut rows = Vec::new();
		for (page, items) in pagination.pages.iter().enumerate() {
			for item in items {
				prop_assert_eq!(item.block, 0);
				prop_assert!((f64::from(item.scale) - expected_scale).abs() <= 0.000001);
				prop_assert_eq!((item.top, item.bottom), (declared[item.bands.start].0, declared[item.bands.end - 1].1));
				prop_assert!(item.y + (item.bottom - item.top) * item.scale <= f32::from(height) + 0.5);
				for row in item.bands.clone() {
					rows.push(row);
					let y = item.y + (declared[row].0 - item.top) * item.scale;
					prop_assert_eq!(pagination.anchors[&format!("row-{row}")], (page, y));
				}
			}
		}
		prop_assert_eq!(rows, (0..declared.len()).collect::<Vec<_>>());
		let items: Vec<_> = pagination.pages.iter().flatten().collect();
		prop_assert!(items.first().unwrap().first && items.last().unwrap().last);
	}

	#[test]
	fn headings_share_a_page_with_the_first_body_lines(
		filler in 1u16..80, heading_height in 1u16..17, body_lines in 4usize..13,
		lead in 0u16..7, trail in 0u16..7, height in 80u16..161,
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let blocks = vec![
			builder.block(Vec::new(), BlockKind::Rule),
			builder.block(Vec::new(), BlockKind::Heading { level: 2, text: Vec::new(), anchor: "heading".to_owned() }),
			builder.block(Vec::new(), BlockKind::Code { language: String::new(), text: String::new() }),
		];
		let layouts = [lines(&[(filler, 1)], 0, 0).0, lines(&[(heading_height, 1)], lead, trail).0, lines(&vec![(12, 2); body_lines], lead, 0).0];
		let snapshot = LayoutSnapshot { blocks: layouts.into_iter().zip(&blocks).map(|(layout, block)| place(block.id(), layout)).collect(), ..Default::default() };
		let pagination = paginate(&builder.finish(blocks), &snapshot, &geometry(f32::from(height)));
		let heading: Vec<_> = pagination.pages.iter().enumerate().flat_map(|(page, items)| items.iter().filter(|item| item.block == 1).map(move |item| (page, item))).collect();
		let body = pagination.pages.iter().enumerate().find(|(_, items)| items.iter().any(|item| item.block == 2)).unwrap().0;
		prop_assert_eq!(heading.len(), 1);
		prop_assert_eq!(heading[0].0, body);
	}

	#[test]
	fn pagination_conserves_ordered_bands_and_anchor_destinations(
		specs in prop::collection::vec((prop::collection::vec((1u16..17, 1u16..5), 0..20), 0u16..7, 0u16..7), 0..7),
		height in 80u16..161,
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let mut blocks = Vec::new();
		let mut snapshot = LayoutSnapshot::default();
		let mut declared = Vec::new();
		for (bi, (spec, lead, trail)) in specs.iter().enumerate() {
			let block = builder.block(Vec::new(), BlockKind::Code { language: String::new(), text: "line\n".repeat(spec.len()) });
			let (mut layout, bands) = lines(spec, *lead, *trail);
			for (i, &(top, _)) in bands.iter().enumerate() {
				layout.anchors.push(HeadingAnchor { anchor: AnchorId::Fragment(format!("{bi}-{i}")), y: top });
				layout.anchors.push(HeadingAnchor { anchor: AnchorId::FootnoteDefinition(crate::document::FootnoteId::new(format!("{bi}-{i}"))), y: top });
			}
			snapshot.blocks.push(place(block.id(), layout));
			blocks.push(block);
			declared.push(bands);
		}
		let document = builder.finish(blocks);
		let pagination = paginate(&document, &snapshot, &geometry(f32::from(height)));
		let expected: Vec<_> = declared.iter().enumerate().flat_map(|(block, bands)| (0..bands.len()).map(move |band| (block, band))).collect();
		let mut actual = Vec::new();
		for (page, items) in pagination.pages.iter().enumerate() {
			prop_assert!(!items.is_empty() || expected.is_empty());
			let mut previous_bottom = 0.0;
			for item in items {
				let bands = &declared[item.block];
				prop_assert!(item.bands.start < item.bands.end && item.bands.end <= bands.len());
				prop_assert_eq!(item.scale, 1.0);
				prop_assert_eq!(item.top, bands[item.bands.start].0);
				prop_assert_eq!(item.bottom, bands[item.bands.end - 1].1);
				prop_assert_eq!(item.first, item.bands.start == 0);
				prop_assert_eq!(item.last, item.bands.end == bands.len());
				prop_assert!(item.y >= previous_bottom);
				previous_bottom = item.y + item.bottom - item.top;
				prop_assert!(previous_bottom <= f32::from(height) + 0.5);
				for band in item.bands.clone() {
					actual.push((item.block, band));
					let destination = (page, item.y + bands[band].0 - item.top);
					let name = format!("{}-{band}", item.block);
					prop_assert_eq!(pagination.anchors.get(&name), Some(&destination));
					prop_assert_eq!(pagination.targets.get(&AnchorId::FootnoteDefinition(crate::document::FootnoteId::new(name))), Some(&destination));
				}
			}
		}
		prop_assert_eq!(actual, expected);
		prop_assert_eq!(pagination.anchors.len(), declared.iter().map(Vec::len).sum::<usize>());
		if declared.iter().all(Vec::is_empty) { prop_assert_eq!(pagination.pages.len(), 1); }
		for (block, bands) in declared.iter().enumerate() {
			let fragments: Vec<_> = pagination.pages.iter().flatten().filter(|item| item.block == block).collect();
			if bands.len() >= 4 && fragments.len() > 1 {
				prop_assert!(fragments[0].bands.len() >= 2);
				prop_assert!(fragments.last().unwrap().bands.len() >= 2);
			}
		}
	}

	#[test]
	fn explicit_orphans_widows_and_keep_together_constrain_page_breaks(
		count in 1usize..25, orphans in 1u16..5, widows in 1u16..5, keep in any::<bool>(),
		height in 80u16..161, filler in 1u16..80,
	) {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let blocks: Vec<_> = (0..2).map(|_| builder.block(Vec::new(), BlockKind::Code { language: String::new(), text: String::new() })).collect();
		let (filler, _) = lines(&[(filler, 1)], 0, 0);
		let (mut target, _) = lines(&vec![(8, 2); count], 0, 0);
		let target_height = target.height;
		target.page_constraints.push(crate::scene::PageConstraint { top: 0.0, bottom: target_height, orphans: Some(orphans), widows: Some(widows), keep_together: keep });
		let snapshot = LayoutSnapshot { blocks: vec![place(blocks[0].id(), filler), place(blocks[1].id(), target)], ..Default::default() };
		let pagination = paginate(&builder.finish(blocks), &snapshot, &geometry(f32::from(height)));
		let fragments: Vec<_> = pagination.pages.iter().flatten().filter(|item| item.block == 1).collect();
		prop_assert_eq!(fragments.iter().map(|item| item.bands.len()).sum::<usize>(), count);
		if (keep && target_height <= f32::from(height)) || count < usize::from(orphans + widows) {
			prop_assert_eq!(fragments.len(), 1);
		}
		if fragments.len() > 1 {
			prop_assert!(fragments[0].bands.len() >= usize::from(orphans));
			prop_assert!(fragments.last().unwrap().bands.len() >= usize::from(widows));
		}
		for item in pagination.pages.iter().flatten() {
			prop_assert!(item.y + item.bottom - item.top <= f32::from(height) + 0.5);
		}
	}
}
