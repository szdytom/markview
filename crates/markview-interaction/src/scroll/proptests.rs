use super::*;
use proptest::prelude::*;

proptest! {
	#[test]
	fn pending_absolute_requests_survive_partial_layout(ops in prop::collection::vec((0u8..7,-200i16..200,any::<bool>()),0..64)) {
		let mut scroll = Scroll::default();
		let mut max = 0.;
		let mut complete = true;
		let mut requested = 0f32;
		let mut displayed = 0f32;
		let mut pending = false;
		for (step, (op, value, done)) in ops.into_iter().enumerate() {
			let value = value as f32;
			match op {
				0 | 1 => {
					max = value.max(0.);
					complete = op == 1 || done;
					if op == 1 {
						scroll.set_max(value)
					} else {
						scroll.fit(ScrollBounds {
							max: value,
							complete,
						})
					}
				}
				2 => {
					requested = value.max(0.);
					displayed = requested;
					pending = true;
					scroll.set(value);
				}
				3 => {
					requested = (requested + value).max(0.);
					pending = true;
					scroll.by(value);
				}
				4 => scroll.resolve(),
				5 => scroll.cancel(),
				_ => {
					requested = 0.;
					displayed = 0.;
					pending = false;
					scroll.reset();
				}
			}
			if pending && (complete || requested <= max) {
				requested = requested.clamp(0., max);
				displayed = requested;
				pending = false;
			}
			if complete {
				displayed = displayed.clamp(0., max);
				if !pending {
					requested = displayed;
				}
			}
			prop_assert_eq!(
				scroll.offset(),
				displayed.clamp(0., max),
				"step={}",
				step
			);
			prop_assert_eq!(
				scroll.target,
				pending.then_some(requested),
				"step={}",
				step
			);
		}
	}
	#[test]
	fn discrete_animation_settles_or_cancels(from in 0u16..1000,to in 0u16..1500,max in 0u16..1200, cancel in any::<bool>()) {
		let now = Instant::now();
		let mut scroll = Scroll::default();
		scroll.set_max(2000.);
		scroll.set(from as f32);
		scroll.animate_to(to as f32, now);
		prop_assert_eq!(scroll.offset(), from as f32);
		prop_assert_eq!(scroll.animating(), from != to);
		let a = ScrollAnimation::new(from as f32, to as f32, now);
		prop_assert_eq!(a.offset_at(now), from as f32);
		prop_assert_eq!(a.offset_at(a.end()), to as f32);
		let mut previous = from as f32;
		for tick in 0..=5 {
			let elapsed = a.duration.mul_f32(tick as f32 / 10.);
			let active = scroll.advance(now + elapsed);
			let value = scroll.offset();
			let t = elapsed.as_secs_f64() / a.duration.as_secs_f64();
			let expected = f64::from(from)
				+ (f64::from(to) - f64::from(from))
					* (3. * t - 3. * t * t + t * t * t);
			// Cubic evaluation below 1500 logical pixels stays within 0.001 pixels.
			prop_assert!((f64::from(value) - expected).abs() < 0.001);
			prop_assert_eq!(active, from != to);
			prop_assert_eq!(scroll.animating(), from != to);
			prop_assert_eq!(scroll.target, (from != to).then_some(to as f32));
			prop_assert!(
				value >= from.min(to) as f32 - 0.001
					&& value <= from.max(to) as f32 + 0.001
			);
			prop_assert!(
				(to >= from && value >= previous - 0.001)
					|| (to < from && value <= previous + 0.001)
			);
			previous = value;
		}
		scroll.fit(ScrollBounds {
			max: max as f32,
			complete: false,
		});
		if cancel {
			let position = scroll.offset();
			scroll.cancel();
			prop_assert!(!scroll.advance(now + Duration::from_secs(1)));
			prop_assert_eq!(scroll.offset(), position);
		} else {
			scroll.advance(now + Duration::from_secs(1));
			scroll.set_max(2000.);
			prop_assert_eq!(scroll.offset(), to as f32);
			prop_assert!(!scroll.animating());
		}
	}
}

#[test]
fn partial_relative_request_waits_without_moving() {
	let mut scroll = Scroll::default();
	scroll.fit(ScrollBounds {
		max: 1.,
		complete: false,
	});
	scroll.by(2.);
	assert_eq!(scroll.offset(), 0.);
	assert_eq!(scroll.target, Some(2.));
	scroll.cancel();
	assert_eq!(scroll.target, Some(2.));
	scroll.set_max(3.);
	assert_eq!(scroll.offset(), 2.);
	assert_eq!(scroll.target, None);
}

proptest! {
	#[test]
	fn animation_retains_and_retargets_pending_destination(prefix in 1u16..1000, gap in 1u16..500, delta in 1u16..500, cancel in any::<bool>()) {
		let now = Instant::now();
		let (prefix, goal, delta) =
			(prefix as f32, (prefix + gap) as f32, delta as f32);
		let mut scroll = Scroll::default();
		scroll.fit(ScrollBounds {
			max: prefix,
			complete: false,
		});
		scroll.animate_to(goal, now);
		let duration = scroll.animation.unwrap().duration;
		prop_assert_eq!(scroll.offset(), 0.);
		prop_assert!(scroll.advance(now + duration / 2));
		prop_assert!(scroll.animating());
		// The half-duration cubic covers 7/8 of the requested distance.
		prop_assert!((scroll.offset() - (goal * 0.875).min(prefix)).abs() < 0.001);
		prop_assert_eq!(scroll.target, Some(goal));
		prop_assert!(!scroll.advance(now + duration));
		prop_assert!(!scroll.animating());
		prop_assert_eq!(scroll.offset(), prefix);
		prop_assert_eq!(scroll.target, Some(goal));
		scroll.cancel();
		prop_assert_eq!(scroll.target, Some(goal));
		let restart = now + Duration::from_secs(1);
		scroll.animate_by(delta, restart);
		prop_assert!(scroll.animating());
		prop_assert_eq!(scroll.offset(), prefix);
		prop_assert_eq!(scroll.target, Some(goal + delta));
		let duration = scroll.animation.unwrap().duration;
		scroll.fit(ScrollBounds {
			max: goal + delta + 1.,
			complete: false,
		});
		scroll.resolve();
		prop_assert_eq!(scroll.offset(), prefix);
		prop_assert!(scroll.advance(restart + duration / 2));
		let expected = prefix + (goal + delta - prefix) * 0.875;
		prop_assert!((scroll.offset() - expected).abs() < 0.001);
		if cancel {
			let position = scroll.offset();
			scroll.cancel();
			prop_assert_eq!(scroll.target, None);
			prop_assert!(!scroll.advance(restart + duration));
			prop_assert_eq!(scroll.offset(), position);
		} else {
			prop_assert!(!scroll.advance(restart + duration));
			prop_assert_eq!(scroll.offset(), goal + delta);
			prop_assert_eq!(scroll.target, None);
		}
		prop_assert!(!scroll.animating());
	}
}
