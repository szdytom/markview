use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn animations_preserve_endpoints_bounds_direction_and_completion(
		from in -1000i16..1001, to in -1000i16..1001, millis in 1u64..1001,
		samples in prop::collection::vec(0u64..2002, 0..32),
	) {
		let start = Instant::now() + Duration::from_millis(1);
		let duration = Duration::from_millis(millis);
		let (from, to) = (f32::from(from), f32::from(to));
		let mut samples = samples;
		samples.extend([0, millis, millis + 1]);
		samples.sort_unstable();
		for easing in [Easing::CubicOut, Easing::ExpoOut, Easing::TabDrag] {
			let tween = Tween::new(from, to, start, duration, easing);
			prop_assert_eq!(tween.value(start - Duration::from_millis(1)), from);
			prop_assert_eq!(tween.value(start), from);
			prop_assert_eq!(tween.value(start + duration), to);
			let mut previous = from;
			for &elapsed in &samples {
				let now = start + Duration::from_millis(elapsed);
				let value = tween.value(now);
				prop_assert!((0.0..=1.0).contains(&tween.progress(now)));
				prop_assert!(value >= from.min(to) - 0.001 && value <= from.max(to) + 0.001);
				if to >= from { prop_assert!(value >= previous - 0.001); }
				else { prop_assert!(value <= previous + 0.001); }
				prop_assert_eq!(tween.is_finished(now), elapsed >= millis);
				previous = value;
			}
			let mut active = Some(Tween::new(from, to, start, duration, easing));
			prop_assert!(active.advance(start));
			prop_assert!(!active.advance(start + duration));
			prop_assert!(active.is_none());
			prop_assert!(!active.advance(start + duration));
		}
		let mut fade = Fade::new(from, to, start, duration);
		for &elapsed in &samples {
			let now = start + Duration::from_millis(elapsed);
			let t = elapsed.min(millis) as f64 / millis as f64;
			let expected = f64::from(from) * (1.0 - t) + f64::from(to) * t;
			prop_assert!((f64::from(fade.value(now)) - expected).abs() <= 0.001);
			prop_assert_eq!(fade.is_finished(now), elapsed >= millis);
			prop_assert_eq!(fade.advance(now), elapsed < millis);
		}
	}
}
