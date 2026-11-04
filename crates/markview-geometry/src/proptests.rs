use super::*;
use proptest::prelude::*;

fn rect(v: (i16, i16, u8, u8)) -> Rect {
	Rect {
		x: v.0 as f32,
		y: v.1 as f32,
		w: v.2 as f32,
		h: v.3 as f32,
	}
}
proptest! {
	#[test]
	fn integer_rectangles_match_interval_model(a in (-100i16..100,-100i16..100,0u8..50,0u8..50), b in (-100i16..100,-100i16..100,0u8..50,0u8..50), point in (-150i16..150,-150i16..150)) {
		let (a, b) = (rect(a), rect(b));
		let left = (a.x as i32).max(b.x as i32);
		let right = (a.x as i32 + a.w as i32).min(b.x as i32 + b.w as i32);
		let top = (a.y as i32).max(b.y as i32);
		let bottom = (a.y as i32 + a.h as i32).min(b.y as i32 + b.h as i32);
		let expected = (right > left && bottom > top).then_some(Rect {
			x: left as f32,
			y: top as f32,
			w: (right - left) as f32,
			h: (bottom - top) as f32,
		});
		prop_assert_eq!(a.intersect(b), expected);
		prop_assert_eq!(b.intersect(a), expected);
		let (x, y) = (point.0 as f32, point.1 as f32);
		prop_assert_eq!(
			a.contains(x, y),
			a.w > 0.
				&& a.h > 0.
				&& (a.x..=a.x + a.w).contains(&x)
				&& (a.y..=a.y + a.h).contains(&y)
		);
		prop_assert_eq!(a.contains(a.x + a.w, a.y + a.h), a.w > 0. && a.h > 0.);
	}
	#[test]
	fn bounded_floats_intersection_bounds(a in (-1000f32..1000.,-1000f32..1000.,0f32..1000.,0f32..1000.), b in (-1000f32..1000.,-1000f32..1000.,0f32..1000.,0f32..1000.)) {
		let a = Rect {
			x: a.0,
			y: a.1,
			w: a.2,
			h: a.3,
		};
		let b = Rect {
			x: b.0,
			y: b.1,
			w: b.2,
			h: b.3,
		};
		prop_assert_eq!(a.intersect(b), b.intersect(a));
		if let Some(r) = a.intersect(b) {
			for outer in [a, b] {
				prop_assert!(r.x >= outer.x && r.y >= outer.y);
				// Four additions/subtractions at coordinates below 2000: 0.001 logical pixels exceeds four ULPs.
				prop_assert!(
					r.x + r.w <= outer.x + outer.w + 0.001
						&& r.y + r.h <= outer.y + outer.h + 0.001
				);
			}
		}
	}
}
#[test]
fn geometric_categories() {
	let a = Rect {
		x: 0.,
		y: 0.,
		w: 4.,
		h: 4.,
	};
	for (b, expected) in [
		(Rect { x: 5., ..a }, None),
		(Rect { x: 4., ..a }, None),
		(Rect { x: 2., ..a }, Some(Rect { x: 2., w: 2., ..a })),
		(a, Some(a)),
		(Rect { w: 0., ..a }, None),
		(Rect { w: -1., ..a }, None),
	] {
		assert_eq!(a.intersect(b), expected);
	}
}
