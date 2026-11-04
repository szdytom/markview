use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn solver_respects_capacity(natural in 0f32..1000., target in 0f32..1000., stretch in 0u16..100, shrink in 0u16..100, shares in 0usize..20) {
		let (stretch, shrink) = (stretch as f32, shrink as f32);
		let j = solve(natural, target, stretch, shrink, shares);
		let delta = target - natural;
		if delta < 0. {
			prop_assert!(j.ratio < 0.);
			prop_assert_eq!(j.extra, 0.);
		} else {
			prop_assert!((0. ..=1.).contains(&j.ratio));
			prop_assert!(j.extra >= 0.);
			if delta <= stretch {
				prop_assert_eq!(j.extra, 0.);
			}
		}
		let expected = if delta < 0. && shrink == 0. {
			0.
		} else if delta > stretch && shares == 0 {
			stretch
		} else {
			delta
		};
		let actual = if j.ratio < 0. {
			j.ratio * shrink
		} else {
			j.ratio * stretch + j.extra * shares as f32
		};
		// At most three rounded operations over widths below 1000 logical pixels.
		prop_assert!(
			(actual - expected).abs() < 0.001,
			"j={:?}, expected={}, actual={}",
			j,
			expected,
			actual
		);
	}
	#[test]
	fn gained_matches_per_cluster_sum(stretch in 0u16..64, shrink in 0u16..64, shares in 0usize..16, ratio in -16i16..17, extra in 0u16..64) {
		let j = Justify {
			ratio: ratio as f32 / 8.,
			extra: extra as f32 / 8.,
		};
		let expected = if ratio < 0 {
			(0..shrink).map(|_| j.ratio).sum::<f32>()
		} else {
			(0..stretch)
				.map(|_| j.ratio)
				.chain((0..shares).map(|_| j.extra))
				.sum::<f32>()
		};
		prop_assert_eq!(gained(stretch as f32, shrink as f32, shares, j), expected);
	}
}

#[test]
fn exact_capacity_boundaries_and_unfillable_requests() {
	for (target, stretch, shrink, shares, ratio, extra, gain) in [
		(100., 0., 0., 0, 0., 0., 0.),
		(110., 10., 0., 0, 1., 0., 10.),
		(120., 10., 0., 2, 1., 5., 20.),
		(120., 10., 0., 0, 1., 10., 10.),
		(90., 0., 0., 2, -1., 0., 0.),
		(90., 0., 5., 0, -2., 0., -10.),
	] {
		let expected = Justify { ratio, extra };
		assert_eq!(solve(100., target, stretch, shrink, shares), expected);
		assert_eq!(gained(stretch, shrink, shares, expected), gain);
	}
}
