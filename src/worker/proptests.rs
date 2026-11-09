use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn first_prefix_publication_respects_size_time_and_coverage_boundaries(
		blocks in 1usize..65, target in 1u16..4097,
		bytes in 0usize..65_537, micros in 0u64..64_001,
	) {
		let target = f32::from(target);
		for bytes in [32 * 1024 - 1, 32 * 1024, bytes] {
			for elapsed in [0, 31_999, 32_000, micros] {
				for (height, covered) in [(target - 0.5, false), (target, true), (target + 0.5, true)] {
					let mut publication = PrefixPublication::new(bytes);
					let at = Duration::from_micros(elapsed);
					// Empty probes must not consume the initial publication opportunity.
					prop_assert!(!publication.publish(0, height, target, at));
					let expected = if bytes < 32 * 1024 { elapsed >= 32_000 } else { covered };
					prop_assert_eq!(publication.publish(blocks, height, target, at), expected);
				}
			}
		}
	}

	#[test]
	fn rejected_probes_preserve_batch_deadlines_and_changed_demand_bypasses_batching(
		initial in 1usize..33, target in 1u16..2049,
		start in 32_000u64..64_001, rounds in 1usize..6,
	) {
		let target = f32::from(target);
		for bytes in [0, 32 * 1024] {
			let mut publication = PrefixPublication::new(bytes);
			let mut blocks = initial;
			let mut at = start;
			prop_assert!(publication.publish(blocks, target, target, Duration::from_micros(at)));
			for _ in 0..rounds {
				// Both gates must open: elapsed time alone and growth alone are insufficient.
				for (count, elapsed) in [(blocks * 2, at + 31_999), (blocks * 2 - 1, at + 32_000)] {
					prop_assert!(!publication.publish(count, target, target, Duration::from_micros(elapsed)));
				}
				blocks *= 2;
				at += 32_000;
				prop_assert!(publication.publish(blocks, target, target, Duration::from_micros(at)));
				prop_assert!(!publication.publish(blocks, target, target, Duration::from_micros(at)));
			}
			let changed = target + 1.0;
			let now = Duration::from_micros(at + 1);
			prop_assert!(!publication.publish(blocks, changed - 0.5, changed, now));
			prop_assert!(publication.publish(blocks, changed, changed, now));
			prop_assert!(!publication.publish(blocks, changed, changed, now));
		}
	}
}
