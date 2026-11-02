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
	assert_eq!(layout.edge_scroll(10.0), 0.0);
	for _ in 0..200 {
		layout = TabLayout::new(
			viewport(),
			&widths,
			layout.scroll + layout.edge_scroll(310.0),
		);
		drag.update(&layout, 310.0);
	}
	assert_eq!(drag.index, 0);
	assert_eq!(drag.drop_index(), Some(19));
	assert_eq!(layout.edge_scroll(310.0), 0.0);
	assert_eq!(layout.scroll, layout.max_scroll);
	for _ in 0..200 {
		layout = TabLayout::new(
			viewport(),
			&widths,
			layout.scroll + layout.edge_scroll(10.0),
		);
		drag.update(&layout, 10.0);
	}
	assert_eq!(drag.drop_index(), None);
	assert_eq!(layout.scroll, 0.0);
	let mut strip = TabStrip {
		drag: Some(drag),
		scroll_at: Some(Instant::now()),
		..Default::default()
	};
	strip.cancel_drag();
	assert!(strip.drag.is_none() && strip.scroll_at.is_none());
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
