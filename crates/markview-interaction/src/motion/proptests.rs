use super::*;
use proptest::prelude::*;
use std::time::Duration;
use web_time::Instant;

proptest! {
	#[test]
	fn coast_is_independent_of_frame_partition_and_conserves_integrated_distance(
		x in 100i16..3001, y in -3000i16..3001,
		a in 0u64..50_001, b in 0u64..50_001,
	) {
		let now = Instant::now();
		let velocity = (f32::from(x), f32::from(y));
		let mut whole = Motion::new(now);
		whole.velocity = velocity;
		let mut split = Motion::new(now);
		split.velocity = velocity;
		let first = split.advance(now + Duration::from_micros(a)).unwrap();
		let end = now + Duration::from_micros(a + b);
		let second = split.advance(end).unwrap();
		let total = whole.advance(end).unwrap();
		for (initial, final_velocity, split_velocity, distance, split_distance) in [
			(velocity.0, whole.velocity.0, split.velocity.0, total.0, first.0 + second.0),
			(velocity.1, whole.velocity.1, split.velocity.1, total.1, first.1 + second.1),
		] {
			// Bounds are in logical pixels and pixels/second, respectively.
			prop_assert!((distance - split_distance).abs() <= 0.002);
			prop_assert!((final_velocity - split_velocity).abs() <= 0.01);
			prop_assert!((distance - 0.22 * (initial - final_velocity)).abs() <= 0.002);
			prop_assert!(final_velocity.abs() <= initial.abs());
			prop_assert!(initial == 0.0 || final_velocity.signum() == initial.signum());
		}
	}

	#[test]
	fn sampling_is_bounded_and_release_expires_after_eighty_milliseconds(
		deltas in prop::collection::vec((-20_000i32..20_001, -20_000i32..20_001, 0u64..101), 0..32),
	) {
		let mut now = Instant::now();
		let mut motion = Motion::new(now);
		for (x, y, millis) in deltas {
			let previous = motion.velocity;
			now += Duration::from_millis(millis);
			motion.sample((x as f32, y as f32), now);
			prop_assert!(motion.velocity.0.abs() <= 3000.0 && motion.velocity.1.abs() <= 3000.0);
			if millis == 0 || (x == 0 && y == 0) {
				prop_assert_eq!(motion.velocity, previous);
			}
		}
		// A fresh strong sample gives an unambiguous release above the speed threshold.
		now += Duration::from_millis(10);
		motion.sample((1000.0, 0.0), now);
		prop_assert!(motion.release(now + Duration::from_millis(79)));
		prop_assert!(!motion.release(now + Duration::from_millis(80)));
		prop_assert!(motion.advance(now + Duration::from_millis(181)).is_none());
	}
}
