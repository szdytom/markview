use super::*;
use std::time::Duration;
fn viewport() -> Rect {
	Rect {
		x: 10.0,
		y: 4.0,
		w: 300.0,
		h: 32.0,
	}
}
#[test]
fn widths_shrink_before_scrolling_and_never_below_minimum() {
	let small = TabLayout::new(viewport(), &[(100.0, 60.0); 2], 50.0);
	assert_eq!(small.rects[0].w, 100.0);
	assert_eq!(small.scroll, 0.0);
	let compressed = TabLayout::new(viewport(), &[(120.0, 60.0); 4], 0.0);
	assert_eq!(compressed.max_scroll, 0.0);
	assert_eq!(compressed.rects[0].w, 73.5);
	let overflow = TabLayout::new(viewport(), &[(120.0, 60.0); 8], 10000.0);
	assert_eq!(overflow.rects[0].w, 60.0);
	assert_eq!(overflow.scroll, overflow.max_scroll);
	assert_eq!(overflow.rects.last().unwrap().x + 60.0, 310.0);
}
#[test]
fn scrolling_clips_hits_and_can_reveal_every_tab() {
	let widths = [(120.0, 60.0); 20];
	let layout = TabLayout::new(viewport(), &widths, 25.0);
	assert_eq!(layout.hit(9.0, 10.0), None);
	assert_eq!(layout.hit(311.0, 10.0), None);
	assert_eq!(layout.hit(12.0, 10.0), Some(0));
	assert_eq!(layout.hit(12.0, 38.0), None);
	for i in 0..20 {
		let revealed = TabLayout::new(viewport(), &widths, layout.reveal(i));
		let rect = revealed.rects[i];
		assert!(rect.x >= 10.0 && rect.x + rect.w <= 310.0);
	}
	assert_eq!(TabLayout::new(viewport(), &[], 500.0).scroll, 0.0);
}

#[test]
fn drag_threshold_and_slots_leave_the_order_alone() {
	let layout = TabLayout::new(viewport(), &[(80.0, 60.0); 4], 0.0);
	let mut drag = TabDrag {
		index: 0,
		start: 20.0,
		grab: 10.0,
		moving: false,
		slot: None,
	};
	drag.update(&layout, 24.0);
	assert!(!drag.moving);
	assert_eq!(drag.drop_index(), None);
	drag.update(&layout, 140.0);
	assert!(drag.moving);
	assert_eq!(drag.slot, Some(2));
	assert_eq!(drag.drop_index(), Some(1));
	// The tab rests until the pointer reaches the previous tab's center.
	drag.update(&layout, 20.0);
	assert_eq!(drag.slot, Some(0));
	assert_eq!(drag.drop_index(), None);
}

#[test]
fn edge_scroll_aims_at_later_tabs_without_moving_any() {
	let widths = [(120.0, 60.0); 20];
	let mut layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut drag = TabDrag {
		index: 0,
		start: 20.0,
		grab: 10.0,
		moving: true,
		slot: None,
	};
	assert_eq!(layout.edge_scroll_speed(10.0), 0.0);
	for _ in 0..200 {
		layout = TabLayout::new(
			viewport(),
			&widths,
			layout.scroll + layout.edge_scroll_speed(310.0) * 0.016,
		);
		drag.update(&layout, 310.0);
	}
	assert_eq!(drag.index, 0);
	assert_eq!(drag.drop_index(), Some(19));
	assert_eq!(layout.edge_scroll_speed(310.0) * 0.016, 0.0);
	assert_eq!(layout.scroll, layout.max_scroll);
	for _ in 0..200 {
		layout = TabLayout::new(
			viewport(),
			&widths,
			layout.scroll + layout.edge_scroll_speed(10.0) * 0.016,
		);
		drag.update(&layout, 10.0);
	}
	assert_eq!(drag.drop_index(), None);
	assert_eq!(layout.scroll, 0.0);
	let mut strip = TabStrip {
		drag: Some(drag),
		..Default::default()
	};
	strip.cancel_drag();
	assert!(strip.drag.is_none());
}

#[test]
fn a_dropped_tab_slides_from_the_pointer_into_its_new_place() {
	let widths = [(100.0, 60.0), (60.0, 60.0), (80.0, 60.0)];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let dragged = Rect {
		x: 240.0,
		..layout.rects[0]
	};
	let mut strip = TabStrip::default();
	strip.begin_reorder(&layout.rects, &layout.rects, 0, 2, dragged);
	let after = TabLayout::new(
		viewport(),
		&[(60.0, 60.0), (80.0, 60.0), (100.0, 60.0)],
		0.0,
	);
	strip.freeze_slide(Duration::ZERO);
	let start = strip.rects(&after).unwrap();
	assert_eq!(start[0], layout.rects[1]);
	assert_eq!(start[1], layout.rects[2]);
	assert_eq!(start[2], dragged);
	strip.freeze_slide(Duration::from_millis(110));
	let halfway = strip.rects(&after).unwrap();
	assert!(halfway[2].x < dragged.x);
	assert!(halfway[2].x > after.rects[2].x);
	strip.advance_animation(Instant::now() + Duration::from_secs(1));
	assert!(strip.rects(&after).is_none());
}

#[test]
fn closing_a_tab_slides_the_following_tabs_left_and_settles() {
	let widths = [(100.0, 60.0); 3];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut strip = TabStrip::default();
	strip.begin_close(&layout.rects, 1);
	assert!(strip.animating());
	let after = [(100.0, 60.0); 2];
	let settled = TabLayout::new(viewport(), &after, 0.0);
	strip.freeze_slide(Duration::ZERO);
	let start = strip.rects(&settled).unwrap();
	assert_eq!(start[0], layout.rects[0]);
	assert_eq!(start[1], layout.rects[2]);
	strip.freeze_slide(Duration::from_millis(110));
	let halfway = strip.rects(&settled).unwrap();
	assert!(halfway[1].x < layout.rects[2].x);
	assert!(halfway[1].x > settled.rects[1].x);
	strip.advance_animation(Instant::now() + Duration::from_secs(1));
	assert!(!strip.animating());
	assert!(strip.rects(&settled).is_none());
}

#[test]
fn opening_a_tab_grows_it_from_its_slot_and_settles() {
	let widths = [(100.0, 60.0); 2];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut strip = TabStrip::default();
	let end = layout.rects[1].x + layout.rects[1].w;
	strip.begin_open(&layout, 1);
	assert!(strip.animating());
	let after = [(100.0, 60.0); 3];
	let settled = TabLayout::new(viewport(), &after, 0.0);
	let settled_end = settled.rects[2].x + settled.rects[2].w;
	strip.freeze_slide(Duration::ZERO);
	let start = strip.rects(&settled).unwrap();
	assert_eq!(start[0], layout.rects[0]);
	assert_eq!(start[1].x, layout.rects[1].x);
	assert_eq!(start[1].w, 0.0);
	assert_eq!(start[2], layout.rects[1]);
	assert_eq!(strip.end(settled_end, 3), end);
	strip.freeze_slide(Duration::from_millis(110));
	let halfway = strip.rects(&settled).unwrap();
	assert!(halfway[1].w > 0.0 && halfway[1].w < settled.rects[1].w);
	assert!(halfway[2].x > layout.rects[1].x);
	assert!(halfway[2].x < settled.rects[2].x);
	let halfway_end = strip.end(settled_end, 3);
	assert!(halfway_end > end && halfway_end < settled_end);
	strip.advance_animation(Instant::now() + Duration::from_secs(1));
	assert!(!strip.animating());
	assert!(strip.rects(&settled).is_none());
	// An appended tab grows out of the old right edge.
	strip.begin_open(&layout, 2);
	strip.freeze_slide(Duration::ZERO);
	let appended = strip.rects(&settled).unwrap();
	assert_eq!(
		appended[2],
		Rect {
			x: layout.rects[1].x + layout.rects[1].w,
			w: 0.0,
			..layout.rects[1]
		}
	);
}

#[test]
fn dragging_opens_a_gap_that_matches_the_reordered_layout() {
	let widths = [(100.0, 60.0), (60.0, 60.0), (80.0, 60.0)];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut strip = TabStrip {
		drag: Some(TabDrag {
			index: 0,
			start: 10.0,
			grab: 5.0,
			moving: true,
			slot: Some(2),
		}),
		..Default::default()
	};
	strip.retarget_shift(&layout, &widths);
	strip.freeze_shift(Duration::from_secs(1));
	let offsets = strip.drag_offsets(Instant::now()).unwrap();
	let reordered = [(60.0, 60.0), (100.0, 60.0), (80.0, 60.0)];
	let preview = TabLayout::new(viewport(), &reordered, 0.0);
	assert_eq!(offsets[0], 0.0);
	assert_eq!(layout.rects[1].x + offsets[1], preview.rects[0].x);
	assert_eq!(layout.rects[2].x + offsets[2], preview.rects[2].x);
	strip.cancel_drag();
	assert!(
		strip.drag_offsets(Instant::now()).unwrap()[1] < 0.0,
		"releasing without a drop closes the gap from where it was"
	);
}

#[test]
fn closing_mid_drag_starts_the_slide_from_the_shifted_tabs() {
	let widths = [(100.0, 60.0); 3];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut strip = TabStrip {
		drag: Some(TabDrag {
			index: 0,
			start: 10.0,
			grab: 5.0,
			moving: true,
			slot: Some(2),
		}),
		..Default::default()
	};
	strip.retarget_shift(&layout, &widths);
	strip.freeze_shift(Duration::from_secs(1));
	strip.begin_close(&layout.rects, 0);
	let after = [(100.0, 60.0); 2];
	let settled = TabLayout::new(viewport(), &after, 0.0);
	strip.freeze_slide(Duration::ZERO);
	let start = strip.rects(&settled).unwrap();
	assert_eq!(start[0].x, layout.rects[0].x);
	assert_eq!(start[1].x, layout.rects[2].x);
	assert!(
		strip.drag_offsets(Instant::now()).is_none(),
		"the close slide owns the tabs' positions"
	);
}

#[test]
fn opening_mid_drag_starts_the_slide_from_the_shifted_tabs() {
	let widths = [(100.0, 60.0); 3];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut strip = TabStrip {
		drag: Some(TabDrag {
			index: 0,
			start: 10.0,
			grab: 5.0,
			moving: true,
			slot: Some(2),
		}),
		..Default::default()
	};
	strip.retarget_shift(&layout, &widths);
	strip.freeze_shift(Duration::from_secs(1));
	let mut shown = layout.rects.clone();
	for (rect, offset) in shown
		.iter_mut()
		.zip(strip.drag_offsets(Instant::now()).unwrap())
	{
		rect.x += offset;
	}
	// The strip still rests, so the open has to pick up the live gap itself.
	strip.begin_open(&layout, 1);
	let after = [(100.0, 60.0); 4];
	let settled = TabLayout::new(viewport(), &after, 0.0);
	strip.freeze_slide(Duration::ZERO);
	let start = strip.rects(&settled).unwrap();
	assert_eq!(start[0], shown[0]);
	assert_eq!(start[1].x, shown[1].x);
	assert_eq!(start[1].w, 0.0);
	assert_eq!(start[2], shown[1]);
	assert_eq!(start[3], shown[2]);
	assert!(
		strip.drag_offsets(Instant::now()).is_none(),
		"the open slide owns the tabs' positions"
	);
}

#[test]
fn opening_mid_close_starts_from_the_tabs_as_painted() {
	let widths = [(100.0, 60.0); 3];
	let layout = TabLayout::new(viewport(), &widths, 0.0);
	let mut strip = TabStrip::default();
	strip.begin_close(&layout.rects, 0);
	let closing_widths = [(100.0, 60.0); 2];
	let closing = TabLayout::new(viewport(), &closing_widths, 0.0);
	strip.freeze_slide(Duration::from_millis(110));
	let shown = strip.rects(&closing).unwrap();
	let mut shown_layout = TabLayout::new(viewport(), &closing_widths, 0.0);
	shown_layout.rects = shown.clone();
	strip.begin_open(&shown_layout, 1);
	assert!(strip.animating());
	strip.freeze_slide(Duration::ZERO);
	let mut start = strip.rects(&layout).unwrap();
	// The one tab that is new takes the slot the other two left behind.
	start.remove(1);
	assert_eq!(start, shown);
}

#[test]
fn clearing_a_slide_leaves_the_tabs_at_rest() {
	let layout = TabLayout::new(viewport(), &[(100.0, 60.0); 2], 0.0);
	let mut strip = TabStrip::default();
	strip.begin_open(&layout, 1);
	assert!(strip.animating());
	strip.clear_slide();
	let after = TabLayout::new(viewport(), &[(100.0, 60.0); 3], 0.0);
	assert!(!strip.animating());
	assert!(strip.rects(&after).is_none());
}

#[test]
fn the_first_tab_of_an_empty_strip_opens_from_the_viewport_edge() {
	let empty = TabLayout::new(viewport(), &[], 0.0);
	let mut strip = TabStrip::default();
	strip.begin_open(&empty, 0);
	assert!(strip.animating());
	let after = TabLayout::new(viewport(), &[(100.0, 60.0)], 0.0);
	strip.freeze_slide(Duration::ZERO);
	assert_eq!(
		strip.rects(&after).unwrap()[0],
		Rect {
			w: 0.0,
			..viewport()
		}
	);
	assert_eq!(
		strip.end(after.rects[0].x + after.rects[0].w, 1),
		viewport().x
	);
	// Phone mode hides the strip, so there is no tab to grow.
	let hidden = TabLayout::new(
		Rect {
			w: 0.0,
			..viewport()
		},
		&[],
		0.0,
	);
	let mut hidden_strip = TabStrip::default();
	hidden_strip.begin_open(&hidden, 0);
	assert!(!hidden_strip.animating());
}

#[test]
fn hit_testing_prefers_the_tab_painted_on_top() {
	let layout = TabLayout {
		viewport: viewport(),
		rects: vec![
			Rect {
				x: 10.0,
				w: 100.0,
				..viewport()
			},
			Rect {
				x: 50.0,
				w: 100.0,
				..viewport()
			},
		],
		max_scroll: 0.0,
		scroll: 0.0,
	};
	assert_eq!(layout.hit(60.0, 10.0), Some(0));
	assert_eq!(layout.hit_over(60.0, 10.0, 1), Some(1));
	assert_eq!(layout.hit_over(20.0, 10.0, 1), Some(0));
}
