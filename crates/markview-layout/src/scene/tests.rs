use super::*;
#[test]
fn scrolled_links_cannot_be_activated_outside_their_clip() {
	let document = markview_commonmark::parse(
		"| Link |\n|---|\n| [abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz](https://example.com) |\n",
	);
	let snapshot = crate::layout::LayoutEngine::new().layout(
		&document,
		&crate::layout::LayoutOptions {
			width: 100.0,
			..Default::default()
		},
	);
	let block = &snapshot.blocks[0];
	let overflow = &block.layout.overflow[0];
	let link = &block.layout.links.at(0);
	let horizontal = HashMap::from([((0, 0), 80.0)]);
	assert_eq!(
		snapshot.link_at(
			overflow.rect.x - 1.0,
			block.y + link.rect.y + 1.0,
			&horizontal
		),
		None
	);
	assert_eq!(
		snapshot.link_at(
			overflow.rect.x + 5.0,
			block.y + link.rect.y + 1.0,
			&horizontal
		),
		Some("https://example.com")
	);
}
#[test]
fn viewport_translation_and_clipping_share_logical_coordinates() {
	let viewport = Viewport {
		width: 800.0,
		height: 600.0,
		left: 20.0,
		top: 68.0,
		bottom: 38.0,
		scroll: 100.0,
	};
	let rect = viewport.window_rect(Rect {
		x: 30.0,
		y: 150.0,
		w: 60.0,
		h: 25.0,
	});
	assert_eq!(viewport.document_point(rect.x, rect.y), (30.0, 150.0));
	assert!(viewport.clip().contains(rect.x, rect.y));
	assert!(!viewport.clip().contains(rect.x, 30.0));
}
#[test]
fn scrollbar_thumb_spans_the_track_and_maps_to_the_scroll_range() {
	let metrics = ScrollbarMetrics::DOCUMENT;
	let track = Rect {
		x: 786.0,
		y: 40.0,
		w: metrics.thickness,
		h: 732.0,
	};
	// A document that fits the viewport shows no scrollbar at all.
	assert!(Scrollbar::vertical(track, 0.0, 100.0, 100.0, metrics).is_none());
	let top = Scrollbar::vertical(track, 0.0, 2000.0, 500.0, metrics).unwrap();
	assert_eq!(top.thumb.y, 40.0);
	assert_eq!(top.thumb.h, track.h * 0.25);
	let travel = track.h - top.thumb.h;
	let bottom =
		Scrollbar::vertical(track, 1500.0, 2000.0, 500.0, metrics).unwrap();
	assert!((bottom.thumb.y - (40.0 + travel)).abs() < 0.01);
	// Grabbing 10 px inside the thumb and dragging by the whole travel
	// moves the document by exactly the scroll maximum.
	let grab = top.grab(800.0, 50.0);
	assert_eq!(grab, 10.0);
	assert!((top.scroll_for(800.0, 50.0 + travel, grab) - 1500.0).abs() < 0.01);
	assert_eq!(top.scroll_for(800.0, -460.0, grab), 0.0);
	// A press on the empty track puts the thumb's start under the pointer.
	assert_eq!(top.scroll_for(800.0, 40.0, 0.0), 0.0);
	assert_eq!(top.scroll_for(800.0, 40.0 + travel, 0.0), 1500.0);
}
#[test]
fn empty_documents_have_no_scrollable_tail() {
	let snapshot = crate::layout::LayoutEngine::new().layout(
		&markview_commonmark::parse(""),
		&crate::layout::LayoutOptions::default(),
	);
	assert_eq!(snapshot.scroll_limit(600.0), 0.0);
}

#[test]
fn scroll_limit_keeps_the_last_visible_line_at_the_top() {
	for source in [
		"First line.  \nSecond line.  \nLast line.",
		"```text\nFirst line.\nSecond line.\nLast line.\n```",
		"| First | Second |\n|---|---|\n| Short | A long final cell that wraps across several lines. |",
		"<details>\n<summary>Last visible line</summary>\n\nHidden text.\n\n</details>",
		"<details open>\n<summary>Summary</summary>\n\nFirst line.  \nLast line.\n\n</details>",
	] {
		let document = markview_commonmark::parse(source);
		for font_size in [12.0, 28.0] {
			let snapshot = crate::layout::LayoutEngine::new().layout(
				&document,
				&crate::layout::LayoutOptions {
					width: 180.0,
					font_size,
					..Default::default()
				},
			);
			let last_line = snapshot
				.blocks
				.iter()
				.flat_map(|block| {
					block.clusters().map(|(_, _, mut rect)| {
						rect.y += block.y;
						rect
					})
				})
				.max_by(|a, b| a.y.total_cmp(&b.y))
				.unwrap();
			for viewport in [300.0, 900.0] {
				assert_eq!(
					snapshot.scroll_limit(viewport),
					last_line.y,
					"{source}"
				);
			}
			assert!(snapshot.height - last_line.y < 100.0);
			// A viewport shorter than the last row must still reach its bottom.
			assert_eq!(
				snapshot.scroll_limit(1.0),
				last_line.y + (last_line.h - 1.0)
			);
		}
	}
}

#[test]
fn scroll_limit_tracks_the_last_line_when_trailing_details_toggle() {
	let document = markview_commonmark::parse(
		"Opening paragraph.\n\n<details>\n<summary>Summary</summary>\n\nFirst hidden line.  \nLast hidden line.\n\n</details>",
	);
	let mut snapshot = crate::layout::LayoutEngine::new()
		.layout(&document, &crate::layout::LayoutOptions::default());
	let last = snapshot.blocks.last().unwrap();
	let id = last.id;
	let retained = last.layout.clone();
	let closed = snapshot.scroll_limit(600.0);
	for open in [true, false, true, false] {
		snapshot.set_disclosures(
			&std::collections::BTreeMap::from([(id, open)]),
			false,
		);
		let last = snapshot.blocks.last().unwrap();
		assert!(Arc::ptr_eq(&retained, &last.layout));
		let final_row = last
			.clusters()
			.map(|(_, _, rect)| last.y + rect.y)
			.max_by(f32::total_cmp)
			.unwrap();
		assert_eq!(snapshot.scroll_limit(600.0), final_row);
		if open {
			assert!(final_row > closed);
		} else {
			assert_eq!(final_row, closed);
		}
	}
}

#[test]
fn document_scrollbar_shrinks_in_the_blank_tail() {
	let track = Rect {
		x: 0.0,
		y: 40.0,
		w: 8.0,
		h: 600.0,
	};
	let bar = |scroll, content| {
		Scrollbar::document(
			track,
			scroll,
			content,
			600.0,
			(content - 20.0).max(0.0),
			ScrollbarMetrics::DOCUMENT,
		)
		.unwrap()
	};
	for (scroll, y, h) in [
		(0.0, 40.0, 180.0),
		(1400.0, 460.0, 180.0),
		(1600.0, 520.0, 120.0),
		(1800.0, 580.0, 60.0),
		(1980.0, 634.0, 6.0),
	] {
		let bar = bar(scroll, 2000.0);
		assert_eq!((bar.thumb.y, bar.thumb.h), (y, h));
		assert_eq!(bar.max_scroll(), 1980.0);
	}
	// A short document starts fully visible and still scrolls into its tail.
	assert_eq!(bar(0.0, 400.0).thumb.h, 600.0);
	assert_eq!(
		(bar(200.0, 400.0).thumb.y, bar(200.0, 400.0).thumb.h),
		(340.0, 300.0)
	);
	// The represented fraction stays exact even for a very long document.
	assert!((bar(0.0, 1_000_000.0).thumb.h - 0.36).abs() < 0.001);
	for (content, max_scroll) in [(0.0, 0.0), (100.0, 0.0), (600.0, 0.0)] {
		assert!(
			Scrollbar::document(
				track,
				0.0,
				content,
				600.0,
				max_scroll,
				ScrollbarMetrics::DOCUMENT
			)
			.is_none()
		);
	}
}

#[test]
fn scrollbar_grab_zone_is_the_band_and_the_thumb_keeps_its_thickness() {
	let metrics = ScrollbarMetrics::DOCUMENT;
	let band = Rect {
		x: 786.0,
		y: 40.0,
		w: metrics.thickness,
		h: 700.0,
	};
	let vertical =
		Scrollbar::vertical(band, 0.0, 2000.0, 500.0, metrics).unwrap();
	assert!(vertical.hit(786.0, 80.0));
	assert!(vertical.hit(786.0 + metrics.thickness - 1.0, 80.0));
	assert!(!vertical.hit(785.0, 80.0));
	assert!(!vertical.hit(790.0, 35.0));
	assert!(!vertical.hit(790.0, 745.0));
	// The track and thumb both fill the interactive band.
	let (track, thumb) = vertical.bars();
	assert_eq!(track.w, metrics.thickness);
	assert_eq!(track.x, band.x);
	assert_eq!(thumb.w, metrics.thickness);
	// Anywhere across the band at the thumb's own extent grabs the thumb,
	// so a press that looks like it landed on the bar never jumps.
	assert!(vertical.on_thumb(band.x, band.y + 1.0));
	assert!(vertical.on_thumb(band.x + band.w, vertical.thumb.y));
	assert!(
		vertical.on_thumb(band.x + band.w, vertical.thumb.y + vertical.thumb.h)
	);
	assert!(
		!vertical.on_thumb(band.x, vertical.thumb.y + vertical.thumb.h + 1.0)
	);
	assert!(!vertical.on_thumb(band.x - 1.0, vertical.thumb.y));
}
#[test]
fn horizontal_scrollbar_uses_the_block_gutter_and_keeps_its_thickness() {
	let metrics = ScrollbarMetrics::OVERFLOW;
	let band = Rect {
		x: 40.0,
		y: 300.0,
		w: 200.0,
		h: SCROLLBAR_GUTTER,
	};
	let bar = Scrollbar::horizontal(band, 0.0, 400.0, 200.0, metrics).unwrap();
	assert_eq!(bar.thumb.w, 100.0);
	assert_eq!(bar.thumb.x, 40.0);
	// The band is the reserved gutter, or the bar itself without one.
	assert_eq!(metrics.overflow_band(SCROLLBAR_GUTTER), SCROLLBAR_GUTTER);
	assert_eq!(metrics.overflow_band(30.0), 30.0);
	assert_eq!(metrics.overflow_band(0.0), metrics.thickness);
	let (track, thumb) = bar.bars();
	assert_eq!(track.h, metrics.thickness);
	assert_eq!(track.y, band.y + (band.h - metrics.thickness) * 0.5);
	assert_eq!(thumb.h, metrics.thickness);
	assert!(bar.hit(100.0, 300.0));
	assert!(bar.hit(100.0, 307.0));
	assert!(!bar.hit(100.0, 309.0));
	assert!(!bar.hit(20.0, 300.0));
	assert!(bar.on_thumb(100.0, 300.0));
	assert!(!bar.on_thumb(150.0, 300.0));
	// A press on the empty track jumps the thumb under the pointer.
	assert_eq!(bar.scroll_for(240.0, 300.0, 0.0), 200.0);
}

#[test]
fn a_zero_extent_edge_fit_keeps_denormals_finite() {
	// A fuzz finding: with a non-positive extent the pair still overflowed
	// the `sum > extent` test, and the `0 / denormal` scale produced NaN
	// border widths.
	let rect = Rect {
		x: 0.0,
		y: 0.0,
		w: 0.0,
		h: -1.7e38,
	};
	let out = fit_edges(rect, [0.0, 0.0, f32::MIN_POSITIVE, 0.0]);
	assert!(
		out.iter().all(|v| v.is_finite()),
		"a zero or negative extent must not scale the edges: {out:?}"
	);
}
