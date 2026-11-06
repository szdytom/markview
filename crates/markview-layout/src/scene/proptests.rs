use super::*;
use proptest::prelude::*;

fn shapes(x: f32, y: f32, depth: usize) -> Vec<Draw> {
	let rect = Rect {
		x,
		y,
		w: 30.0,
		h: 20.0,
	};
	let mut draws = vec![
		Draw::Rect(rect, Paint::Accent),
		Draw::FadedRect {
			rect,
			paint: Paint::Muted,
			opacity: 0.5,
		},
		Draw::Image {
			src: "image".to_owned(),
			version: 7,
			rect,
			title: "title".to_owned(),
		},
		Draw::Box {
			rect,
			chain: 0,
			condition: crate::style::Condition::Body,
			radius: 2.0,
			border: 1.0,
			left_only: false,
			decoration: Some(std::sync::Arc::new(BoxDecoration {
				edges: [1.0, 2.0, 3.0, 4.0],
				corners: [2.0; 4],
			})),
		},
		Draw::Polygon {
			center: [x, y],
			points: Arc::from([[1.0, 2.0], [-1.0, -2.0]]),
			paint: Paint::Text,
		},
		Draw::Icon {
			paths: &[],
			paint: Paint::Text,
			x,
			y,
			size: 8.0,
		},
	];
	if depth > 0 {
		draws.push(Draw::Clipped {
			rect,
			draws: shapes(x + 3.0, y + 4.0, depth - 1),
		});
	}
	draws
}

proptest! {
	#[test]
	fn scrollbar_drag_positions_match_scroll_fractions_on_both_axes(
		start in -100i16..101, length in 32u16..513, band in 1u16..25,
		viewport in 16u16..513, extra in 8u16..2049, fraction in 0u16..257, grab_fraction in 1u16..256,
	) {
		for vertical in [false, true] {
			let (start, length, band, viewport, extra) = (f32::from(start), f32::from(length), f32::from(band), f32::from(viewport), f32::from(extra));
			let track = if vertical { Rect { x: 0.0, y: start, w: band, h: length } } else { Rect { x: start, y: 0.0, w: length, h: band } };
			let create = if vertical { Scrollbar::vertical } else { Scrollbar::horizontal };
			prop_assert!(create(track, 0.0, viewport, viewport, ScrollbarMetrics::default()).is_none());
			let desired = extra * f32::from(fraction) / 256.0;
			let bar = create(track, desired, viewport + extra, viewport, ScrollbarMetrics::default()).unwrap();
			let (thumb_start, thumb_len) = if vertical { (bar.thumb.y, bar.thumb.h) } else { (bar.thumb.x, bar.thumb.w) };
			let travel = length - thumb_len;
			prop_assert_eq!(bar.max_scroll(), extra);
			prop_assert!(thumb_len >= 24.0 && thumb_len <= length);
			prop_assert!(thumb_start >= start && thumb_start + thumb_len <= start + length + 0.0001);
			if length * viewport > 24.0 * (viewport + extra) {
				prop_assert!((thumb_len * (viewport + extra) - length * viewport).abs() <= 0.05);
			} else { prop_assert_eq!(thumb_len, 24.0); }
			let grab = thumb_len * f32::from(grab_fraction) / 256.0;
			let point = |along| if vertical { (band * 0.5, along) } else { (along, band * 0.5) };
			let (x, y) = point(thumb_start + grab);
			prop_assert!(bar.hit(x, y));
			prop_assert!(bar.on_thumb(x, y));
			// A quarter-millipixel pointer error is amplified by the scroll/travel ratio.
			let tolerance = 0.001 + 0.00025 * extra / travel;
			let actual_grab = bar.grab(x, y);
			prop_assert!((actual_grab - grab).abs() <= 0.00025);
			prop_assert!((bar.scroll_for(x, y, actual_grab) - desired).abs() <= tolerance);
			let (x, y) = point(start + travel * f32::from(fraction) / 256.0 + grab);
			prop_assert!((bar.scroll_for(x, y, grab) - desired).abs() <= tolerance);
			for (along, expected) in [(start - 10_000.0, 0.0), (start + length + 10_000.0, extra)] {
				let (x, y) = point(along);
				prop_assert_eq!(bar.scroll_for(x, y, grab), expected);
			}
			let (painted, thumb) = bar.bars();
			if vertical {
				prop_assert_eq!((painted.y, painted.h, thumb.y, thumb.h), (track.y, track.h, bar.thumb.y, bar.thumb.h));
				prop_assert_eq!(painted.w, band.min(8.0));
			} else {
				prop_assert_eq!((painted.x, painted.w, thumb.x, thumb.w), (track.x, track.w, bar.thumb.x, bar.thumb.w));
				prop_assert_eq!(painted.h, band.min(8.0));
			}
		}
		for length in [0.0, 1.0, 24.0] {
			let track = Rect { x: 0.0, y: 0.0, w: 8.0, h: length };
			if let Some(bar) = Scrollbar::vertical(track, 100.0, 200.0, 100.0, ScrollbarMetrics::default()) {
				prop_assert_eq!(bar.thumb.h, length);
				prop_assert_eq!(bar.scroll_for(0.0, length, 0.0), 0.0);
			} else { prop_assert_eq!(length, 0.0); }
		}
	}

	#[test]
	fn corner_and_edge_fitting_preserve_proportions_without_overlapping(
		width in 0u16..257, height in 0u16..257, values in prop::array::uniform4(0u16..65),
	) {
		let rect = Rect { x: 0.0, y: 0.0, w: f32::from(width), h: f32::from(height) };
		let input = values.map(f32::from);
		let corners = fit_corners(rect, input);
		let edges = fit_edges(rect, input);
		for i in 0..4 {
			prop_assert!(corners[i] >= 0.0 && corners[i] <= input[i]);
			prop_assert!(edges[i] >= 0.0 && edges[i] <= input[i]);
			for j in 0..4 { prop_assert!((corners[i] * input[j] - corners[j] * input[i]).abs() <= 0.001); }
		}
		for (a, b, extent) in [(0, 1, rect.w), (3, 2, rect.w), (0, 3, rect.h), (1, 2, rect.h)] {
			prop_assert!(corners[a] + corners[b] <= extent + 0.0001);
		}
		let sides = [(0, 1, rect.w), (3, 2, rect.w), (0, 3, rect.h), (1, 2, rect.h)];
		if sides.iter().all(|&(a, b, extent)| input[a] + input[b] <= extent) {
			prop_assert_eq!(corners, input);
		} else {
			prop_assert!(sides.iter().any(|&(a, b, extent)| (corners[a] + corners[b] - extent).abs() <= 0.0001));
		}
		for (a, b, extent) in [(0, 2, rect.h), (1, 3, rect.w)] {
			if extent > 0.0 { prop_assert!(edges[a] + edges[b] <= extent + 0.0001); }
			else { prop_assert_eq!((edges[a], edges[b]), (input[a], input[b])); }
			if extent > 0.0 && input[a] + input[b] > extent {
				prop_assert!((edges[a] + edges[b] - extent).abs() <= 0.0001);
			} else { prop_assert_eq!((edges[a], edges[b]), (input[a], input[b])); }
			prop_assert!((edges[a] * input[b] - edges[b] * input[a]).abs() <= 0.001);
		}
	}

	#[test]
	fn viewport_coordinates_use_the_declared_column_and_scroll_origins(
		left in 0i16..100, top in 0i16..100, bottom in 0i16..100, scroll in 0i16..1000,
		x in -100i16..1001, y in -100i16..1001,
	) {
		let viewport = Viewport { width: 640.0, height: 480.0, left: f32::from(left), top: f32::from(top), bottom: f32::from(bottom), scroll: f32::from(scroll) };
		let (wx, wy) = (f32::from(x), f32::from(y));
		prop_assert_eq!(viewport.document_point(wx, wy), (f32::from(x - left), f32::from(y - top + scroll)));
		prop_assert_eq!(viewport.clip().contains(wx, wy), (0..=640).contains(&x) && top <= y && y <= 480 - bottom);
		let rect = viewport.window_rect(Rect { x: wx, y: wy, w: 12.0, h: 13.0 });
		prop_assert_eq!(rect, Rect { x: f32::from(x + left), y: f32::from(y + top - scroll), w: 12.0, h: 13.0 });
	}

	#[test]
	fn drawing_translation_moves_absolute_nested_coordinates_and_preserves_other_fields(
		x in -256i16..257, y in -256i16..257, dx in -128i16..129, dy in -128i16..129, depth in 0usize..4,
	) {
		let mut actual = shapes(f32::from(x), f32::from(y), depth);
		for draw in &mut actual { draw.translate(f32::from(dx), f32::from(dy)); }
		let expected = shapes(f32::from(x + dx), f32::from(y + dy), depth);
		prop_assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
	}
}
