use super::*;
use proptest::prelude::*;

#[test]
fn wheel_retarget_comparison_reports_discontinuity_and_response_time() {
	let start = Instant::now();
	let arrival = start + Duration::from_millis(45);
	let last_frame = start + Duration::from_millis(32);
	let baseline = ScrollAnimation::new(0.0, 120.0, start);
	let stale = baseline.offset_at(last_frame);
	let restarted = ScrollAnimation::new(stale, 240.0, arrival);
	let lost = baseline.offset_at(arrival) - stale;
	let old_jump =
		restarted.velocity_at(arrival) - baseline.velocity_at(arrival);
	let mut scroll = Scroll::default();
	scroll.set_max(2000.0);
	scroll.wheel_by(120.0, start);
	let spring = scroll.animation.unwrap();
	scroll.advance(last_frame);
	scroll.wheel_by(120.0, arrival);
	let new_jump = scroll.animation.unwrap().velocity_at(arrival)
		- spring.velocity_at(arrival);
	assert!(lost > 10.0);
	assert!(old_jump > 1000.0);
	assert_eq!(scroll.offset, spring.offset_at(arrival));
	assert!(new_jump.abs() < 1e-8);
	println!(
		"Retarget at 45 ms, last frame at 32 ms: old lost={lost:.3} px, old velocity jump={old_jump:.3} px/s, new lost=0 px, new velocity jump={new_jump:.6} px/s"
	);
	for (name, animation) in [("old cubic", baseline), ("new spring", spring)] {
		let first_frame =
			animation.offset_at(start + Duration::from_secs_f64(1.0 / 60.0));
		let crossing = |fraction: f32| {
			(0..=2000)
				.find(|ms| {
					animation.offset_at(start + Duration::from_millis(*ms))
						>= 120.0 * fraction
				})
				.unwrap()
		};
		let settled = (0..=2000)
			.find(|ms| animation.finished(start + Duration::from_millis(*ms)))
			.unwrap();
		println!(
			"{name}: first 60 Hz frame={first_frame:.3} px, 50%={} ms, 95%={} ms, settled={settled} ms",
			crossing(0.5),
			crossing(0.95)
		);
	}
}

proptest! {
	#[test]
	fn wheel_velocity_limit_prevents_overshoot(
		distance in 1u16..2000, excess in 1u16..1000, reverse in any::<bool>(),
	) {
		let start = Instant::now();
		let direction = if reverse { -1.0 } else { 1.0 };
		let to = 3000.0 + f32::from(distance) * direction;
		let limit = f64::from(distance) * WHEEL_FREQUENCY * f64::from(direction);
		let animation = ScrollAnimation::wheel(3000.0, to, limit * f64::from(excess + 1), start);
		prop_assert!((animation.velocity_at(start) - limit).abs() < 1e-8);
		for ms in 0..=1000 {
			let offset = animation.offset_at(start + Duration::from_millis(ms));
			prop_assert!(offset >= 3000.0_f32.min(to) && offset <= 3000.0_f32.max(to));
		}
	}

	#[test]
	fn same_direction_detents_conserve_total_distance(
		packets in prop::collection::vec((1u16..300, 1u16..300), 1..40),
		reverse in any::<bool>(),
	) {
		let direction = if reverse { -1.0 } else { 1.0 };
		let mut scroll = Scroll::default();
		scroll.set_max(40000.0);
		scroll.set(20000.0);
		let mut at = Instant::now();
		let mut total = 0.0;
		for (gap, delta) in packets {
			at += Duration::from_millis(u64::from(gap));
			let delta = f32::from(delta) * direction;
			total += delta;
			scroll.wheel_by(delta, at);
			prop_assert_eq!(scroll.target, Some(20000.0 + total));
		}
		scroll.advance(at + Duration::from_secs(2));
		prop_assert_eq!(scroll.offset, 20000.0 + total);
		prop_assert!(!scroll.animating());
	}

	#[test]
	fn wheel_springs_preserve_velocity_and_settle_without_overshoot(
		distance in 1u16..2000, speed_fraction in 0u16..1000, reverse in any::<bool>(),
	) {
		let start = Instant::now();
		let direction = if reverse { -1.0 } else { 1.0 };
		let from = 3000.0;
		let to = from + f32::from(distance) * direction;
		let velocity = f64::from(distance) * WHEEL_FREQUENCY
			* f64::from(speed_fraction) / 1000.0 * f64::from(direction);
		let animation = ScrollAnimation::wheel(from, to, velocity, start);
		prop_assert_eq!(animation.offset_at(start), from);
		prop_assert!((animation.velocity_at(start) - velocity).abs() < 1e-8);
		let mut previous = from;
		for step in 1..=100 {
			let at = start + animation.duration.mul_f32(step as f32 / 100.0);
			let offset = animation.offset_at(at);
			prop_assert!(offset >= from.min(to) && offset <= from.max(to));
			prop_assert!((offset - previous) * direction >= 0.0);
			previous = offset;
		}
		prop_assert_eq!(animation.offset_at(animation.end()), to);
		prop_assert_eq!(animation.velocity_at(animation.end()), 0.0);
		prop_assert!(animation.finished(animation.end()));
	}

	#[test]
	fn wheel_event_traces_are_independent_of_frame_partition(
		packets in prop::collection::vec((1u16..120, -200i16..200), 1..40),
	) {
		let mut dense = Scroll::default();
		dense.set_max(2000.0);
		dense.set(1000.0);
		let mut sparse = dense.clone();
		let mut at = Instant::now();
		for (gap, delta) in packets {
			let arrival = at + Duration::from_millis(u64::from(gap));
			for ms in 1..gap {
				dense.advance(at + Duration::from_millis(u64::from(ms)));
			}
			dense.wheel_by(f32::from(delta), arrival);
			sparse.wheel_by(f32::from(delta), arrival);
			// A zero packet is a no-op; sample both traces at the same time.
			dense.advance(arrival);
			sparse.advance(arrival);
			prop_assert!((dense.offset - sparse.offset).abs() < 0.001);
			prop_assert_eq!(dense.target, sparse.target);
			at = arrival;
		}
		dense.advance(at + Duration::from_secs(2));
		sparse.advance(at + Duration::from_secs(2));
		prop_assert_eq!(dense.offset, sparse.offset);
		prop_assert!(!dense.animating() && !sparse.animating());
	}

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
