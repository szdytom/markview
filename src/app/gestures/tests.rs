use super::recognizer::Contact;
use super::*;
use winit::event::DeviceId;

fn contact(id: u64) -> Contact {
	(DeviceId::dummy(), id)
}

#[test]
fn a_tap_tolerates_jitter_but_a_drag_never_activates_on_release() {
	let mut input = Recognizer::default();
	input.begin(contact(1), (50.0, 50.0), "link");
	assert_eq!(
		input.update(contact(1), TouchPhase::Moved, (53.0, 52.0)),
		None
	);
	assert_eq!(
		input.update(contact(1), TouchPhase::Ended, (52.0, 51.0)),
		Some(Gesture::Tap("link"))
	);
	input.begin(contact(2), (50.0, 50.0), "link");
	assert_eq!(
		input.update(contact(2), TouchPhase::Moved, (52.0, 30.0)),
		Some(Gesture::Pan("link", (0.0, -20.0)))
	);
	assert_eq!(
		input.update(contact(2), TouchPhase::Moved, (80.0, 25.0)),
		Some(Gesture::Pan("link", (0.0, -5.0)))
	);
	assert_eq!(
		input.update(contact(2), TouchPhase::Ended, (50.0, 50.0)),
		Some(Gesture::Pan("link", (0.0, 25.0)))
	);
}

#[test]
fn cancellation_and_additional_fingers_cannot_turn_into_taps() {
	let mut input = Recognizer::default();
	input.begin(contact(1), (10.0, 10.0), "close tab");
	assert_eq!(
		input.update(contact(1), TouchPhase::Cancelled, (10.0, 10.0)),
		None
	);
	assert_eq!(
		input.update(contact(1), TouchPhase::Ended, (10.0, 10.0)),
		None
	);
	input.begin(contact(1), (10.0, 10.0), "close tab");
	input.begin(contact(2), (10.0, 30.0), "link");
	assert_eq!(
		input.update(contact(2), TouchPhase::Ended, (10.0, 30.0)),
		None
	);
	assert_eq!(
		input.update(contact(1), TouchPhase::Ended, (10.0, 10.0)),
		None
	);
	input.begin(contact(3), (10.0, 10.0), "new tap");
	assert_eq!(
		input.update(contact(99), TouchPhase::Cancelled, (0.0, 0.0)),
		None
	);
	assert_eq!(
		input.update(contact(3), TouchPhase::Ended, (10.0, 10.0)),
		Some(Gesture::Tap("new tap"))
	);
}

#[test]
fn horizontal_drag_keeps_its_captured_surface_and_includes_release_motion() {
	let mut input = Recognizer::default();
	input.begin(contact(1), (100.0, 100.0), "wide table");
	assert_eq!(
		input.update(contact(1), TouchPhase::Moved, (70.0, 102.0)),
		Some(Gesture::Pan("wide table", (-30.0, 0.0)))
	);
	assert_eq!(
		input.update(contact(1), TouchPhase::Ended, (60.0, 130.0)),
		Some(Gesture::Pan("wide table", (-10.0, 0.0)))
	);
	assert!(input.contacts.is_empty());
	assert!(input.drag.is_none());
}

#[test]
fn additional_contacts_cancel_the_tap_and_drag() {
	let mut input = Recognizer::default();
	input.begin(contact(1), (10.0, 10.0), "first");
	input.begin(contact(2), (10.0, 110.0), "second");
	assert_eq!(
		input.update(contact(2), TouchPhase::Moved, (10.0, 160.0)),
		None
	);
	assert_eq!(
		input.update(contact(2), TouchPhase::Ended, (10.0, 160.0)),
		None
	);
	assert_eq!(
		input.update(contact(1), TouchPhase::Moved, (10.0, 90.0)),
		None
	);
	assert_eq!(
		input.update(contact(1), TouchPhase::Ended, (10.0, 90.0)),
		None
	);
}

#[test]
fn coast_decays_stops_after_a_pause_and_is_independent_of_frame_rate() {
	let start = Instant::now();
	let mut motion = Motion::new(start);
	motion.sample((0.0, -30.0), start + Duration::from_millis(20));
	assert!(motion.release(start + Duration::from_millis(25)));
	let first = motion.advance(start + Duration::from_millis(41)).unwrap();
	let second = motion.advance(start + Duration::from_millis(57)).unwrap();
	assert!(first.1 < second.1 && second.1 < 0.0);
	let mut combined = Motion::new(start);
	combined.sample((0.0, -30.0), start + Duration::from_millis(20));
	assert!(combined.release(start + Duration::from_millis(25)));
	let whole = combined.advance(start + Duration::from_millis(57)).unwrap();
	assert!((first.1 + second.1 - whole.1).abs() < 0.001);
	assert!(motion.advance(start + Duration::from_secs(1)).is_none());
	let mut held = Motion::new(start);
	held.sample((0.0, -30.0), start + Duration::from_millis(20));
	assert!(!held.release(start + Duration::from_millis(150)));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an X11 display; exercises the real application input adapter"]
fn native_touch_and_trackpad_route_through_reader_and_chrome() {
	use winit::{
		dpi::PhysicalPosition, event_loop::EventLoop,
		platform::x11::EventLoopBuilderExtX11,
	};
	let event_loop = EventLoop::<super::super::Event>::with_user_event()
		.with_x11()
		.with_any_thread(true)
		.build()
		.unwrap();
	let mut app = App::new(
		crate::cli::LaunchOptions {
			offline: true,
			mode: crate::cli::Mode::Smoke,
			..Default::default()
		},
		event_loop.create_proxy(),
	);
	app.readers.session.snapshot.height = 5000.0;
	app.readers.session.snapshot.width = 800.0;
	app.readers.session.snapshot_complete = true;
	let touch = |id, phase, x, y| Touch {
		device_id: DeviceId::dummy(),
		id,
		phase,
		location: PhysicalPosition::new(x, y),
		force: None,
	};
	app.handle_touch(touch(1, TouchPhase::Started, 600.0, 400.0));
	app.handle_touch(touch(1, TouchPhase::Moved, 600.0, 300.0));
	assert_eq!(app.readers.session.scroll, 100.0);
	assert!(app.interaction.selection.is_none());
	app.handle_touch(touch(1, TouchPhase::Ended, 600.0, 300.0));
	app.cancel_gestures();
	app.interaction.cursor = (600.0, 300.0);
	app.trackpad_scroll(0.0, -50.0, TouchPhase::Started);
	assert_eq!(app.readers.session.scroll, 150.0);
	app.gestures.motion.as_mut().unwrap().1 =
		Motion::new(Instant::now() - Duration::from_millis(20));
	app.interaction.cursor = (10.0, 10.0);
	app.trackpad_scroll(0.0, -30.0, TouchPhase::Moved);
	assert_eq!(
		app.readers.session.scroll, 180.0,
		"the initial surface owns the whole trackpad gesture"
	);
	app.trackpad_scroll(0.0, 0.0, TouchPhase::Ended);
	assert!(app.gestures.coasting);
	app.advance_gestures(Instant::now() + Duration::from_millis(16));
	assert!(app.readers.session.scroll > 180.0);
	app.trackpad_scroll(0.0, 0.0, TouchPhase::Cancelled);
	assert!(!app.gestures.coasting);
	let size = app.preferences.values.font_size;
	app.handle_touch(touch(1, TouchPhase::Started, 500.0, 300.0));
	app.handle_touch(touch(2, TouchPhase::Started, 600.0, 300.0));
	app.handle_touch(touch(2, TouchPhase::Moved, 650.0, 300.0));
	assert_eq!(app.preferences.values.font_size, size);
	app.handle_touch(touch(2, TouchPhase::Ended, 650.0, 300.0));
	app.handle_touch(touch(1, TouchPhase::Ended, 500.0, 300.0));
	let settings = app
		.buttons()
		.into_iter()
		.find(|button| button.action == Command::Settings)
		.unwrap()
		.rect;
	let (x, y) = (
		f64::from(settings.x + settings.w / 2.0),
		f64::from(settings.y + settings.h / 2.0),
	);
	app.handle_touch(touch(1, TouchPhase::Started, x, y));
	assert!(!app.interaction.panel_open());
	app.handle_touch(touch(1, TouchPhase::Cancelled, x, y));
	assert!(!app.interaction.panel_open());
	app.handle_touch(touch(2, TouchPhase::Started, x, y));
	app.handle_touch(touch(2, TouchPhase::Ended, x, y));
	assert!(app.interaction.panel_open());
	let scroll = app.readers.session.scroll;
	app.handle_touch(touch(3, TouchPhase::Started, 600.0, 400.0));
	app.handle_touch(touch(3, TouchPhase::Moved, 600.0, 200.0));
	app.handle_touch(touch(3, TouchPhase::Ended, 600.0, 200.0));
	assert_eq!(
		app.readers.session.scroll, scroll,
		"panel scrolling must not move the document"
	);
	app.cancel_gestures();
	assert!(!app.gestures.coasting);
	assert!(app.gestures.gesture.contacts.is_empty());
}
