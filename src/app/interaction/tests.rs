//! What a selection copies, what a press past an edge asks the loop to do, and
//! how an arriving update carries a selection into a new layout.
//!
//! `InteractionState` owns the selection machine and `state/tests.rs` drives
//! it directly. These tests cover the parts that only exist here: the reading
//! text a copy takes, the auto-scroll tick a drag in the margin arms, and the
//! order in which an update rebases a selection before it is accepted.
use super::*;
use crate::app::SendEvent;
use crate::app::gestures::Inertia;
use crate::app::window::Loop;
use crate::layout::{LayoutEngine, LayoutOptions, LayoutSnapshot};
use crate::state::{Drag, Grain, Point};
use crate::worker::{ReaderSnapshot, Update};
use markview_core::document::Document;
use markview_core::text::{Affinity, TextSelection};
use std::path::PathBuf;
use std::sync::Arc;
use winit::dpi::PhysicalPosition;
use winit::event::{
	DeviceId, ElementState, MouseButton, TouchPhase, WindowEvent,
};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

mod viewer;

const SOURCE: &str = "First paragraph.\n\nSecond paragraph.";

#[derive(Clone)]
struct StubProxy;
impl SendEvent for StubProxy {
	fn try_send(&self, _event: Event) -> bool {
		true
	}
}

#[test]
fn enabling_a_surviving_independent_window_registers_it_for_later_launches() {
	use crate::app::single_instance::{Start, start};
	use std::sync::mpsc;
	use std::time::Duration;

	#[derive(Clone)]
	struct Proxy(mpsc::Sender<Option<PathBuf>>);
	impl SendEvent for Proxy {
		fn try_send(&self, event: Event) -> bool {
			if let Event::Activate(path) = event {
				self.0.send(path).is_ok()
			} else {
				true
			}
		}
	}
	let dir = tempfile::tempdir().unwrap();
	let lock = dir.path().join("instance.lock");
	let Start::Primary(primary) = start(&lock, false, None).unwrap() else {
		panic!()
	};
	let (original_tx, original_rx) = mpsc::channel();
	let listener = primary.listen(move |event| {
		let Event::Activate(path) = event else {
			panic!()
		};
		original_tx.send(path).is_ok()
	});
	assert!(matches!(
		start(&lock, false, None).unwrap(),
		Start::Independent
	));
	let (tx, rx) = mpsc::channel();
	let mut app = App::new(
		crate::cli::LaunchOptions {
			mode: Mode::Smoke,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy(tx),
	);
	app.instance_path = Some(lock.clone());
	let started = Instant::now();
	app.action(Command::SingleInstance);
	assert!(started.elapsed() < Duration::from_secs(1));
	assert!(app.preferences.values.single_instance);
	assert!(app.instance.is_none());
	assert!(matches!(
		original_rx.try_recv(),
		Err(mpsc::TryRecvError::Empty)
	));
	app.action(Command::SingleInstance);
	drop(listener);
	app.action(Command::SingleInstance);
	assert!(app.preferences.values.single_instance);
	assert!(app.instance.is_some());
	let path = dir.path().join("second.md");
	assert!(matches!(
		start(&lock, true, Some(path.clone())).unwrap(),
		Start::Forwarded(_)
	));
	assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Some(path));
}

/// A loop that does nothing, for the timers and handlers that only ask
/// whether to stop.
struct StubLoop;
impl Loop for StubLoop {
	fn exit(&self) {}
}

fn position(block: usize, offset: usize) -> TextPosition {
	TextPosition {
		revision: 1,
		block,
		node: 0,
		offset,
		affinity: Affinity::Before,
	}
}

fn selection(anchor: TextPosition, focus: TextPosition) -> TextSelection {
	TextSelection { anchor, focus }
}

/// A settled reader: `source` is the accepted document at revision 1, laid out
/// at `width`, and the reader is showing that layout. The document is returned
/// so a test can lay it out again at another width.
fn reader(source: &str, width: f32) -> (App<StubProxy>, Arc<Document>) {
	let mut app = App::new(
		crate::cli::LaunchOptions {
			mode: Mode::Smoke,
			options: crate::test_support::options(),
			..Default::default()
		},
		StubProxy,
	);
	app.ui = crate::test_support::shaper();
	app.preferences.values = crate::settings::ReaderSettings::default();
	let document = Arc::new(crate::document::parse(source));
	let options = LayoutOptions {
		width,
		..app.options()
	};
	let snapshot = LayoutEngine::new().layout(&document, &options);
	let session = &mut app.readers.session;
	session.document = Some(document.clone());
	// Equal ids mean equal reading positions, which is what lets an update
	// rebase a selection instead of clearing it.
	session.accepted_content_id = document.content_id;
	session.snapshot = snapshot;
	session.content_version = 1;
	session.accepted_revision = 1;
	session.parse_complete = true;
	session.snapshot_complete = true;
	session.requested_options = Some(options);
	(app, document)
}

/// An update the reader will accept: same path, same version, complete.
fn update(
	path: PathBuf,
	document: Arc<Document>,
	layout: LayoutSnapshot,
	content_version: u64,
) -> Event {
	Event::Ready(Box::new(Update {
		version: 1,
		path,
		result: Some(Ok(ReaderSnapshot {
			document,
			layout,
			content_version,
			complete: true,
			parse_complete: true,
			remote_deferred: 0,
		})),
		requested: Instant::now(),
		read_ms: 0.0,
		parse_ms: 0.0,
		layout_ms: 0.0,
		counts: None,
	}))
}

#[test]
fn a_copy_takes_the_reading_text_and_a_stale_selection_takes_nothing() {
	let (mut app, _) = reader(SOURCE, 760.0);
	assert_eq!(app.selected_text(), None, "nothing is selected yet");

	app.interaction.selection =
		Some(selection(position(0, 0), position(0, 16)));
	assert_eq!(app.selected_text().as_deref(), Some("First paragraph."));

	// An empty selection names a position but covers no character.
	app.interaction.selection = Some(selection(position(0, 5), position(0, 5)));
	assert_eq!(app.selected_text(), None);

	// The layout rejects a selection tagged with a revision it never produced,
	// so a stale one copies nothing rather than the wrong reading text.
	app.interaction.selection = Some(selection(
		TextPosition {
			revision: 2,
			..position(0, 0)
		},
		TextPosition {
			revision: 2,
			..position(0, 16)
		},
	));
	assert_eq!(app.selected_text(), None);

	// A selection that spans blocks joins them with the separator.
	let whole = app.readers.session.snapshot.select_all(1).unwrap();
	app.interaction.selection = Some(whole);
	assert_eq!(
		app.selected_text().as_deref(),
		Some(SOURCE),
		"the whole document, paragraphs joined by the separator"
	);
}

#[test]
fn dragging_into_the_margin_asks_for_a_tick_and_a_tick_scrolls_one_step() {
	let (mut app, _) = reader(&"Body line.\n\n".repeat(200), 760.0);
	// Scrolled away from the top, so dragging above the text has somewhere to
	// travel.
	app.readers.session.scrolling.offset = 100.0;
	app.interaction.selection = Some(selection(position(0, 0), position(0, 4)));
	app.interaction.pointer_down = Some(Drag {
		start: Point::new(100.0, 100.0),
		link: None,
		grain: Grain::Char,
		base: None,
	});
	app.interaction.dragged = true;
	app.interaction.cursor = (100.0, TOP + 4.0);

	app.update_drag();
	assert!(
		app.interaction.drag_at.is_some(),
		"a drag inside the top margin must arm the auto-scroll tick"
	);

	let scroll = app.readers.session.scrolling.offset;
	app.tick(&StubLoop, Instant::now() + Duration::from_millis(32));
	assert_eq!(
		app.readers.session.scrolling.offset,
		scroll - 14.0,
		"one tick travels one step towards the pointer's edge"
	);
	assert!(
		app.interaction.drag_at.is_some(),
		"the tick re-arms while the pointer stays in the margin"
	);

	// Out of the margin and short of the text, nothing more is scheduled.
	app.interaction.cursor = (100.0, 400.0);
	app.update_drag();
	assert!(app.interaction.drag_at.is_none());
}

/// The window point at the centre of the first cluster of `block`, as the
/// renderer maps document coordinates onto the window.
fn point_over(app: &App<StubProxy>, block: usize) -> (f32, f32) {
	let geometry = app.view_geometry();
	let placed = &app.readers.session.snapshot.blocks[block];
	let cluster = &placed.layout.text[0].clusters[0];
	(
		geometry.left + cluster.rect.x + cluster.rect.w * 0.5,
		geometry.top + placed.y + cluster.rect.y + cluster.rect.h * 0.5
			- geometry.scroll,
	)
}

/// One physical drag: move there, press, move elsewhere, release.
fn physical_drag(app: &mut App<StubProxy>, from: (f32, f32), to: (f32, f32)) {
	// A tap holds mouse input back for a moment; this drives the mouse path.
	app.gestures.allow_mouse();
	let moved = |at: (f32, f32)| WindowEvent::CursorMoved {
		device_id: DeviceId::dummy(),
		position: PhysicalPosition::new(f64::from(at.0), f64::from(at.1)),
	};
	let button = |state| WindowEvent::MouseInput {
		device_id: DeviceId::dummy(),
		button: MouseButton::Left,
		state,
	};
	for event in [
		moved(from),
		button(ElementState::Pressed),
		moved(to),
		button(ElementState::Released),
	] {
		app.handle_window_event(&StubLoop, WindowId::dummy(), event);
	}
}

/// One physical click at a point: move there, press, release in place.
fn physical_click(app: &mut App<StubProxy>, at: (f32, f32)) {
	physical_drag(app, at, at);
}

#[test]
fn a_press_and_a_drag_over_text_select_it() {
	let (mut app, _) = reader(
		"One line here.\n\nTwo line here.\n\nThree line here.",
		760.0,
	);
	let from = point_over(&app, 0);
	// Past the first letter of the last paragraph, so the drag covers it.
	let (x, y) = point_over(&app, 2);
	let to = (x + 40.0, y);
	physical_drag(&mut app, from, to);
	let selection =
		app.interaction.selection.expect("a drag over text selects");
	assert!(!selection.is_empty(), "{selection:?}");
	let text = app.selected_text().expect("the drag copies reading text");
	assert!(text.starts_with("One line here."), "{text:?}");
	assert!(text.contains("Two line here."), "{text:?}");
	assert!(text.contains("Three"), "{text:?}");
}

/// The same drag over every kind of content a block can hold, so a change to
/// how a cluster is split cannot pass by only ever seeing plain paragraphs.
#[test]
fn a_drag_selects_across_code_tables_math_and_ligatures() {
	let source = "office and file\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n|---|---|\n| one | two |\n\n$x^2$ and 中文\n\nLast paragraph.";
	let (mut app, _) = reader(source, 760.0);
	let blocks = app.readers.session.snapshot.blocks.len();
	assert!(blocks >= 5, "the source laid out as {blocks} blocks");
	let from = point_over(&app, 0);
	let (x, y) = point_over(&app, blocks - 1);
	let to = (x + 60.0, y);
	physical_drag(&mut app, from, to);
	let text = app.selected_text().expect("the drag selects reading text");
	assert!(text.contains("office"), "{text:?}");
	assert!(text.contains("let x = 1;"), "{text:?}");
	assert!(text.contains("one"), "{text:?}");
	assert!(text.contains("中文"), "{text:?}");
}

#[test]
fn a_double_click_selects_the_word_and_a_triple_click_the_paragraph() {
	let (mut app, _) = reader("First one here.\n\nSecond one here.", 760.0);
	let at = point_over(&app, 0);
	physical_click(&mut app, at);
	physical_click(&mut app, at);
	let word = app
		.selected_text()
		.expect("the double click selects a word");
	assert!(
		word.split_whitespace().count() == 1,
		"a word, not a paragraph: {word:?}"
	);
	physical_click(&mut app, at);
	let paragraph = app
		.selected_text()
		.expect("the triple click selects a block");
	assert!(
		paragraph.contains("First one here."),
		"the whole block: {paragraph:?}"
	);
}

#[test]
fn a_shift_click_extends_from_where_the_press_landed() {
	let (mut app, _) = reader("Alpha beta.\n\nGamma delta.", 760.0);
	let at = point_over(&app, 0);
	physical_click(&mut app, at);
	let split = app.readers.session.snapshot.select_all(1).unwrap();
	app.interaction.selection = Some(split);
	app.interaction.modifiers = ModifiersState::SHIFT;
	let next = point_over(&app, 1);
	physical_click(&mut app, next);
	let selection = app.interaction.selection.expect("shift extends");
	assert_eq!(
		selection.anchor.block, 0,
		"the earlier press keeps the anchor"
	);
	assert_eq!(selection.focus.block, 1, "the shift click moves the focus");
}

#[test]
fn scrolling_moves_what_a_held_press_covers() {
	let (mut app, _) = reader(&"Body line.\n\n".repeat(200), 760.0);
	app.interaction.cursor = (100.0, 200.0);
	let pressed = app.text_at_cursor().expect("text under the pointer");
	app.interaction.begin_selection(pressed, None);
	// Far enough that the press counts as a drag rather than a click.
	app.interaction.cursor = (160.0, 260.0);
	app.update_drag();
	let held = app.interaction.selection.expect("a drag selects");
	assert_eq!(held.anchor, pressed, "the anchor is where the press landed");

	// The wheel moves the text under a pointer that has not moved, so no
	// pointer event arrives to say the focus now covers something else.
	app.scroll_by(100.0);
	let covered = app.text_at_cursor().expect("text under the pointer");
	assert_ne!(covered, held.focus, "the scroll did move the text under it");
	let after = app.interaction.selection.unwrap();
	assert_eq!(
		after.focus, covered,
		"the focus follows the text now under the pointer"
	);
	assert_eq!(
		after.anchor, held.anchor,
		"the anchor stays where the press landed"
	);
}

#[test]
fn an_update_carries_the_selection_into_the_new_layout() {
	let (mut app, document) = reader(SOURCE, 760.0);
	let path = PathBuf::from("/tmp/markview-selection-test.md");
	app.readers.session.path = Some(path.clone());
	app.readers.session.version = 1;
	app.interaction.selection = app.readers.session.snapshot.select_all(1);
	let before = app.selected_text().expect("the selection copies");

	// The same reading text laid out at another width moves every block on
	// the page without changing a word of it, so the selection must come
	// across rather than being dropped. The revision moves too, which is what
	// makes the order matter: a selection is rebased against the snapshot that
	// produced it, before the new one is accepted, or it no longer names a
	// revision the layout recognises and is silently dropped.
	let options = LayoutOptions {
		width: 520.0,
		..app.options()
	};
	let layout = LayoutEngine::new().layout(&document, &options);
	app.handle_user_event(&StubLoop, update(path, document, layout, 2));

	assert_eq!(
		app.readers.session.accepted_revision, 2,
		"the update's revision is the one a selection now names"
	);
	assert_eq!(
		app.selected_text().as_deref(),
		Some(before.as_str()),
		"a re-layout of the same reading text keeps the selection"
	);
}

#[test]
fn a_deferred_select_all_resolves_once_the_layout_is_complete() {
	let (mut app, document) = reader(SOURCE, 760.0);
	let path = PathBuf::from("/tmp/markview-select-all-test.md");
	app.readers.session.path = Some(path.clone());
	app.readers.session.version = 1;
	// What `Ctrl+A` leaves behind when the layout is still running: nothing
	// selected yet, and the request waiting for a complete snapshot.
	app.readers.session.layout_pending = true;
	app.readers.session.select_all_pending = true;
	assert_eq!(app.interaction.selection, None);

	let options = LayoutOptions {
		width: 760.0,
		..app.options()
	};
	let layout = LayoutEngine::new().layout(&document, &options);
	app.handle_user_event(&StubLoop, update(path, document, layout, 1));

	assert!(
		!app.readers.session.select_all_pending,
		"the deferred request is consumed"
	);
	let expected = app
		.readers
		.session
		.snapshot
		.extract_text(app.readers.session.snapshot.select_all(1).unwrap(), 1);
	assert_eq!(
		app.selected_text().as_deref(),
		Some(expected.as_str()),
		"the completed layout selects the whole document"
	);
}

#[test]
fn a_fractional_line_wheel_coasts_on_windows_and_eases_elsewhere() {
	let (mut app, _) = reader(&"A scrolling paragraph.\n\n".repeat(100), 760.0);
	app.interaction.cursor = point_over(&app, 0);
	let feed = |app: &mut App<StubProxy>, lines| {
		app.handle_window_event(
			&StubLoop,
			WindowId::dummy(),
			WindowEvent::MouseWheel {
				device_id: DeviceId::dummy(),
				delta: winit::event::MouseScrollDelta::LineDelta(0.0, lines),
				phase: winit::event::TouchPhase::Moved,
			},
		);
	};
	feed(&mut app, -1.0);
	let whole = app.readers.session.scrolling.target.unwrap();
	assert!(app.readers.session.scrolling.animation.is_some());
	app.readers.session.cancel_scroll_animation();
	app.interaction.wheel = Default::default();
	feed(&mut app, -0.5);
	assert_eq!(app.readers.session.scrolling.target, Some(whole * 0.5));
	if cfg!(windows) {
		// A fractional line count names a touchpad's packet stream, which
		// owns the offset and coasts under momentum instead of an eased step.
		assert!(app.readers.session.scrolling.animation.is_none());
		assert!(app.readers.session.scroll_animating());
	} else {
		assert!(app.readers.session.scrolling.animation.is_some());
	}
	app.readers.session.advance_scroll(
		Instant::now() + Duration::from_secs(1),
		app.viewport(),
	);
	assert_eq!(app.readers.session.scrolling.offset, whole * 0.5);
	assert!(!app.readers.session.scroll_animating());
}

/// Feeds the seam the way each desktop delivers pixel scrolling: macOS puts
/// its momentum in the deltas, the rest ask the reader to coast on release.
fn pixel_scroll(
	inertia: Inertia,
) -> impl FnMut(&mut App<StubProxy>, f32, TouchPhase) {
	move |app, dy, phase| app.trackpad_scroll(0.0, dy, phase, inertia)
}

#[test]
fn a_native_pixel_stream_follows_contact_and_stops_where_the_hand_stops() {
	let (mut app, _) = reader(&"A scrolling paragraph.\n\n".repeat(100), 760.0);
	app.interaction.cursor = point_over(&app, 0);
	let mut feed = pixel_scroll(Inertia::Native);
	feed(&mut app, -40.0, TouchPhase::Started);
	feed(&mut app, -60.0, TouchPhase::Moved);
	feed(&mut app, -20.0, TouchPhase::Moved);
	assert_eq!(
		app.readers.session.scrolling.offset, 120.0,
		"contact pans by exactly the deltas, with no easing and no lead"
	);
	feed(&mut app, -10.0, TouchPhase::Ended);
	assert_eq!(app.readers.session.scrolling.offset, 130.0);
	// The deltas were the whole motion: when the OS stops sending them, the
	// page rests where the last one put it and nothing of the reader's own
	// carries on.
	app.readers.session.advance_scroll(
		Instant::now() + Duration::from_secs(1),
		app.viewport(),
	);
	app.advance_gestures(Instant::now() + Duration::from_millis(16));
	assert_eq!(app.readers.session.scrolling.offset, 130.0);
	assert!(!app.readers.session.scroll_animating());
}

#[test]
fn a_native_pixel_stream_reverses_with_the_first_opposite_delta() {
	let (mut app, _) = reader(&"A scrolling paragraph.\n\n".repeat(100), 760.0);
	app.interaction.cursor = point_over(&app, 0);
	let mut feed = pixel_scroll(Inertia::Native);
	feed(&mut app, -50.0, TouchPhase::Started);
	feed(&mut app, -30.0, TouchPhase::Moved);
	assert_eq!(app.readers.session.scrolling.offset, 80.0);
	feed(&mut app, 12.0, TouchPhase::Moved);
	assert_eq!(app.readers.session.scrolling.offset, 68.0);
}

#[test]
fn a_native_stream_start_cancels_a_running_wheel_momentum() {
	let (mut app, _) = reader(&"A scrolling paragraph.\n\n".repeat(100), 760.0);
	// The packet path owns the offset and coasts under momentum of its own.
	app.scroll_wheel_packet(30.0);
	app.readers.session.advance_scroll(
		Instant::now() + Duration::from_millis(100),
		app.viewport(),
	);
	assert!(app.readers.session.scroll_animating());
	app.interaction.cursor = point_over(&app, 0);
	app.trackpad_scroll(0.0, -10.0, TouchPhase::Started, Inertia::Native);
	let resting = app.readers.session.scrolling.offset;
	assert!(resting > 0.0, "the momentum had already moved the page");
	assert!(!app.readers.session.scroll_animating());
	app.readers.session.advance_scroll(
		Instant::now() + Duration::from_secs(1),
		app.viewport(),
	);
	assert_eq!(app.readers.session.scrolling.offset, resting);
}

#[test]
fn a_pixel_stream_without_native_inertia_still_coasts_on_release() {
	let (mut app, _) = reader(&"A scrolling paragraph.\n\n".repeat(100), 760.0);
	app.interaction.cursor = point_over(&app, 0);
	let mut feed = pixel_scroll(Inertia::Synthesized);
	feed(&mut app, -40.0, TouchPhase::Started);
	for _ in 0..4 {
		feed(&mut app, -40.0, TouchPhase::Moved);
	}
	feed(&mut app, 0.0, TouchPhase::Ended);
	let released = app.readers.session.scrolling.offset;
	app.advance_gestures(Instant::now() + Duration::from_millis(16));
	assert!(
		app.readers.session.scrolling.offset > released,
		"an unmarked stream keeps the reader's own coast on release"
	);
}

#[test]
fn forwarded_files_open_tabs_and_reuse_existing_tabs() {
	let (mut app, _) = reader(SOURCE, 400.0);
	let first = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/../fixtures/ordinary-10k.md");
	let second = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/code-10k.md");
	app.handle_user_event(&StubLoop, Event::Activate(Some(first.clone())));
	app.handle_user_event(&StubLoop, Event::Activate(Some(second.clone())));
	let first = first.canonicalize().unwrap();
	let second = second.canonicalize().unwrap();
	assert_eq!(app.readers.session.path.as_ref(), Some(&second));
	let count = app.readers.entries().len();
	assert_eq!(count, 2);
	app.handle_user_event(&StubLoop, Event::Activate(Some(first.clone())));
	assert_eq!(app.readers.entries().len(), count);
	assert_eq!(app.readers.session.path.as_ref(), Some(&first));
}
