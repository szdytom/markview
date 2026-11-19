use super::*;
use markview_layout::scene::LinkTarget;
use markview_platform::event::{
	DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase,
	WindowEvent,
};

struct StubLoop;
impl crate::app::window::Loop for StubLoop {
	fn exit(&self) {
		panic!("the window must stay open");
	}
}

fn window_event(app: &mut App<Proxy>, event: WindowEvent) {
	app.handle_window_event(
		&StubLoop,
		winit::window::WindowId::from_raw(0),
		event,
	);
}

fn mouse_click(app: &mut App<Proxy>, button: MouseButton) {
	for state in [ElementState::Pressed, ElementState::Released] {
		mouse_button(app, button, state);
	}
}

fn mouse_button(
	app: &mut App<Proxy>,
	button: MouseButton,
	state: ElementState,
) {
	window_event(
		app,
		WindowEvent::MouseInput {
			device_id: DeviceId::from_raw(0),
			state,
			button,
		},
	);
}

fn touch(app: &mut App<Proxy>, id: u64, phase: TouchPhase, (x, y): (f32, f32)) {
	app.handle_touch(Touch {
		device_id: DeviceId::from_raw(0),
		id,
		phase,
		location: (f64::from(x), f64::from(y)).into(),
		force: None,
	});
}

fn center(rect: Rect) -> (f32, f32) {
	(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

fn link_point(
	app: &App<Proxy>,
	matches: impl Fn(&LinkTarget) -> bool,
) -> (f32, f32) {
	app.readers
		.session
		.display
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().find_map(|(link, mut rect)| {
				matches(link.target).then(|| {
					rect.y += block.y;
					center(app.view_geometry().window_rect(rect))
				})
			})
		})
		.unwrap()
}

fn hold(app: &mut App<Proxy>, id: u64, at: (f32, f32)) -> Instant {
	touch(app, id, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().expect("eligible hold");
	app.preview_tick(due);
	touch(app, id, TouchPhase::Ended, at);
	due
}

fn link_to(app: &App<Proxy>, target: &str) -> (f32, f32) {
	link_point(
		app,
		|link| matches!(link, LinkTarget::Url(url) if url.as_ref() == target),
	)
}

#[test]
fn mouse_leave_and_outside_click_keep_a_touch_preview_pinned() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	std::fs::write(
		&source,
		format!(
			"[Preview](target.md)\n\n{}",
			"Ordinary paragraph.\n\n".repeat(7)
		),
	)
	.unwrap();
	for event in [None, Some(MouseButton::Left), Some(MouseButton::Middle)] {
		let (mut app, events) = app();
		load(&mut app, &events, source.clone());
		let at = link_to(&app, "target.md");
		let due = hold(&mut app, 1, at);
		settle_preview(&mut app, due, |_| true);
		let popup = app.preview.floating().unwrap().rect;
		let (width, height, _) = app.dimensions();
		let outside = if event == Some(MouseButton::Left) {
			let block =
				app.readers.session.display.snapshot.blocks.last().unwrap();
			let (_, _, mut rect) = block.clusters().next().unwrap();
			rect.y += block.y;
			center(app.view_geometry().window_rect(rect))
		} else {
			(width * 0.8, height * 0.75)
		};
		assert!(!popup.contains(outside.0, outside.1));
		app.gestures.allow_mouse();
		window_event(
			&mut app,
			WindowEvent::CursorMoved {
				device_id: DeviceId::from_raw(0),
				position: (f64::from(outside.0), f64::from(outside.1)).into(),
			},
		);
		assert!(app.preview.floating().is_some());
		if let Some(button) = event {
			mouse_button(&mut app, button, ElementState::Pressed);
			if button == MouseButton::Left {
				assert!(app.interaction.pointer.drag().is_some());
				app.preview_tick(due + SHOW_DELAY);
				assert!(
					app.preview.floating().is_some(),
					"the preview closed during a click on ordinary text"
				);
			}
			mouse_button(&mut app, button, ElementState::Released);
		} else {
			window_event(
				&mut app,
				WindowEvent::CursorLeft {
					device_id: DeviceId::from_raw(0),
				},
			);
		}
		app.preview_tick(due + SHOW_DELAY);
		assert!(
			app.preview.floating().is_some(),
			"{event:?} closed the touch preview"
		);
	}
}

#[test]
fn touch_cancels_a_mouse_hover_without_reusing_its_deadline() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "target.md");
	let at = link_to(&app, "target.md");
	let now = links::hover(&mut app, "target.md");
	app.preview_tick(now + PREPARE_DELAY);
	assert!(app.preview.pending.is_some());
	let completion = app.preview.worker.wait(Duration::from_secs(10)).unwrap();
	touch(&mut app, 1, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().unwrap();
	app.preview.accept(completion);
	assert!(now + SHOW_DELAY < due);
	app.preview_tick(now + SHOW_DELAY);
	assert!(app.preview.floating().is_none());
	app.preview_tick(due);
	settle_preview(&mut app, due, |_| true);
	assert!(app.preview.touch.is_some());
	touch(&mut app, 1, TouchPhase::Ended, at);
}

#[test]
fn touch_targets_the_document_under_an_old_focused_hover_preview() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(
		dir.path().join("first.md"),
		"First preview paragraph.\n\nContinuation.",
	)
	.unwrap();
	std::fs::write(dir.path().join("second.md"), "Second destination.")
		.unwrap();
	std::fs::write(&source, "[First](first.md)\n\n[Second](second.md).\n")
		.unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source);
	let now = links::hover(&mut app, "first.md");
	finish_footnote(&mut app, now);
	let second = link_to(&app, "second.md");
	let popup = app.preview.floating().unwrap();
	assert!(
		popup.rect.contains(second.0, second.1),
		"{:?} {second:?}",
		popup.rect
	);
	let focus_at = center(popup.body());
	app.interaction.pointer.move_to(focus_at);
	app.pointer_pressed();
	app.pointer_released();
	assert!(app.preview.floating().unwrap().focused());
	touch(&mut app, 1, TouchPhase::Started, second);
	assert!(
		app.preview.floating().is_none(),
		"the mouse preview survived touch-down"
	);
	let due = app.gestures.preview_deadline().unwrap();
	app.preview_tick(due);
	touch(&mut app, 1, TouchPhase::Ended, second);
	settle_preview(&mut app, due, |_| true);
	assert!(links::text(&app).contains("Second destination."));
}

#[test]
fn an_unavailable_preview_keeps_the_held_finger_available_to_scroll() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(
		&source,
		format!("[Broken](#unavailable)\n\n{}", "Paragraph.\n\n".repeat(60)),
	)
	.unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source);
	let at = link_to(&app, "#unavailable");
	touch(&mut app, 1, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().unwrap();
	app.preview_tick(due);
	settle_preview(&mut app, due, |_| true);
	assert!(!app.preview.tracking());
	let end = (at.0, at.1 - 45.0);
	touch(&mut app, 1, TouchPhase::Moved, end);
	assert!(app.readers.session.scrolling.offset > 0.0);
	touch(&mut app, 1, TouchPhase::Ended, end);
}

#[test]
fn pending_sidebar_preview_does_not_present_at_an_old_scrolled_anchor() {
	let dir = crate::test_support::TestDir::new();
	for i in 0..50 {
		std::fs::write(
			dir.path().join(format!("{i:02}.md")),
			"Preview contents.",
		)
		.unwrap();
	}
	let mut app = folder::sidebar(dir.path(), Theme::Light);
	let (target, row) = folder::point(&mut app, "03.md");
	let due = hold(&mut app, 1, center(row));
	assert!(app.preview.open().is_none());
	let completion = app.preview.worker.wait(Duration::from_secs(10)).unwrap();
	app.scroll_folder(28.0);
	let moved = app.scene_preview_anchor(target).unwrap();
	assert_ne!(moved, row);
	app.preview.accept(completion);
	app.preview_tick(due + SHOW_DELAY);
	assert!(
		app.preview
			.floating()
			.is_none_or(|popup| popup.anchor == moved)
	);
}

#[test]
fn synthetic_mouse_events_cannot_close_a_preview_during_the_touch() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(dir.path().join("target.md"), "Preview contents.").unwrap();
	std::fs::write(
		&source,
		format!("[Preview](target.md)\n\n{}", "Paragraph.\n\n".repeat(60)),
	)
	.unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source);
	let at = link_to(&app, "target.md");
	touch(&mut app, 1, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().unwrap();
	app.preview_tick(due);
	settle_preview(&mut app, due, |_| true);
	app.gestures.allow_mouse();
	mouse_click(&mut app, MouseButton::Left);
	window_event(
		&mut app,
		WindowEvent::CursorLeft {
			device_id: DeviceId::from_raw(0),
		},
	);
	assert!(app.preview.floating().is_some());
	assert!(app.gestures.suppress_mouse());
	window_event(
		&mut app,
		WindowEvent::MouseWheel {
			device_id: DeviceId::from_raw(0),
			delta: MouseScrollDelta::PixelDelta((0.0, -70.0).into()),
			phase: TouchPhase::Started,
		},
	);
	assert!(
		app.gestures.suppress_mouse(),
		"wheel released the active touch"
	);
	window_event(
		&mut app,
		WindowEvent::MouseWheel {
			device_id: DeviceId::from_raw(0),
			delta: MouseScrollDelta::PixelDelta((0.0, -70.0).into()),
			phase: TouchPhase::Moved,
		},
	);
	app.preview_tick(due + SHOW_DELAY);
	assert!(app.preview.floating().is_some());
	assert_eq!(app.readers.session.scrolling.offset, 0.0);
	touch(&mut app, 1, TouchPhase::Ended, at);
}

#[test]
fn a_quick_tap_opens_while_the_previous_touch_preview_is_pending() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	let second = dir.path().join("second.md");
	std::fs::write(dir.path().join("first.md"), "First destination.").unwrap();
	std::fs::write(&second, "Second destination.").unwrap();
	std::fs::write(&source, "[First](first.md) and [Second](second.md).\n")
		.unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source);
	let first = link_to(&app, "first.md");
	let at = link_to(&app, "second.md");
	hold(&mut app, 1, first);
	assert!(app.preview.pending.is_some());
	assert!(app.preview.open().is_none());
	touch(&mut app, 2, TouchPhase::Started, at);
	touch(&mut app, 2, TouchPhase::Ended, at);
	settle(&mut app, &events);
	assert!(!app.preview.tracking());
	assert_eq!(
		app.readers.session.path.as_ref(),
		Some(&second),
		"the invisible preview swallowed the quick tap",
	);
}

#[test]
fn holding_an_unsupported_attachment_keeps_its_normal_open_confirmation() {
	let dir = crate::test_support::TestDir::new();
	let attachment = dir.path().join("example.attachment");
	std::fs::write(&attachment, "Attachment contents.").unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "example.attachment");
	let at = link_to(&app, "example.attachment");
	hold_without_preview(&mut app, at);
	assert!(!app.preview.tracking());
	assert_eq!(
		app.readers.session.path.as_ref(),
		Some(&dir.path().join("source.md")),
	);
	assert!(matches!(
		app.interaction.overlays.modal(),
		Some(crate::app::modal::Modal::OpenLocal { path, .. }) if path == &attachment
	));
}

fn hold_without_preview(app: &mut App<Proxy>, at: (f32, f32)) {
	touch(app, 1, TouchPhase::Started, at);
	let due = after_hold(app.gestures.preview_deadline());
	app.preview_tick(due);
	touch(app, 1, TouchPhase::Ended, at);
}

#[test]
fn holding_a_link_with_previews_off_opens_it() {
	let dir = crate::test_support::TestDir::new();
	let target = dir.path().join("target.md");
	std::fs::write(&target, "Destination.").unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "target.md");
	app.preferences
		.values
		.set_preview_size(PreviewKind::Link, PreviewSize::Off);
	let at = link_to(&app, "target.md");
	hold_without_preview(&mut app, at);
	assert_eq!(app.readers.session.path.as_ref(), Some(&target));
	assert!(!app.preview.tracking());
}

#[test]
fn holding_a_footnote_with_previews_off_jumps_to_it() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(&source, "Note[^note].\n\n[^note]: Destination.\n").unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source);
	app.preferences
		.values
		.set_preview_size(PreviewKind::Footnote, PreviewSize::Off);
	let at = link_point(&app, |link| {
		matches!(link, LinkTarget::FootnoteDefinition(_))
	});
	hold_without_preview(&mut app, at);
	assert!(app.readers.session.jump_origin.is_some());
	assert!(!app.preview.tracking());
}

#[test]
fn holding_a_sidebar_file_with_previews_off_opens_it() {
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("note.md");
	std::fs::write(&path, "Destination.").unwrap();
	let mut app = folder::sidebar(dir.path(), Theme::Light);
	app.preferences
		.values
		.set_preview_size(PreviewKind::Folder, PreviewSize::Off);
	let (_, row) = folder::point(&mut app, "note.md");
	hold_without_preview(&mut app, center(row));
	assert_eq!(app.readers.session.path.as_ref(), Some(&path));
	assert!(!app.preview.tracking());
}

#[test]
fn escape_cancels_a_pending_preview_before_the_search_bar() {
	use markview_platform::event::{ElementState, KeyEvent, WindowEvent};
	use winit::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};

	struct StubLoop;
	impl crate::app::window::Loop for StubLoop {
		fn exit(&self) {
			panic!("Escape closed the window");
		}
	}

	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	for find in [false, true] {
		let (mut app, _) = links::file_reader(dir.path(), "target.md");
		if find {
			app.open_search();
		}
		let at = link_to(&app, "target.md");
		let due = hold(&mut app, 1, at);
		assert!(app.preview.pending.is_some());
		assert!(app.preview.open().is_none());
		let completion =
			app.preview.worker.wait(Duration::from_secs(10)).unwrap();
		let key = Key::Named(NamedKey::Escape);
		app.handle_window_event(
			&StubLoop,
			winit::window::WindowId::from_raw(0),
			WindowEvent::KeyboardInput {
				device_id: DeviceId::from_raw(0),
				event: KeyEvent {
					physical_key: PhysicalKey::Code(KeyCode::Escape),
					key_without_modifiers: key.clone(),
					logical_key: key,
					text: None,
					text_with_all_modifiers: None,
					location: KeyLocation::Standard,
					state: ElementState::Pressed,
					repeat: false,
				},
				is_synthetic: false,
			},
		);
		assert!(!app.preview.tracking());
		assert_eq!(app.readers.session.search.open, find);
		app.preview.accept(completion);
		app.preview_tick(due + SHOW_DELAY);
		assert!(app.preview.open().is_none());
	}
}

#[test]
fn reloading_a_document_cannot_retarget_an_armed_link_hold() {
	check_reloaded_link(false, true);
}

#[test]
fn reloading_a_document_cannot_retarget_a_released_link_hold() {
	check_reloaded_link(true, false);
}

#[test]
fn a_rejected_hold_expiring_on_release_does_not_keep_the_contact() {
	check_reloaded_link(false, false);
}

fn check_reloaded_link(release_before_update: bool, tick_before_release: bool) {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	let other = dir.path().join("other.md");
	std::fs::write(dir.path().join("first.md"), "Original file.").unwrap();
	std::fs::write(&other, "Replacement file.").unwrap();
	std::fs::write(&source, "[Preview](first.md)\n\n%% Full reparse %%\n")
		.unwrap();
	let (mut app, events) = app();
	app.preferences.values.markdown_flavor =
		crate::settings::MarkdownFlavor::Obsidian;
	load(&mut app, &events, source.clone());
	let at = link_to(&app, "first.md");
	let target = app.document_preview_target(at, true).unwrap();
	let revision = app.document_interaction().snapshot.revision;
	std::fs::write(&source, "[Preview](other.md)\n\n%% Full reparse %%\n")
		.unwrap();
	app.reload_document(&source, false);
	assert_eq!(link_to(&app, "first.md"), at);
	touch(&mut app, 1, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().unwrap();
	if release_before_update {
		after_hold(Some(due));
		touch(&mut app, 1, TouchPhase::Ended, at);
		assert!(app.preview.tracking());
		assert!(app.preview.pending.is_none());
	}
	settle(&mut app, &events);
	assert_ne!(app.document_interaction().snapshot.revision, revision);
	assert_eq!(link_to(&app, "other.md"), at);
	assert_eq!(app.document_preview_target(at, true), Some(target));
	if tick_before_release {
		app.preview_tick(due);
	}
	if !release_before_update {
		if !tick_before_release {
			after_hold(Some(due));
		}
		touch(&mut app, 1, TouchPhase::Ended, at);
	}
	settle_preview(&mut app, due, |_| true);
	assert_eq!(app.readers.session.path.as_ref(), Some(&source));
	assert!(
		app.preview
			.open()
			.is_none_or(|open| open.path.as_ref() != Some(&other)),
		"the hold previewed the replacement link",
	);
	app.gestures.allow_mouse();
	assert!(!app.gestures.suppress_mouse());
	touch(&mut app, 2, TouchPhase::Started, at);
	assert!(app.gestures.preview_deadline().is_some());
	touch(&mut app, 2, TouchPhase::Cancelled, at);
}

fn recv(events: &mpsc::Receiver<Event>, deadline: Instant) -> Event {
	events
		.recv_timeout(deadline.saturating_duration_since(Instant::now()))
		.unwrap()
}

fn after_hold(due: Option<Instant>) -> Instant {
	let due =
		due.unwrap_or_else(|| Instant::now() + Duration::from_millis(510));
	std::thread::sleep(due.saturating_duration_since(Instant::now()));
	due
}

#[test]
fn pending_touch_preview_survives_a_reader_image_update() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	let source = dir.path().join("source.md");
	std::fs::write(
		&source,
		"[Preview](target.md)\n\n![Picture](picture.png)\n",
	)
	.unwrap();
	image::RgbaImage::new(32, 24)
		.save(dir.path().join("picture.png"))
		.unwrap();
	let (mut app, events) = app();
	app.open(source);
	let deadline = Instant::now() + Duration::from_secs(10);
	while app.readers.session.display.snapshot.blocks.is_empty() {
		accept_ready(&mut app, recv(&events, deadline));
	}
	assert!(
		app.readers
			.session
			.display
			.snapshot
			.images
			.decoded()
			.is_empty()
	);
	let at = link_point(&app, |link| matches!(link, LinkTarget::Url(_)));
	let due = hold(&mut app, 1, at);
	let pending = app.preview.pending.as_ref().unwrap().source.clone();
	let anchor = app.scene_preview_anchor(pending.target);
	assert_eq!(anchor, Some(pending.anchor));
	while app
		.readers
		.session
		.display
		.snapshot
		.images
		.decoded()
		.is_empty()
	{
		accept_ready(&mut app, recv(&events, deadline));
	}
	assert_eq!(app.scene_preview_anchor(pending.target), anchor);
	assert!(!pending.matches(&app));
	settle_preview(&mut app, due, |_| true);
	assert!(
		app.preview.floating().is_some(),
		"image update discarded the requested preview"
	);
	assert!(links::text(&app).contains("Destination."));
}

#[test]
fn a_long_press_outside_an_existing_popup_previews_the_next_link() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(dir.path().join("first.md"), "First destination.").unwrap();
	std::fs::write(dir.path().join("second.md"), "Second destination.")
		.unwrap();
	std::fs::write(
		&source,
		format!(
			"[First](first.md)\n\n{}[Second](second.md)\n",
			"Spacer.\n\n".repeat(6)
		),
	)
	.unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source.clone());
	let first = link_point(
		&app,
		|link| matches!(link, LinkTarget::Url(url) if url.as_ref() == "first.md"),
	);
	let second = link_point(
		&app,
		|link| matches!(link, LinkTarget::Url(url) if url.as_ref() == "second.md"),
	);
	let due = hold(&mut app, 1, first);
	settle_preview(&mut app, due, |_| true);
	assert!(
		!app.preview
			.floating()
			.unwrap()
			.rect
			.contains(second.0, second.1)
	);
	assert!(links::text(&app).contains("First destination."));
	touch(&mut app, 2, TouchPhase::Started, second);
	let due = app
		.gestures
		.preview_deadline()
		.unwrap_or(due + Duration::from_secs(1));
	app.preview_tick(due);
	touch(&mut app, 2, TouchPhase::Ended, second);
	settle_preview(&mut app, due, |_| true);
	assert_eq!(app.readers.session.path.as_ref(), Some(&source));
	assert!(
		app.preview.floating().is_some(),
		"second hold did not open a preview"
	);
	assert!(links::text(&app).contains("Second destination."));
}

#[test]
fn replacing_the_folder_cannot_retarget_an_armed_file_hold() {
	check_replaced_folder(false);
}

#[test]
fn a_reader_reload_does_not_close_a_sidebar_touch_preview() {
	check_sidebar_preview_during_reader_reload(false);
}

#[test]
fn a_reader_reload_does_not_discard_a_pending_sidebar_touch_preview() {
	check_sidebar_preview_during_reader_reload(true);
}

fn check_sidebar_preview_during_reader_reload(pending: bool) {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	let note = dir.path().join("note.md");
	std::fs::write(&source, "Original paragraph.").unwrap();
	std::fs::write(&note, "Sidebar preview.").unwrap();
	let (mut app, events) = app();
	app.folder.tree =
		Some(crate::app::folder::Tree::read(dir.path().to_owned()).unwrap());
	app.set_folder_open(true);
	app.folder.stop_animation();
	load(&mut app, &events, source.clone());
	let (target, row) = folder::point(&mut app, "note.md");
	let due = hold(&mut app, 1, center(row));
	if pending {
		assert!(app.preview.pending.is_some());
		assert!(app.preview.open().is_none());
	} else {
		settle_preview(&mut app, due, |_| true);
		assert!(links::text(&app).contains("Sidebar preview."));
	}
	let revision = app.document_interaction().snapshot.revision;
	std::fs::write(&source, "Revised paragraph.").unwrap();
	app.reload_document(&source, false);
	settle(&mut app, &events);
	assert_ne!(app.document_interaction().snapshot.revision, revision);
	assert_eq!(app.scene_preview_anchor(target), Some(row));
	app.preview_tick(due + SHOW_DELAY);
	if !pending {
		assert!(
			app.preview.open().is_some(),
			"reader reload closed the sidebar preview"
		);
	}
	settle_preview(&mut app, due, |_| true);
	assert_eq!(
		app.preview.open().and_then(|open| open.path.as_ref()),
		Some(&note)
	);
	assert!(links::text(&app).contains("Sidebar preview."));
}

#[test]
fn replacing_the_folder_cannot_retarget_a_released_file_hold() {
	check_replaced_folder(true);
}

fn check_replaced_folder(release_before_tick: bool) {
	let old = crate::test_support::TestDir::new();
	let new = crate::test_support::TestDir::new();
	std::fs::write(old.path().join("old.md"), "Old file.").unwrap();
	let image = new.path().join("new.png");
	image::RgbaImage::new(32, 24).save(&image).unwrap();
	let (mut app, events) = app();
	app.folder.tree =
		Some(crate::app::folder::Tree::read(old.path().to_owned()).unwrap());
	app.set_folder_open(true);
	app.folder.stop_animation();
	let (target, rect) = folder::point(&mut app, "old.md");
	let Target::File(id) = target else {
		unreachable!()
	};
	let at = center(rect);
	app.open_folder(new.path().to_owned(), true);
	touch(&mut app, 1, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().unwrap();
	if release_before_tick {
		after_hold(Some(due));
		touch(&mut app, 1, TouchPhase::Ended, at);
		assert!(app.gestures.preview_deadline().is_none());
		assert!(app.preview.tracking());
		assert!(app.preview.pending.is_none());
	}
	let deadline = Instant::now() + Duration::from_secs(10);
	loop {
		if let Event::FolderLoaded {
			generation,
			select,
			result,
		} = recv(&events, deadline)
		{
			assert!(result.as_ref().unwrap().initial().is_none());
			app.folder_loaded(generation, select, result);
			break;
		}
	}
	assert_eq!(app.folder_preview_path(id), Some(image.as_path()));
	app.preview_tick(due);
	if !release_before_tick {
		touch(&mut app, 1, TouchPhase::Ended, at);
	}
	settle_preview(&mut app, due, |_| true);
	assert!(app.readers.session.path.is_none());
	assert!(
		app.preview
			.open()
			.is_none_or(|open| open.path.as_ref() != Some(&image)),
		"hold previewed the replacement file"
	);
}

#[test]
fn expanded_file_row_hit_previews_on_hold_and_opens_on_tap() {
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("note.md");
	std::fs::write(&path, "Folder contents.").unwrap();
	for long in [false, true] {
		let mut app = folder::sidebar(dir.path(), Theme::Light);
		let (target, rect) = folder::point(&mut app, "note.md");
		let at = (center(rect).0, rect.y + rect.h + 3.0);
		assert!(!rect.contains(at.0, at.1));
		touch(&mut app, 1, TouchPhase::Started, at);
		if long {
			let due = after_hold(app.gestures.preview_deadline());
			touch(&mut app, 1, TouchPhase::Ended, at);
			settle_preview(&mut app, due, |_| true);
			assert!(
				app.readers.session.path.is_none(),
				"holding the file row opened it"
			);
			assert_eq!(
				app.preview.open().map(|open| open.target),
				Some(target)
			);
		} else {
			touch(&mut app, 1, TouchPhase::Ended, at);
			assert_eq!(app.readers.session.path.as_ref(), Some(&path));
		}
	}
}

#[test]
fn holding_a_tappable_file_row_while_the_drawer_opens_previews_it() {
	use crate::state::{ChromeCommand, Command};
	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("note.md"), "Folder contents.").unwrap();
	let mut app = folder::sidebar(dir.path(), Theme::Light);
	let (target, settled_row) = folder::point(&mut app, "note.md");
	let Target::File(id) = target else {
		unreachable!()
	};
	app.set_folder_open(false);
	app.folder.stop_animation();
	app.set_folder_open(true);
	app.folder.freeze_animation(Duration::from_millis(110));
	let opening_row = app
		.buttons()
		.into_iter()
		.find(|button| {
			button.action == Command::Chrome(ChromeCommand::FolderEntry(id))
		})
		.unwrap()
		.rect;
	assert_ne!(opening_row, settled_row);
	let at = center(opening_row.intersect(settled_row).unwrap());
	touch(&mut app, 1, TouchPhase::Started, at);
	app.folder.stop_animation();
	let due = after_hold(app.gestures.preview_deadline());
	touch(&mut app, 1, TouchPhase::Ended, at);
	settle_preview(&mut app, due, |_| true);
	assert!(
		app.readers.session.path.is_none(),
		"holding the opening row opened it"
	);
	assert_eq!(app.preview.open().map(|open| open.target), Some(target));
}

#[test]
fn a_visible_link_can_be_previewed_while_find_is_open() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	let (mut app, events) = links::file_reader(dir.path(), "target.md");
	for find in [false, true] {
		if find {
			app.open_search();
			app.request(false);
			settle(&mut app, &events);
			assert!(app.readers.session.search.open);
		}
		let at = link_point(&app, |link| matches!(link, LinkTarget::Url(_)));
		let due = hold(&mut app, 1, at);
		settle_preview(&mut app, due, |_| true);
		assert!(
			app.preview.floating().is_some(),
			"Find suppressed the link preview"
		);
		assert!(links::text(&app).contains("Destination."));
		assert!(app.chrome().overlay_with_form(None).1.is_some());
		assert_eq!(app.readers.session.search.open, find);
		app.dismiss_preview();
	}
}

#[test]
fn android_back_dismisses_a_touch_preview() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "target.md");
	app.tab_strip.phone = true;
	let at = link_point(&app, |link| matches!(link, LinkTarget::Url(_)));
	let due = hold(&mut app, 1, at);
	settle_preview(&mut app, due, |_| true);
	assert!(app.preview.floating().is_some());
	app.android_back();
	assert!(!app.preview.tracking(), "Back left the popup open");
}

#[test]
fn long_press_file_link_survives_release_before_the_worker_finishes() {
	let dir = crate::test_support::TestDir::new();
	let target = dir.path().join("target.md");
	std::fs::write(&target, "# Destination\n\nPreview this file.").unwrap();
	let (mut app, events) = links::file_reader(dir.path(), "target.md");
	app.tab_strip.phone = true;
	let at = link_point(&app, |link| matches!(link, LinkTarget::Url(_)));
	touch(&mut app, 1, TouchPhase::Started, at);
	let due = app.gestures.preview_deadline().unwrap();
	app.preview_tick(due - Duration::from_nanos(1));
	assert!(!app.preview.tracking());
	touch(&mut app, 1, TouchPhase::Moved, (at.0 + 2.0, at.1 + 2.0));
	assert_eq!(app.gestures.preview_deadline(), Some(due));
	app.preview_tick(due);
	assert!(app.preview.pending.is_some());
	assert!(app.preview.open().is_none());
	touch(&mut app, 1, TouchPhase::Ended, at);
	settle_preview(&mut app, due, |_| true);
	assert!(links::text(&app).contains("Preview this file"));
	assert_eq!(
		app.chrome().overlay_with_form(None).1,
		app.preview.floating().map(Floating::body)
	);
	assert_eq!(
		app.readers.session.path.as_deref(),
		Some(dir.path().join("source.md").as_path())
	);
	assert_eq!(app.readers.entries().len(), 1);

	touch(&mut app, 2, TouchPhase::Started, at);
	touch(&mut app, 2, TouchPhase::Ended, at);
	assert!(!app.preview.tracking());
	assert_eq!(app.readers.entries().len(), 1);
	touch(&mut app, 3, TouchPhase::Started, at);
	touch(&mut app, 3, TouchPhase::Ended, at);
	settle(&mut app, &events);
	assert_eq!(app.readers.session.path.as_ref(), Some(&target));
}

#[test]
fn long_press_previews_footnote_and_visible_section_without_jumping() {
	let dir = crate::test_support::TestDir::new();
	let source = dir.path().join("source.md");
	std::fs::write(
		&source,
		"[Section](#section) and note[^note].\n\n# Section\n\nSection excerpt.\n\n[^note]: Footnote excerpt.\n",
	).unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, source);
	for footnote in [true, false] {
		let at = link_point(&app, |link| {
			if footnote {
				matches!(link, LinkTarget::FootnoteDefinition(_))
			} else {
				matches!(link, LinkTarget::Url(url) if url.as_ref() == "#section")
			}
		});
		touch(&mut app, 1, TouchPhase::Started, at);
		let due = app.gestures.preview_deadline().unwrap();
		app.preview_tick(due);
		settle_preview(&mut app, due, |_| true);
		assert!(links::text(&app).contains(if footnote {
			"Footnote excerpt."
		} else {
			"Section excerpt."
		}));
		touch(&mut app, 1, TouchPhase::Ended, at);
		assert!(app.readers.session.jump_origin.is_none());
		assert!(app.preview.floating().is_some());
		app.dismiss_preview();
	}
}

#[test]
fn long_press_on_desktop_and_phone_sidebar_previews_file_and_can_scroll() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(
		dir.path().join("Notes.md"),
		format!("# Notes\n\n{}", "Preview paragraph.\n\n".repeat(80)),
	)
	.unwrap();
	for phone in [false, true] {
		let mut app = folder::sidebar(dir.path(), Theme::Light);
		app.tab_strip.phone = phone;
		if phone {
			app.switch_sidebar_tab(crate::state::SidebarTab::Folder);
			app.tab_strip.stop_animation();
		}
		let (target, row) = folder::point(&mut app, "Notes.md");
		let at = center(row);
		touch(&mut app, 1, TouchPhase::Started, at);
		let due = app.gestures.preview_deadline().unwrap();
		app.preview_tick(due);
		touch(&mut app, 1, TouchPhase::Ended, at);
		settle_preview(&mut app, due, |_| true);
		let open = app.preview.open().unwrap();
		assert_eq!(open.target, target);
		let body = open.floating.as_ref().unwrap().body();
		assert_eq!(app.chrome().overlay_with_form(None).1, Some(body));
		assert!(app.readers.session.path.is_none());
		let at = center(body);
		let end = (at.0, at.1 - 60.0);
		touch(&mut app, 2, TouchPhase::Started, at);
		touch(&mut app, 2, TouchPhase::Moved, end);
		touch(&mut app, 2, TouchPhase::Ended, end);
		assert!(app.preview.floating().unwrap().scroll.offset() > 0.0);
	}
}

#[test]
fn dragging_cancelling_and_a_second_contact_disarm_the_preview_hold() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(dir.path().join("target.md"), "Destination.").unwrap();
	let (mut app, _) = links::file_reader(dir.path(), "target.md");
	let at = link_point(&app, |link| matches!(link, LinkTarget::Url(_)));
	for (phase, second, end) in [
		(TouchPhase::Moved, 1, (at.0, at.1 + 32.0)),
		(TouchPhase::Cancelled, 1, at),
		(TouchPhase::Started, 2, (at.0 + 32.0, at.1)),
	] {
		touch(&mut app, 1, TouchPhase::Started, at);
		let due = app.gestures.preview_deadline().unwrap();
		touch(&mut app, second, phase, end);
		assert!(app.gestures.preview_deadline().is_none());
		app.preview_tick(due);
		touch(&mut app, 1, TouchPhase::Ended, at);
		touch(&mut app, 2, TouchPhase::Ended, end);
		assert!(!app.preview.tracking());
		assert_eq!(app.readers.entries().len(), 1);
	}
}
