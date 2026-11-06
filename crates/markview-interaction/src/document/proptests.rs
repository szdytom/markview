use super::*;
use markview_layout::{
	document::{BlockId, FieldId, LinkOccurrenceId},
	scene::{BlockLayout, Draw, LinkRect, Overflow, Paint, PlacedBlock},
	text::{TextCluster, TextNode},
};
use proptest::prelude::*;
use std::sync::Arc;

fn fixture(y: f32, content_width: f32) -> LayoutSnapshot {
	let text_rect = Rect {
		x: 0.0,
		y: 5.0,
		w: 160.0,
		h: 10.0,
	};
	let image_rect = Rect {
		x: 50.0,
		y: 5.0,
		w: 50.0,
		h: 20.0,
	};
	let mut node = TextNode::new("abcdefgh".to_owned(), "\n\n");
	node.search_field = Some(FieldId::from_index(0));
	node.search_ranges = vec![(0..8, 0..8)].into();
	node.push(TextCluster {
		mixed_spacing: (0.0, 0.0),
		range: 0..8,
		rect: text_rect,
		rtl: false,
		atomic: false,
		command: 0,
	});
	LayoutSnapshot {
		blocks: vec![PlacedBlock {
			id: BlockId::new(1).unwrap(),
			content: Default::default(),
			y,
			flow: Default::default(),
			layout: Arc::new(BlockLayout {
				height: 60.0,
				width: 64.0,
				text: vec![node].into(),
				draws: vec![
					Draw::Rect(text_rect, Paint::Text),
					Draw::Image {
						src: "image".to_owned(),
						version: 7,
						rect: image_rect,
						title: "Image title".to_owned(),
					},
				]
				.into(),
				links: vec![LinkRect {
					commands: 0..1,
					id: LinkId::Inline(LinkOccurrenceId::new(7).unwrap()),
					rect: Rect {
						x: 20.0,
						y: 5.0,
						w: 40.0,
						h: 10.0,
					},
					url: "https://example.test".into(),
					target: LinkTarget::Url("https://example.test".into()),
				}]
				.into(),
				overflow: vec![Overflow {
					rect: Rect {
						x: 0.0,
						y: 0.0,
						w: 64.0,
						h: 40.0,
					},
					content_width,
					commands: 0..2,
					gutter: 8.0,
				}]
				.into(),
				..Default::default()
			}),
		}],
		..Default::default()
	}
}

proptest! {
	#[test]
	fn hover_and_cursor_follow_independent_clipped_window_regions(
		left in 0i16..33, top in 0i16..33, scroll in 0i16..129, block_y in 0i16..129,
		horizontal_offset in -64i16..257, x in -32i16..193, y in -32i16..257,
	) {
		let snapshot = fixture(f32::from(block_y), 160.0);
		let horizontal = Horizontal::from([((0, 0), f32::from(horizontal_offset))]);
		let viewport = Viewport { width: 200.0, height: 240.0, left: f32::from(left), top: f32::from(top), bottom: 16.0, scroll: f32::from(scroll) };
		let context = DocumentInteraction { snapshot: &snapshot, viewport, horizontal: &horizontal, revision: 1 };
		let offset = horizontal_offset.clamp(0, 96);
		// Half-pixel probes avoid ambiguous zero-area contacts at clipping edges.
		for point in [Point::new(f32::from(x) + 0.5, f32::from(y) + 0.5), Point::new(f32::from(left) + 32.5, f32::from(top + block_y - scroll) + 10.5)] {
			let window = 0.0 <= point.x && point.x <= 200.0 && f32::from(top) <= point.y && point.y <= 224.0;
			let dx = point.x - f32::from(left);
			let dy = point.y - f32::from(top + block_y - scroll);
			let clipped = window && 0.0 < dx && dx < 64.0 && 0.0 < dy && dy < 40.0;
			let content_x = dx + f32::from(offset);
			let text = clipped && 0.0 < content_x && content_x < 160.0 && 5.0 < dy && dy < 15.0;
			let link = clipped && 20.0 < content_x && content_x < 60.0 && 5.0 < dy && dy < 15.0;
			let image = clipped && 50.0 < content_x && content_x < 100.0 && 5.0 < dy && dy < 25.0;
			prop_assert_eq!(context.contains(point), window);
			prop_assert_eq!(context.contains_text(point), text);
			prop_assert_eq!(context.link(point), link.then_some("https://example.test"));
			prop_assert_eq!(context.image(point), image.then_some(("image", Rect { x: 50.0, y: 5.0, w: 50.0, h: 20.0 }, "Image title")));
			for links_clickable in [false, true] {
				for images_clickable in [false, true] {
					let cursor = if (link && links_clickable) || (image && images_clickable) { Cursor::Pointer } else if text { Cursor::Text } else { Cursor::Default };
					prop_assert_eq!(context.cursor(point, links_clickable, images_clickable), cursor);
				}
			}
			for holding in [false, true] {
				let hover = context.hover(point, holding, true);
				prop_assert_eq!(hover.cursor, if !holding && (link || image) { Cursor::Pointer } else if text { Cursor::Text } else { Cursor::Default });
				prop_assert_eq!(hover.image_title.as_deref(), (image && !holding).then_some("Image title"));
				prop_assert_eq!(hover.link.is_some(), link && !holding);
				if let Some(link) = &hover.link {
					prop_assert_eq!(link.block, BlockId::new(1).unwrap());
					prop_assert_eq!(link.id, LinkId::Inline(LinkOccurrenceId::new(7).unwrap()));
					prop_assert_eq!(Some(link.link_ref()), context.link_ref(point));
				}
			}
		}
	}

	#[test]
	fn horizontal_pan_sequences_clamp_one_overflow_and_preserve_other_offsets(
		extra in 1u16..201, operations in prop::collection::vec((any::<bool>(), -256i16..257), 0..48), thickness in 1u16..17,
	) {
		let snapshot = fixture(0.0, 64.0 + f32::from(extra));
		let viewport = Viewport { width: 200.0, height: 200.0, left: 10.0, top: 10.0, bottom: 10.0, scroll: 0.0 };
		let mut horizontal = Horizontal::from([((7, 7), 42.0)]);
		let mut expected = None;
		for (inside, delta) in [(true, 256), (true, -256), (false, 256)].into_iter().chain(operations) {
			let point = if inside { Point::new(42.0, 30.0) } else { Point::new(11.0, 70.0) };
			prop_assert_eq!(horizontal_by(&snapshot, viewport, &mut horizontal, point, f32::from(delta)), inside);
			if inside { expected = Some((expected.unwrap_or(0i32) + i32::from(delta)).clamp(0, i32::from(extra))); }
			prop_assert_eq!(horizontal.get(&(0, 0)).copied(), expected.map(|n| n as f32));
			prop_assert_eq!(horizontal.get(&(7, 7)), Some(&42.0));
			let context = DocumentInteraction { snapshot: &snapshot, viewport, horizontal: &horizontal, revision: 1 };
			prop_assert_eq!(context.overflow_at(point), inside.then_some((0, 0)));
			let bar = context.overflow_bar(0, 0, ScrollbarMetrics { thickness: f32::from(thickness) }).unwrap();
			prop_assert_eq!(bar.max_scroll(), f32::from(extra));
			prop_assert_eq!(bar.track, Rect { x: 10.0, y: 50.0, w: 64.0, h: f32::from(thickness.max(8)) });
			let pointer = Point::new(bar.track.x + bar.track.w * 0.5, bar.track.y + bar.track.h * 0.5);
			prop_assert_eq!(context.overflow_bar_at(pointer, ScrollbarMetrics { thickness: f32::from(thickness) }).map(|(block, overflow, _)| (block, overflow)), Some((0, 0)));
		}
	}
}
