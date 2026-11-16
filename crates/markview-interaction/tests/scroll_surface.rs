use markview_interaction::{ScrollClock, ScrollSurface, WheelMode};
use proptest::prelude::*;
use std::{cell::Cell, rc::Rc, time::Duration};
use web_time::Instant;

fn surface(max: f32) -> ScrollSurface {
	let mut surface = ScrollSurface::default();
	surface.set_max(max);
	surface
}

#[test]
fn new_surfaces_wake_and_advance_without_host_registration() {
	let wakes = Rc::new(Cell::new(0));
	let wake = wakes.clone();
	let clock = ScrollClock::new(move || wake.set(wake.get() + 1));
	let start = Instant::now();
	let mut first = surface(600.0);
	let mut second = surface(300.0);
	first.apply(clock.wheel(120.0, start, WheelMode::Detent));
	second.apply(clock.step(80.0, start));
	assert_eq!(wakes.get(), 2);
	assert_eq!(first.offset(), 0.0);
	assert_eq!(second.offset(), 0.0);
	assert!(clock.animating());
	assert!(clock.advance(start + Duration::from_millis(40)));
	assert!(first.offset() > 0.0 && first.offset() < 120.0);
	assert!(second.offset() > 0.0 && second.offset() < 80.0);
	assert!(!clock.advance(start + Duration::from_secs(2)));
	assert_eq!(first.offset(), 120.0);
	assert_eq!(second.offset(), 80.0);
	assert!(!clock.animating());
}

#[test]
fn high_resolution_packets_keep_their_mode_and_momentum() {
	let clock = ScrollClock::new(|| {});
	let start = Instant::now();
	let mut surface = surface(600.0);
	for millis in [0, 16, 32] {
		let now = start + Duration::from_millis(millis);
		surface.apply(clock.wheel(40.0, now, WheelMode::Packet));
		assert!(clock.advance(now + Duration::from_millis(8)));
	}
	let before = surface.offset();
	assert!(before > 0.0 && before < 120.0);
	clock.advance(start + Duration::from_millis(80));
	assert!(surface.offset() > before);
	for frame in 11..1000 {
		if !clock.advance(start + Duration::from_millis(frame * 8)) {
			break;
		}
	}
	assert!(!clock.animating());
}

#[test]
fn direct_input_and_pointer_tracking_interrupt_easing() {
	let clock = ScrollClock::new(|| {});
	let start = Instant::now();
	for pointer in [false, true] {
		let mut surface = surface(600.0);
		surface.apply(clock.wheel(120.0, start, WheelMode::Detent));
		clock.advance(start + Duration::from_millis(40));
		let before = surface.offset();
		if pointer {
			surface.pan_by(10.0);
		} else {
			surface.apply(clock.wheel(
				10.0,
				start + Duration::from_millis(40),
				WheelMode::Direct,
			));
		}
		assert_eq!(surface.offset(), before + 10.0);
		assert!(!clock.advance(start + Duration::from_secs(2)));
		assert_eq!(surface.offset(), before + 10.0);
	}
}

#[test]
fn dropping_resetting_or_dragging_a_surface_releases_its_frames() {
	let clock = ScrollClock::new(|| {});
	let start = Instant::now();
	for action in 0..3 {
		let mut surface = surface(600.0);
		surface.apply(clock.wheel(120.0, start, WheelMode::Detent));
		assert!(clock.animating());
		match action {
			0 => drop(surface),
			1 => {
				surface.reset();
				assert_eq!(surface.offset(), 0.0);
			}
			_ => {
				surface.jump_to(75.0);
				clock.advance(start + Duration::from_secs(2));
				assert_eq!(surface.offset(), 75.0);
			}
		}
		assert!(!clock.animating());
		assert!(!clock.advance(start + Duration::from_secs(2)));
	}
}

#[test]
fn moving_to_a_different_clock_removes_the_previous_registration() {
	let old = ScrollClock::new(|| {});
	let new = ScrollClock::new(|| {});
	let start = Instant::now();
	let mut surface = surface(600.0);
	surface.apply(old.wheel(120.0, start, WheelMode::Detent));
	surface.apply(new.wheel(80.0, start, WheelMode::Detent));
	assert!(!old.animating());
	assert!(!old.advance(start + Duration::from_secs(2)));
	assert_eq!(surface.offset(), 0.0);
	assert!(new.advance(start + Duration::from_millis(40)));
	assert!(!new.advance(start + Duration::from_secs(2)));
	assert_eq!(surface.offset(), 200.0);
}

#[test]
fn navigation_clamps_its_destination_and_accumulates_pending_steps() {
	let clock = ScrollClock::new(|| {});
	let start = Instant::now();
	let mut surface = surface(300.0);
	surface.apply(clock.step(100.0, start));
	surface.apply(clock.step(100.0, start));
	clock.advance(start + Duration::from_secs(2));
	assert_eq!(surface.offset(), 200.0);
	for (target, expected) in [(f32::MAX, 300.0), (0.0, 0.0)] {
		surface.apply(clock.to(target, start));
		assert!(clock.advance(start + Duration::from_millis(40)));
		assert!(!clock.advance(start + Duration::from_secs(2)));
		assert_eq!(surface.offset(), expected);
	}
}

proptest! {
	#[test]
	fn routed_steps_stay_bounded_and_finish_without_per_surface_ticks(
		max in 1u16..2000,
		initial in 0u16..3000,
		deltas in prop::collection::vec(-1000i16..1000, 1..30),
	) {
		let clock = ScrollClock::new(|| {});
		let start = Instant::now();
		let max = f32::from(max);
		let mut surface = surface(max);
		surface.jump_to(f32::from(initial));
		let mut expected = f32::from(initial).min(max);
		for (index, delta) in deltas.into_iter().enumerate() {
			let now = start + Duration::from_millis(index as u64 * 16);
			expected = (expected + f32::from(delta)).clamp(0.0, max);
			surface.apply(clock.step(f32::from(delta), now));
			clock.advance(now + Duration::from_millis(8));
			prop_assert!((0.0..=max).contains(&surface.offset()));
		}
		prop_assert!(!clock.advance(start + Duration::from_secs(10)));
		prop_assert_eq!(surface.offset(), expected);
		prop_assert!(!clock.animating());
	}
}
