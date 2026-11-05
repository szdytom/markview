use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn tab_widths_share_the_available_budget_and_reveal_fitting_tabs(
		specs in prop::collection::vec((8u16..33, 0u16..33), 0..12),
		width in 0u16..769, scroll in -100i16..1001, probe in -50i16..1001,
	) {
		let viewport = Rect { x: 10.0, y: 20.0, w: f32::from(width), h: 30.0 };
		let widths: Vec<_> = specs.iter().map(|&(minimum, extra)| (f32::from(minimum + extra), f32::from(minimum))).collect();
		let layout = TabLayout::new(viewport, &widths, f32::from(scroll));
		let natural: f32 = specs.iter().map(|&(m, e)| f32::from(m + e)).sum();
		let minimum: f32 = specs.iter().map(|&(m, _)| f32::from(m)).sum();
		let gaps = specs.len().saturating_sub(1) as f32 * 2.0;
		let actual: f32 = layout.rects.iter().map(|rect| rect.w).sum::<f32>() + gaps;
		if natural + gaps <= viewport.w {
			prop_assert_eq!(actual, natural + gaps);
		} else if minimum + gaps >= viewport.w {
			prop_assert_eq!(actual, minimum + gaps);
		} else { prop_assert!((actual - viewport.w).abs() <= 0.001); }
		prop_assert!((layout.max_scroll - (actual - viewport.w).max(0.0)).abs() <= 0.001);
		prop_assert_eq!(layout.scroll, f32::from(scroll).clamp(0.0, layout.max_scroll));
		let mut x = viewport.x - layout.scroll;
		for (i, rect) in layout.rects.iter().enumerate() {
			prop_assert!((rect.x - x).abs() <= 0.001);
			prop_assert_eq!((rect.y, rect.h), (20.0, 30.0));
			prop_assert!(rect.w >= widths[i].1 - 0.001 && rect.w <= widths[i].0 + 0.001);
			for (j, other) in layout.rects.iter().enumerate() {
				let gain_i = widths[i].0 - rect.w;
				let gain_j = widths[j].0 - other.w;
				prop_assert!((gain_i * f32::from(specs[j].1) - gain_j * f32::from(specs[i].1)).abs() <= 0.001);
			}
			let revealed = layout.reveal(i);
			prop_assert!((0.0..=layout.max_scroll).contains(&revealed));
			if rect.w <= viewport.w {
				let shifted = rect.x + layout.scroll - revealed;
				prop_assert!(shifted >= viewport.x - 0.001 && shifted + rect.w <= viewport.x + viewport.w + 0.001);
			}
			x += rect.w + 2.0;
		}
		prop_assert!((TabSpan::new(&widths).end(viewport, layout.scroll) - (viewport.x + actual - layout.scroll)).abs() <= 0.001);
		let px = f32::from(probe) + 0.5;
		let expected = if viewport.x <= px && px <= viewport.x + viewport.w {
			layout.rects.iter().position(|rect| rect.x <= px && px <= rect.x + rect.w)
		} else { None };
		prop_assert_eq!(layout.hit(px, 25.0), expected);
		prop_assert_eq!(layout.reveal(specs.len()), layout.scroll);
	}

	#[test]
	fn drag_sequences_latch_and_shift_tabs_to_the_independently_declared_order(
		widths in prop::collection::vec(16u16..65, 1..10), dragged in any::<usize>(),
		pointers in prop::collection::vec(-100i16..701, 0..24),
	) {
		let widths: Vec<_> = widths.into_iter().map(|width| (f32::from(width), f32::from(width))).collect();
		let layout = TabLayout::new(Rect { x: 10.0, y: 0.0, w: 160.0, h: 30.0 }, &widths, 20.0);
		let index = dragged % widths.len();
		let start = layout.rects[index].x + 2.0;
		let mut drag = TabDrag { index, start, grab: 2.0, moving: false, slot: None };
		let mut moving = false;
		for x in [start, start + 4.0, start + 5.0, start].into_iter().chain(pointers.into_iter().map(f32::from)) {
			moving |= (x - start).abs() >= 5.0;
			drag.update(&layout, x);
			prop_assert_eq!(drag.moving, moving);
			let slot = layout.rects.iter().enumerate().filter(|(_, rect)| rect.x + rect.w * 0.5 > x).map(|(i, _)| i).min().unwrap_or(widths.len());
			prop_assert_eq!(drag.slot, moving.then_some(slot));
			let left: Vec<_> = (0..slot).filter(|&i| i != index).collect();
			let destination = left.len();
			prop_assert_eq!(drag.drop_index(), (moving && destination != index).then_some(destination));
			let shifts = drag.shift_targets(&layout, &widths);
			let order: Vec<_> = if moving { left.into_iter().chain([index]).chain((slot..widths.len()).filter(|&i| i != index)).collect() } else { (0..widths.len()).collect() };
			let mut target = layout.viewport.x - layout.scroll;
			for id in order {
				if id == index { prop_assert_eq!(shifts[id], 0.0); }
				else { prop_assert_eq!(layout.rects[id].x + shifts[id], target); }
				target += widths[id].0 + 2.0;
			}
		}
	}
}
