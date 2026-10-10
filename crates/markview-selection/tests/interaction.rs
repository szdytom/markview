use markview_core::scene::{
	BlockLayout, Draw, LayoutSnapshot, LinkRect, Overflow, PlacedBlock, Rect,
	ScrollbarMetrics, Viewport,
};
use markview_selection::{
	Cursor, DocumentInteraction, Horizontal, Motion, Point, ScrollBounds,
	ScrollState, horizontal_by, selection_scroll,
};
use std::{sync::Arc, time::Duration};
use web_time::Instant;

fn snapshot() -> LayoutSnapshot {
	let rect = Rect {
		x: 0.0,
		y: 0.0,
		w: 100.0,
		h: 30.0,
	};
	LayoutSnapshot {
		height: 100.0,
		width: 100.0,
		blocks: vec![PlacedBlock {
			flow: Default::default(),
			id: 1,
			source: 0..0,
			y: 0.0,
			layout: Arc::new(BlockLayout {
				height: 100.0,
				draws: vec![Draw::Image {
					rect,
					src: "image".into(),
					version: 1,
					title: "Image title".into(),
				}],
				links: vec![LinkRect {
					rect: Rect { w: 30.0, ..rect },
					url: "https://example.com".into(),
					command: 0,
				}],
				overflow: vec![Overflow {
					rect,
					content_width: 300.0,
					gutter: 12.0,
					commands: 0..1,
				}],
				..Default::default()
			}),
		}],
		..Default::default()
	}
}

#[test]
fn hover_and_hits_follow_viewport_and_horizontal_motion() {
	let snapshot = snapshot();
	let mut horizontal = Horizontal::default();
	let viewport = Viewport {
		width: 200.0,
		height: 100.0,
		left: 20.0,
		top: 10.0,
		bottom: 10.0,
		scroll: 0.0,
	};
	let point = Point::new(25.0, 15.0);
	let metrics = ScrollbarMetrics::default();
	let context = DocumentInteraction {
		snapshot: &snapshot,
		viewport,
		horizontal: &horizontal,
		revision: 1,
	};
	assert_eq!(
		context.hover(point, false, false, metrics).cursor,
		Cursor::Pointer
	);
	assert!(context.hover(point, true, true, metrics).link.is_none());
	let image = Point::new(80.0, 15.0);
	assert_eq!(
		context.hover(image, false, false, metrics).cursor,
		Cursor::Default
	);
	assert_eq!(
		context.hover(image, false, true, metrics).cursor,
		Cursor::Pointer
	);
	assert_eq!(
		context
			.hover(image, false, true, metrics)
			.image_title
			.as_deref(),
		Some("Image title")
	);
	let bar = context.overflow_bar(0, 0, metrics).unwrap();
	let hit = Point::new(
		bar.track.x + bar.track.w * 0.5,
		bar.track.y + bar.track.h * 0.5,
	);
	assert!(context.overflow_bar_at(hit, metrics).is_some());
	assert_eq!(
		context.hover(hit, false, false, metrics).overflow,
		Some((0, 0))
	);
	assert!(context.link(Point::new(25.0, 5.0)).is_none());
	assert!(horizontal_by(
		&snapshot,
		viewport,
		&mut horizontal,
		point,
		500.0
	));
	assert_eq!(horizontal[&(0, 0)], 200.0);
	let context = DocumentInteraction {
		snapshot: &snapshot,
		viewport,
		horizontal: &horizontal,
		revision: 1,
	};
	assert!(context.link(point).is_none());
	let context = DocumentInteraction {
		viewport: Viewport {
			scroll: 50.0,
			..viewport
		},
		..context
	};
	assert!(context.link(point).is_none());
}

#[test]
fn external_input_takes_over_and_pending_targets_survive_layout() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 1000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	scroll.wheel_by(300.0, start);
	scroll.wheel_by(300.0, start);
	assert_eq!(scroll.target, Some(600.0));
	scroll.advance(start + Duration::from_millis(40), bounds);
	let visible = scroll.offset;
	scroll.wheel_by(-20.0, start + Duration::from_millis(40));
	assert_eq!(scroll.target, Some(visible - 20.0));
	scroll.by(10.0, bounds);
	assert_eq!(scroll.offset, visible + 10.0);
	assert!(scroll.animation.is_none());
	let prefix = ScrollBounds {
		max: 100.0,
		complete: false,
	};
	scroll.set(900.0, prefix);
	assert_eq!(scroll.visible(prefix), 100.0);
	let growing = ScrollBounds {
		max: 500.0,
		complete: false,
	};
	scroll.resolve(growing);
	assert_eq!(scroll.offset, 900.0);
	assert_eq!(scroll.visible(growing), 500.0);
	assert_eq!(scroll.target, Some(900.0));
	scroll.resolve(bounds);
	assert_eq!(scroll.offset, 900.0);
	scroll.target = Some(f32::INFINITY);
	scroll.by(0.0, prefix);
	assert_eq!(scroll.target, Some(f32::INFINITY));
	scroll.resolve(bounds);
	assert_eq!(scroll.offset, 1000.0);
}

#[test]
fn gesture_inertia_and_selection_edges_stop() {
	let start = Instant::now();
	let mut motion = Motion::new(start);
	motion.sample((0.0, 30.0), start + Duration::from_millis(20));
	assert!(motion.release(start + Duration::from_millis(25)));
	assert!(motion.advance(start + Duration::from_millis(40)).unwrap().1 > 0.0);
	assert!(motion.advance(start + Duration::from_secs(1)).is_none());
	assert_eq!(selection_scroll(5.0, 10.0, 100.0, 0.0, 500.0), 0.0);
	assert_eq!(selection_scroll(5.0, 10.0, 100.0, 50.0, 500.0), -14.0);
	assert_eq!(selection_scroll(110.0, 10.0, 100.0, 50.0, 500.0), 14.0);
	assert_eq!(selection_scroll(110.0, 10.0, 100.0, 500.0, 500.0), 0.0);
}

#[test]
fn wheel_packet_stream_rides_across_gaps_and_settles() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A fast two-finger scroll: packets 40 ms apart, with the frame loop
	// running between them as the event loop would.
	let mut at = start;
	for _ in 0..5 {
		at += Duration::from_millis(40);
		scroll.coast_wheel_by(120.0, at);
		for step in 1..=4 {
			scroll.advance(at + Duration::from_millis(step * 8), bounds);
		}
	}
	// The stream keeps a speed of its own, so the page has ridden well past
	// where easing each packet from a standstill would have left it.
	assert!(
		scroll.offset > 300.0,
		"the page should ride the stream, at {}",
		scroll.offset
	);
	assert_eq!(scroll.target, Some(600.0));
	// The packets stop and the stream goes quiet: the lead is lifted and the
	// speed dies at the faster quiet rate, so the page settles within a beat,
	// keeping only the lead and the tail it travelled — never a long coast.
	let mut frame = at;
	for _ in 0..150 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(
		!scroll.animating(),
		"the quiet stream should settle, at {}",
		scroll.offset
	);
	assert!(
		scroll.offset > 600.0 && scroll.offset < 900.0,
		"the page rests near the packets, at {}",
		scroll.offset
	);
	assert_eq!(scroll.target, None);
	// What settled stays put.
	let rested = scroll.offset;
	for _ in 0..50 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
	}
	assert_eq!(scroll.offset, rested);
}

#[test]
fn a_quiet_wheel_stream_stops_with_the_hand() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 20000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A two-finger scroll rides the stream, and then the hand rests on the
	// pad without lifting: the packets stop, and nothing announces the stop.
	let mut at = start;
	for _ in 0..8 {
		at += Duration::from_millis(40);
		scroll.coast_wheel_by(120.0, at);
		for step in 1..=4 {
			scroll.advance(at + Duration::from_millis(step * 8), bounds);
		}
	}
	// The speed the stream was carrying is still high, so only the quiet decay
	// can still the page — and at its faster rate the page must look stopped
	// within a beat instead of riding the old quarter-second coast.
	let mut frame = at;
	for _ in 0..150 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(
		!scroll.animating(),
		"the page should have settled, at {}",
		scroll.offset
	);
	assert!(
		(scroll.offset - 960.0).abs() < 1.0,
		"the page rests at the packets' distance, at {}",
		scroll.offset
	);
	assert_eq!(scroll.target, None);
	// What stopped stays stopped.
	let rested = scroll.offset;
	for _ in 0..50 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
	}
	assert_eq!(scroll.offset, rested);
}

#[test]
fn a_flush_after_a_silence_continues_the_stream() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 20000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A batchy touchpad hands one batch of motion over, then falls silent,
	// then flushes what it accumulated in the silence — same direction, one
	// large packet, a third of a second later. The stream must continue
	// through the boundary: no dead stop, no restart from a standstill.
	let at = start + Duration::from_millis(16);
	for step in 0..3 {
		scroll.coast_wheel_by(80.0, at + Duration::from_millis(step * 8));
	}
	for step in 1..=4 {
		scroll.advance(at + Duration::from_millis(24 + step * 8), bounds);
	}
	let before_flush = scroll.offset;
	// The silence: frames run while nothing arrives, the speed decays but the
	// stream stays alive.
	let mut frame = at + Duration::from_millis(56);
	while frame < at + Duration::from_millis(376) {
		assert!(
			scroll.advance(frame, bounds),
			"the stream died in the silence at {frame:?}"
		);
		frame += Duration::from_millis(8);
	}
	// The flush: distance the hand travelled during the silence. The stream
	// takes it as a continuation — the page glides through it from where it
	// is, and the silence's own average caps the speed it is believed to
	// have had.
	scroll.coast_wheel_by(400.0, at + Duration::from_millis(376));
	let mut frame = at + Duration::from_millis(376);
	// The flush distance arrives, plus only the bridged speed's small tail.
	for _ in 0..400 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(!scroll.animating());
	assert!(
		scroll.offset > before_flush + 400.0
			&& scroll.offset < before_flush + 260.0 + 400.0,
		"the flush should land as a glide past {}, at {}",
		before_flush + 400.0,
		scroll.offset
	);
	assert_eq!(scroll.target, None);
}

#[test]
fn a_late_flush_keeps_a_sane_speed() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 40000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A fast swipe, then the hand lifts and the OS holds the inertia back.
	let mut at = start;
	for _ in 0..8 {
		at += Duration::from_millis(40);
		scroll.coast_wheel_by(120.0, at);
		for step in 1..=4 {
			scroll.advance(at + Duration::from_millis(step * 8), bounds);
		}
	}
	// The silence: the stream decays while nothing arrives.
	let mut frame = at;
	for _ in 0..37 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
	}
	// The flush: detents of accumulated motion, a third of a second late.
	// Its `distance / gap` would name a speed no hand ever moved at; the
	// stream keeps the speed it decayed to, and the capped chase pays the
	// distance as a glide.
	let before_flush = scroll.offset;
	scroll.coast_wheel_by(5000.0, at + Duration::from_millis(300));
	let mut frame = at + Duration::from_millis(300);
	let mut last = scroll.offset;
	for _ in 0..12 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
		// The first frame after the flush spans the silence's last step too,
		// so it may carry twelve milliseconds of the capped chase; a snapped
		// speed would carry far more.
		assert!(
			scroll.offset - last <= 8000.0 * 0.0125,
			"the flush lurched {} px in one frame",
			scroll.offset - last
		);
		last = scroll.offset;
	}
	// The flush's distance arrives from where the page was, with only the
	// bridged speed's small tail past it.
	for _ in 0..600 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(!scroll.animating());
	assert!(
		scroll.offset > before_flush + 5000.0 - 1.0
			&& scroll.offset < before_flush + 5100.0,
		"the flush should land as a glide at {}, at {}",
		before_flush + 5000.0,
		scroll.offset
	);
	assert_eq!(scroll.target, None);
}

#[test]
fn a_flush_burst_does_not_claim_impossible_speeds() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 40000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// The OS hands a flush over as a dense burst of large packets: hundreds
	// of pixels each, stream cadence. Each packet's distance over its own
	// gap names tens of thousands of pixels per second — a speed no finger
	// reached — so the stream believes the hand no faster than it chases.
	scroll.coast_wheel_by(120.0, start + Duration::from_millis(20));
	let mut frame = start + Duration::from_millis(24);
	let mut last = 0.0;
	for step in 1..=12 {
		scroll.coast_wheel_by(
			300.0,
			start + Duration::from_millis(20 + step * 8),
		);
		for tick in 0..1 {
			frame = start + Duration::from_millis(24 + step * 8 + tick * 8);
			scroll.advance(frame, bounds);
			assert!(
				scroll.offset - last <= 8000.0 * 0.0085,
				"the burst carried the page {} px in one frame",
				scroll.offset - last
			);
			last = scroll.offset;
		}
	}
	// The burst's whole distance still arrives, as a bounded glide.
	for _ in 0..600 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(!scroll.animating());
	assert!(
		(scroll.offset - 3720.0).abs() < 1.0,
		"the burst distance should arrive, at {}",
		scroll.offset
	);
	assert_eq!(scroll.target, None);
}

#[test]
fn repeated_flicks_chain_without_stopping() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 20000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// Same-direction flicks, each a short burst, a third of a second apart:
	// the gaps inside the series are batch gaps, not gesture boundaries. The
	// stream must run through them all — decaying in each gap, continuing
	// with the next burst — without a dead stop in between.
	let mut at = start;
	for _ in 0..3 {
		at += Duration::from_millis(350);
		for step in 0..3 {
			scroll.coast_wheel_by(120.0, at + Duration::from_millis(step * 8));
		}
		let mut frame = at;
		for _ in 0..44 {
			frame += Duration::from_millis(8);
			assert!(
				scroll.advance(frame, bounds),
				"the stream died between flicks at {frame:?}"
			);
		}
	}
	let chained = scroll.offset;
	// After the last flick the page settles. Each burst's accounting started
	// from where the page was, and past the last burst the page carries only
	// its lead and quiet tail — well under another flick's worth of travel.
	let mut frame = at;
	for _ in 0..300 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(!scroll.animating());
	assert!(
		scroll.offset > chained - 0.5 && scroll.offset < chained + 400.0,
		"the page rests within the flicks' reach, at {}",
		scroll.offset
	);
	assert!(scroll.offset >= chained - 0.5);
	assert_eq!(scroll.target, None);
}

#[test]
fn a_large_flush_lands_as_a_glide_not_a_jump() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 20000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// One flush packet carries detents of accumulated motion; the first of a
	// gesture is pure distance, chased without any speed of its own.
	scroll.coast_wheel_by(5000.0, start + Duration::from_millis(16));
	let mut frame = start + Duration::from_millis(16);
	let mut last = 0.0;
	for _ in 0..10 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
		// The chase is capped, so no frame may teleport the page.
		assert!(
			scroll.offset - last <= 8000.0 * 0.0085,
			"the page jumped {} px in one frame",
			scroll.offset - last
		);
		last = scroll.offset;
	}
	// And the whole distance still arrives.
	for _ in 0..400 {
		frame += Duration::from_millis(8);
		if !scroll.advance(frame, bounds) {
			break;
		}
	}
	assert!(!scroll.animating());
	assert!((scroll.offset - 5000.0).abs() < 1.0);
	assert_eq!(scroll.target, None);
}

#[test]
fn a_whole_detent_inside_a_live_stream_stays_adopted() {
	let start = Instant::now();
	// The OS rounds a fractional stream's odd packets to whole detents; the
	// stream adopts them while it is alive, and a stale one does not.
	let mut scroll = ScrollState::default();
	let at = start + Duration::from_millis(16);
	scroll.coast_wheel_by(80.0, at);
	assert!(scroll.wheel_stream_alive(at + Duration::from_millis(400)));
	assert!(!scroll.wheel_stream_alive(at + Duration::from_millis(600)));
}

#[test]
fn wheel_packet_after_a_pause_answers_a_reversal() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	let mut scroll = ScrollState::default();
	// A downward flick hands its inertia over in packets, and the page
	// rides on after the last one.
	let mut at = start;
	for _ in 0..4 {
		at += Duration::from_millis(40);
		scroll.coast_wheel_by(120.0, at);
		for step in 1..=4 {
			scroll.advance(at + Duration::from_millis(step * 8), bounds);
		}
	}
	let ridden = scroll.offset;
	// After the stream has gone quiet the hand reverses. The speed the old
	// gesture left behind is spent, so the page answers the upward packet
	// instead of carrying on downward past it.
	at += Duration::from_millis(200);
	scroll.coast_wheel_by(-300.0, at);
	let destination = (ridden - 300.0).max(0.0);
	assert_eq!(scroll.target, Some(destination));
	let mut frame = at;
	for _ in 0..400 {
		frame += Duration::from_millis(8);
		scroll.advance(frame, bounds);
	}
	assert!(
		scroll.offset < ridden,
		"the page should have turned, at {ridden}"
	);
	assert!((scroll.offset - destination).abs() < 1.0);
	assert_eq!(scroll.target, None);
}

#[test]
fn direct_and_eased_steps_take_over_from_a_wheel_stream() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	for eased in [false, true] {
		let mut scroll = ScrollState::default();
		scroll.coast_wheel_by(120.0, start);
		scroll.coast_wheel_by(120.0, start + Duration::from_millis(40));
		scroll.advance(start + Duration::from_millis(48), bounds);
		let displayed = scroll.offset;
		assert!(scroll.animating());
		if eased {
			scroll.animate_by(10.0, start + Duration::from_millis(48));
			scroll.advance(start + Duration::from_secs(1), bounds);
		} else {
			scroll.by(10.0, bounds);
		}
		assert_eq!(scroll.offset, displayed + 10.0);
		assert!(!scroll.animating());
		assert_eq!(scroll.target, None);
	}
}

#[test]
fn cancelling_a_wheel_stream_forgets_its_target_when_layout_grows() {
	let start = Instant::now();
	let prefix = ScrollBounds {
		max: 100.0,
		complete: false,
	};
	let mut scroll = ScrollState::default();
	scroll.coast_wheel_by(120.0, start);
	scroll.coast_wheel_by(120.0, start + Duration::from_millis(40));
	scroll.advance(start + Duration::from_millis(48), prefix);
	let displayed = scroll.offset;
	scroll.cancel();
	assert!(!scroll.animating());
	assert_eq!(scroll.target, None);
	assert_eq!(scroll.deadline(start + Duration::from_millis(48)), None);
	scroll.resolve(ScrollBounds {
		max: 4000.0,
		complete: true,
	});
	assert_eq!(scroll.offset, displayed);
	// A direct request beyond an incomplete prefix still survives cancellation.
	scroll.set(900.0, prefix);
	scroll.cancel();
	assert_eq!(scroll.target, Some(900.0));
}

#[test]
fn a_wheel_packet_after_a_pause_follows_the_new_input() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	for delta in [-10.0, 10.0] {
		let mut scroll = ScrollState::default();
		for frame in 0..=45 {
			let at = start + Duration::from_millis(frame * 8);
			if frame <= 15 && frame % 5 == 0 {
				scroll.coast_wheel_by(120.0, at);
			}
			scroll.advance(at, bounds);
		}
		let displayed = scroll.offset;
		// The quiet decay still leaves the stream alive this soon; the
		// paused arrival clamps its carried speed to the silence's own
		// average, and the page follows the fresh input from where it is.
		assert!(displayed > 400.0 && scroll.animating());
		scroll.coast_wheel_by(delta, start + Duration::from_millis(368));
		assert_eq!(scroll.target, Some(displayed + delta));
		for frame in 47..=300 {
			let before = scroll.offset;
			scroll.advance(start + Duration::from_millis(frame * 8), bounds);
			assert!((scroll.offset - before) * delta >= 0.0);
		}
		// The bridged speed's lead and quiet tail carry the page a few
		// pixels past the fresh target before it stills.
		assert!((scroll.offset - displayed - delta).abs() < 12.0);
		assert!(!scroll.animating());
	}
}

#[test]
fn a_wheel_packet_after_a_frame_stall_starts_at_its_arrival() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 20000.0,
		complete: true,
	};
	for direction in [-1.0, 1.0] {
		for packets in [1, 4] {
			let mut scroll = ScrollState::default();
			scroll.set(10000.0, bounds);
			for frame in 0..=(packets - 1) * 5 + 1 {
				let at = start + Duration::from_millis(frame * 8);
				if frame % 5 == 0 {
					scroll.coast_wheel_by(120.0 * direction, at);
				}
				scroll.advance(at, bounds);
			}
			let displayed = scroll.offset;
			// No frames run during the pause. New travel starts when its
			// packet arrives, even if the old stream still carries speed.
			let arrival =
				start + Duration::from_millis((packets - 1) * 40 + 240);
			scroll.coast_wheel_by(300.0 * direction, arrival);
			assert_eq!(scroll.target, Some(displayed + 300.0 * direction));
			scroll.advance(arrival, bounds);
			assert_eq!(scroll.offset, displayed);
			scroll.advance(arrival + Duration::from_millis(8), bounds);
			let travel = (scroll.offset - displayed) * direction;
			assert!(
				travel > 0.0 && travel <= 8000.0 * 0.0085,
				"travel: {travel}"
			);
			for frame in 2..=400 {
				scroll.advance(
					arrival + Duration::from_millis(frame * 8),
					bounds,
				);
			}
			assert!(!scroll.animating());
			assert!(
				(scroll.offset - displayed - 300.0 * direction).abs() < 1.0
			);
		}
	}
}

#[test]
fn a_small_wheel_reversal_discards_unpaid_travel() {
	let start = Instant::now();
	let bounds = ScrollBounds {
		max: 4000.0,
		complete: true,
	};
	for direction in [-1.0, 1.0] {
		// Reverse both before a stream has velocity and while it is moving.
		for packets in [1, 2] {
			let mut scroll = ScrollState::default();
			scroll.set(1000.0, bounds);
			for packet in 0..packets {
				let at = start + Duration::from_millis(packet * 40);
				scroll.coast_wheel_by(120.0 * direction, at);
				for frame in 1..=5 {
					scroll
						.advance(at + Duration::from_millis(frame * 8), bounds);
				}
			}
			let displayed = scroll.offset;
			let at = start + Duration::from_millis(packets * 40);
			let delta = -10.0 * direction;
			scroll.coast_wheel_by(delta, at);
			assert_eq!(scroll.target, Some(displayed + delta));
			scroll.advance(at + Duration::from_millis(8), bounds);
			assert!((scroll.offset - displayed) * delta > 0.0);
		}
	}
}
