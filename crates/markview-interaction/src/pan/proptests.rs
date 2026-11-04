use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn gesture_intervals_conserve_motion(events in prop::collection::vec((0u8..8, -1000i16..1000, -1000i16..1000), 0..96)) {
		let mut feed = PanFeed::default();
		let mut motion: Option<(f32, f32)> = None;
		for (step, (op, x, y)) in events.into_iter().enumerate() {
			if op < 3 {
				let status =
					[PanStatus::Building, PanStatus::Running, PanStatus::Inertia]
						[op as usize];
				let expected = motion.is_none().then_some(PanPhase::Started);
				prop_assert_eq!(feed.status(status), expected, "step={}", step);
				motion.get_or_insert((0., 0.));
			} else if op < 6 {
				let expected = motion.take().map(|_| {
					if op == 3 {
						PanPhase::Ended
					} else {
						PanPhase::Cancelled
					}
				});
				let actual = match op {
					3 => feed.status(PanStatus::Ready),
					4 => feed.status(PanStatus::Disabled),
					_ => feed.abandon(),
				};
				prop_assert_eq!(actual, expected, "step={}", step);
			} else {
				let position = (x as f32 / 8., y as f32 / 8.);
				let delta = feed.transform(position.0, position.1);
				match motion.as_mut() {
					Some(total) => {
						let delta = delta.unwrap();
						total.0 += delta.0;
						total.1 += delta.1;
						prop_assert_eq!(*total, position, "step={}", step);
					}
					None => prop_assert_eq!(delta, None, "step={}", step),
				}
			}
		}
	}
}

proptest! {
	#[test]
	fn general_float_deltas_telescope(points in prop::collection::vec((-1000f32..1000.,-1000f32..1000.),1..64)) {
		let mut feed = PanFeed::default();
		feed.status(PanStatus::Running);
		let mut sum = (0f64, 0f64);
		for (step, &(x, y)) in points.iter().enumerate() {
			let (dx, dy) = feed.transform(x, y).unwrap();
			sum.0 += f64::from(dx);
			sum.1 += f64::from(dy);
			// Each subtraction below 2000 pixels contributes less than 0.00025 pixels.
			let tolerance = (step + 1) as f64 * 0.00025;
			prop_assert!(
				(sum.0 - f64::from(x)).abs() <= tolerance
					&& (sum.1 - f64::from(y)).abs() <= tolerance
			);
		}
	}
}
