use super::*;
use proptest::prelude::*;
use std::time::Duration;
use web_time::Instant;

proptest! {
	#[test]
	fn coast_stops_below_the_speed_floor_or_above_the_frame_budget(
		half_speed in 0u16..6001, micros in 0u64..150_001,
		vertical in any::<bool>(), negative in any::<bool>(),
	) {
		let now = Instant::now();
		for speed in [9.5, 10.0, 10.5, f32::from(half_speed) / 2.0] {
			for elapsed in [0, 100_000, 100_001, micros] {
				let scalar = if negative { -speed } else { speed };
				let velocity = if vertical { (0.0, scalar) } else { (scalar, 0.0) };
				let mut motion = Motion::new(now);
				motion.velocity = velocity;
				let delta = motion.advance(now + Duration::from_micros(elapsed));
				prop_assert_eq!(delta.is_some(), speed >= 10.0 && elapsed <= 100_000);
				if let Some(delta) = delta {
					let (distance, remaining, other) = if vertical { (delta.1, motion.velocity.1, delta.0) } else { (delta.0, motion.velocity.0, delta.1) };
					prop_assert_eq!(other, 0.0);
					prop_assert!(remaining.abs() <= speed);
					prop_assert!(distance * scalar >= 0.0);
					if elapsed == 0 { prop_assert_eq!(delta, (0.0, 0.0)); prop_assert_eq!(motion.velocity, velocity); }
				}
			}
		}
	}

	#[test]
	fn release_requires_recent_motion_and_the_inclusive_speed_threshold(
		speed in 0u16..3001, nanos in 0u64..160_000_001,
		negative in any::<bool>(),
	) {
		let now = Instant::now();
		let sign = if negative { -1.0 } else { 1.0 };
		for (x, y, fast) in [(59.0, 0.0, false), (60.0, 0.0, true), (36.0, 48.0, true), (0.0, f32::from(speed), speed >= 60)] {
			for elapsed in [79_999_999, 80_000_000, 80_000_001, nanos] {
				let mut motion = Motion::new(now);
				motion.velocity = (x * sign, y * sign);
				// Stationary samples must not renew the last actual motion.
				motion.sample((0.0, 0.0), now + Duration::from_nanos(elapsed / 2));
				prop_assert_eq!(motion.release(now + Duration::from_nanos(elapsed)), fast && elapsed < 80_000_000);
			}
		}
	}

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
