use super::*;
use crate::{
	app::Event,
	cli::{LaunchOptions, Mode},
	test_support::TestDir,
};
use markview_layout::background::{Executor, Task};
use markview_platform::test_support::ExternalClipboard;
use std::{
	fs,
	sync::{Arc, mpsc},
	time::{Duration, Instant},
};

static CLIPBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Held(mpsc::Sender<Task>);
impl Executor for Held {
	fn try_submit(&self, task: Task) -> Result<(), Task> {
		self.0.send(task).map_err(|error| error.0)
	}
}

#[test]
fn selected_preview_refreshes_after_saves_and_recovers_from_invalid_content() {
	let dir = TestDir::new();
	let path = dir.path().join("target.md");
	fs::write(&path, "# Original").unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "target");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	assert!(text(&app).contains("Original"));
	for (atomic, source) in [
		(false, "# Saved directly\n\nA longer replacement."),
		(true, "# Saved atomically\n\nA replacement via rename."),
	] {
		if atomic {
			let temporary = dir.path().join("replacement.tmp");
			fs::write(&temporary, source).unwrap();
			fs::rename(temporary, &path).unwrap();
		} else {
			fs::write(&path, source).unwrap();
		}
		settle(&mut app, &events, |app| {
			app.interaction
				.overlays
				.quick_open
				.preview
				.document
				.is_some()
				&& text(app).contains(if atomic {
					"Saved atomically"
				} else {
					"Saved directly"
				})
		});
	}
	fs::write(&path, [0xff]).unwrap();
	settle(&mut app, &events, |app| {
		let preview = &app.interaction.overlays.quick_open.preview;
		preview.request.is_some()
			&& !preview.pending
			&& preview.document.is_none()
	});
	fs::write(&path, "# Recovered valid Markdown").unwrap();
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
			&& text(app).contains("Recovered valid Markdown")
	});
	assert!(app.readers.session.path.is_none());
	assert_eq!(app.focused_input(), Some(TextField::QuickOpen));
}

#[test]
fn saving_during_preview_layout_retries_the_selected_file() {
	let dir = TestDir::new();
	let path = dir.path().join("target.md");
	fs::write(&path, "# Before saving").unwrap();
	let (mut app, events) = setup(dir.path());
	let (send, jobs) = mpsc::channel();
	let mut services = crate::services::Services::new(1);
	services.handle.cpu = Arc::new(Held(send));
	app.services = Arc::new(services);
	let before = crate::watch::file_stamp(&path);
	query(&mut app, &events, "target");
	let first = jobs.recv_timeout(Duration::from_secs(5)).unwrap();
	let replacement =
		"# After saving\n\nThe updated file remains valid Markdown.";
	fs::write(&path, replacement).unwrap();
	assert_ne!(before, crate::watch::file_stamp(&path));
	assert_eq!(crate::file::read_markdown(&path).unwrap(), replacement);
	assert!(
		!markview_commonmark::parse(replacement)
			.document
			.blocks()
			.is_empty()
	);
	first.run();
	let deadline = Instant::now() + Duration::from_secs(5);
	loop {
		let event = events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.unwrap();
		let ready = matches!(&event, Event::QuickOpenPreviewReady);
		app.handle_user_event(&Loop, event);
		if ready {
			break;
		}
	}
	for _ in 0..3 {
		app.refresh_quick_open_preview();
	}
	let retry = jobs
		.recv_timeout(Duration::from_secs(5))
		.expect("a changed selected file must queue another preview");
	retry.run();
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	assert!(text(&app).contains("After saving"));
	assert!(!text(&app).contains("Before saving"));
	assert_eq!(
		app.interaction.overlays.quick_open.file().unwrap().path,
		path
	);
	assert!(app.readers.session.path.is_none());
}

#[derive(Clone)]
struct Proxy(mpsc::Sender<Event>);
impl SendEvent for Proxy {
	fn try_send(&self, event: Event) -> bool {
		self.0.send(event).is_ok()
	}
}
struct Loop;
impl crate::app::window::Loop for Loop {
	fn exit(&self) {}
}

fn setup(root: &std::path::Path) -> (App<Proxy>, mpsc::Receiver<Event>) {
	let (send, events) = mpsc::channel();
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			offline: true,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy(send),
	);
	app.ui = crate::test_support::shaper();
	app.folder_loaded(0, false, crate::app::folder::Tree::read(root.into()));
	app.open_quick_open();
	(app, events)
}
fn settle(
	app: &mut App<Proxy>,
	events: &mpsc::Receiver<Event>,
	done: impl Fn(&App<Proxy>) -> bool,
) {
	let deadline = Instant::now() + Duration::from_secs(10);
	while !done(app) {
		let event = events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.expect("picker preview did not settle");
		app.handle_user_event(&Loop, event);
	}
}
fn query(app: &mut App<Proxy>, events: &mpsc::Receiver<Event>, text: &str) {
	app.interaction
		.overlays
		.quick_open
		.search
		.input
		.set_text(&mut app.ui, text);
	app.quick_open_search_changed();
	settle(app, events, |app| {
		!app.interaction.overlays.quick_open.search.pending
	});
}
fn text(app: &App<Proxy>) -> String {
	app.interaction
		.overlays
		.quick_open
		.preview
		.document
		.as_ref()
		.unwrap()
		.snapshot
		.blocks
		.iter()
		.flat_map(|block| block.layout.text.iter())
		.map(|node| node.text.as_str())
		.collect()
}

#[test]
fn selected_file_renders_without_opening_a_tab_and_cancels_on_replacement() {
	let dir = TestDir::new();
	fs::write(
		dir.path().join("read-a.md"),
		"# Alpha\n\n**Formatted** text.",
	)
	.unwrap();
	fs::write(dir.path().join("read-b.md"), "# Beta\n\n> A quotation.")
		.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "read");
	let token = app
		.interaction
		.overlays
		.quick_open
		.preview
		.worker
		.token()
		.unwrap();
	app.quick_open_key(&Key::Named(NamedKey::ArrowDown));
	assert!(token.is_cancelled());
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	assert!(text(&app).contains("Beta"));
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.as_ref()
			.unwrap()
			.snapshot
			.document_box
			.is_none()
	);
	assert!(!text(&app).contains("Alpha"));
	assert!(app.readers.session.path.is_none());
	assert_eq!(app.focused_input(), Some(TextField::QuickOpen));
	let body = app.quick_open_geometry().body.unwrap();
	assert_eq!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.as_ref()
			.unwrap()
			.snapshot
			.width,
		body.w
	);
	query(&mut app, &events, "no-match");
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_none()
	);
	query(&mut app, &events, "read");
	let token = app
		.interaction
		.overlays
		.quick_open
		.preview
		.worker
		.token()
		.unwrap();
	app.close_quick_open();
	assert!(token.is_cancelled());
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_none()
	);
	app.refresh_quick_open_preview();
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.request
			.is_none()
	);
}

#[test]
fn empty_invalid_and_picture_files_have_stable_preview_states() {
	let dir = TestDir::new();
	fs::write(dir.path().join("empty.md"), "").unwrap();
	fs::write(dir.path().join("invalid.md"), [0xff]).unwrap();
	image::RgbaImage::new(17, 13)
		.save(dir.path().join("picture.png"))
		.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "empty");
	assert!(
		app.interaction.overlays.quick_open.file().is_some(),
		"empty query has no result"
	);
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.as_ref()
			.unwrap()
			.snapshot
			.blocks
			.is_empty()
	);
	query(&mut app, &events, "invalid");
	settle(&mut app, &events, |app| {
		!app.interaction.overlays.quick_open.preview.pending
	});
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_none()
	);
	query(&mut app, &events, "picture");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.as_ref()
			.is_some_and(|doc| {
				doc.snapshot.images.entries.values().any(|image| {
					image.size == Some((17, 13))
						&& !image.loading
						&& image.error.is_none()
				})
			})
	});
	let picker = &app.interaction.overlays.quick_open;
	let rect = picker.preview.document.as_ref().unwrap().body();
	assert_eq!((rect.w, rect.h), (17.0, 13.0));
	let column = app.quick_open_geometry().body.unwrap();
	assert_eq!(rect.x + rect.w / 2.0, column.x + column.w / 2.0);
	app.interaction
		.pointer
		.move_to((rect.x + 8.0, rect.y + 6.0));
	app.pointer_pressed();
	app.pointer_released();
	assert!(!app.interaction.overlays.quick_open.open());
	settle(&mut app, &events, |app| {
		app.interaction.overlays.viewer_open()
	});
}

#[test]
fn picker_preview_uses_its_column_with_all_hover_previews_off() {
	use crate::settings::{PreviewKind, PreviewSize};
	let dir = TestDir::new();
	fs::write(
		dir.path().join("target.md"),
		"# Picker content\n\nLong preview. ".repeat(200)
			+ "\n\nFinal paragraph.",
	)
	.unwrap();
	let (mut app, events) = setup(dir.path());
	for kind in [
		PreviewKind::Footnote,
		PreviewKind::Folder,
		PreviewKind::Link,
	] {
		app.preferences
			.values
			.set_preview_size(kind, PreviewSize::Off);
	}
	query(&mut app, &events, "target");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	let body = app.quick_open_geometry().body.unwrap();
	let preview = &app.interaction.overlays.quick_open.preview;
	assert_eq!(preview.request.as_ref().unwrap().options.width, body.w);
	assert_eq!(
		preview.request.as_ref().unwrap().options.viewport_height,
		body.h
	);
	assert_eq!(preview.document.as_ref().unwrap().snapshot.width, body.w);
	assert!(text(&app).contains("Picker content"));
	app.interaction
		.pointer
		.move_to((body.x + 32.0, body.y + 16.0));
	app.pointer_pressed();
	app.pointer_released();
	assert!(app.focused_input().is_none());
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::CONTROL);
	assert!(app.quick_open_key(&Key::Character("a".into())));
	settle(&mut app, &events, |app| {
		!app.interaction.overlays.quick_open.preview.pending
	});
	assert!(app.selected_text().unwrap().ends_with("Final paragraph."));
	let request = app.animations.scrolls.wheel(
		160.0,
		Instant::now(),
		markview_interaction::WheelMode::Direct,
	);
	app.wheel_quick_open(request);
	let picker = &app.interaction.overlays.quick_open;
	assert_eq!(picker.scroll(), 0.0);
	assert_eq!(
		picker.preview.document.as_ref().unwrap().scroll.offset(),
		160.0
	);
	assert!(app.readers.session.path.is_none());
}

#[test]
fn clicking_a_picker_link_follows_it_from_the_selected_file() {
	let dir = TestDir::new();
	let target = dir.path().join("destination.md");
	fs::write(&target, "# Destination").unwrap();
	fs::write(dir.path().join("source.md"), "[Read more](destination.md)")
		.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "source");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	let doc = app
		.interaction
		.overlays
		.quick_open
		.preview
		.document
		.as_ref()
		.unwrap();
	let block = &doc.snapshot.blocks[0];
	let (_, link) = block.links().next().unwrap();
	let body = doc.body();
	app.interaction.pointer.move_to((
		body.x + link.x + link.w / 2.0,
		body.y + block.y + link.y + link.h / 2.0,
	));
	assert_eq!(app.refresh_hover(), winit::cursor::CursorIcon::Pointer);
	let link_point = app.interaction.pointer.cursor();
	assert!(picker_doc(&app).state.hover.link.is_some());
	let input = app.quick_open_geometry().input;
	app.pointer_moved(winit::dpi::PhysicalPosition::new(
		(input.x + 5.0) as f64,
		(input.y + 5.0) as f64,
	));
	app.refresh_hover();
	assert!(picker_doc(&app).state.hover.link.is_none());
	app.interaction.pointer.move_to(link_point);
	app.refresh_hover();
	app.pointer_pressed();
	app.pointer_released();
	assert!(!app.interaction.overlays.quick_open.open());
	settle(&mut app, &events, |app| {
		app.readers.session.path.as_ref() == Some(&target)
	});
}

fn long_picker(root: &std::path::Path) -> App<Proxy> {
	fs::write(root.join("long.md"), "Paragraph.\n\n".repeat(200)).unwrap();
	let (mut app, events) = setup(root);
	query(&mut app, &events, "long");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
			&& !app.interaction.overlays.quick_open.preview.pending
	});
	assert!(picker_doc(&app).view().max_scroll() > 100.0);
	app
}
fn picker_doc(app: &App<Proxy>) -> &FloatingDocument {
	app.interaction
		.overlays
		.quick_open
		.preview
		.document
		.as_ref()
		.unwrap()
}

fn preview_has_decoded_images(app: &App<Proxy>) -> bool {
	app.interaction
		.overlays
		.quick_open
		.preview
		.document
		.as_ref()
		.is_some_and(|doc| !doc.snapshot.images.decoded().is_empty())
}

#[test]
fn touch_drag_scrolls_the_preview() {
	use markview_platform::event::{DeviceId, Touch, TouchPhase};
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	let rect = picker_doc(&app).body();
	for (phase, y) in [
		(TouchPhase::Started, 120.0),
		(TouchPhase::Moved, 40.0),
		(TouchPhase::Ended, 40.0),
	] {
		app.handle_touch(Touch {
			device_id: DeviceId::from_raw(0),
			id: 1,
			phase,
			location: winit::dpi::PhysicalPosition::new(
				(rect.x + 50.0) as f64,
				(rect.y + y) as f64,
			),
			force: None,
		});
	}
	assert!(picker_doc(&app).scroll.offset() > 0.0);
	let now = Instant::now();
	let dragged = picker_doc(&app).scroll.offset();
	assert!(app.advance_gestures(now + Duration::from_millis(10)));
	let coasting = picker_doc(&app).scroll.offset();
	assert!(coasting > dragged);
	app.interaction
		.pointer
		.move_to((rect.x + 50.0, rect.y + 40.0));
	send_wheel(
		&mut app,
		markview_platform::event::MouseScrollDelta::LineDelta(0.0, 1.0),
		TouchPhase::Moved,
	);
	app.advance_gestures(now + Duration::from_millis(20));
	assert_eq!(picker_doc(&app).scroll.offset(), coasting);
	app.advance_control_scroll(now + Duration::from_secs(1));
	assert!(picker_doc(&app).scroll.offset() < coasting);
}

#[test]
fn focus_loss_ends_preview_thumb_drag() {
	use markview_platform::event::WindowEvent;
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	let thumb = picker_doc(&app).scrollbar().unwrap().thumb;
	let x = thumb.x + thumb.w / 2.0;
	app.interaction.pointer.move_to((x, thumb.y + 2.0));
	app.pointer_pressed();
	assert!(picker_doc(&app).state.captured());
	for focused in [false, true] {
		app.handle_window_event(
			&Loop,
			winit::window::WindowId::from_raw(0),
			WindowEvent::Focused(focused),
		);
	}
	app.pointer_moved(winit::dpi::PhysicalPosition::new(
		x as f64,
		(thumb.y + 100.0) as f64,
	));
	assert_eq!(picker_doc(&app).scroll.offset(), 0.0);
	assert!(!picker_doc(&app).state.captured());
}

#[test]
fn open_extensionless_picture_previews_like_the_reader() {
	let dir = TestDir::new();
	let path = dir.path().join("picture");
	image::RgbaImage::new(17, 13)
		.save_with_format(&path, image::ImageFormat::Png)
		.unwrap();
	let (mut app, events) = setup(dir.path());
	app.close_quick_open();
	app.open_document(
		path.clone(),
		crate::security::Security::local(crate::security::Trust::Trusted),
	);
	settle(&mut app, &events, |app| {
		!app.readers
			.session
			.display
			.snapshot
			.images
			.decoded()
			.is_empty()
	});
	app.open_quick_open();
	query(&mut app, &events, "picture");
	assert_eq!(
		app.interaction.overlays.quick_open.file().unwrap().path,
		path
	);
	settle(&mut app, &events, |app| {
		!app.interaction.overlays.quick_open.preview.pending
			&& app
				.interaction
				.overlays
				.quick_open
				.preview
				.request
				.is_some()
	});
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	);
	settle(&mut app, &events, preview_has_decoded_images);
}

#[test]
fn shift_wheel_pans_the_wide_preview_block_and_resize_keeps_its_position() {
	use markview_platform::event::{
		DeviceId, MouseScrollDelta, TouchPhase, WindowEvent,
	};
	let dir = TestDir::new();
	let text = format!(
		"```\n{}\n```\n\n{}",
		"wide ".repeat(200),
		"body\n\n".repeat(100)
	);
	fs::write(dir.path().join("wide.md"), text).unwrap();
	let (mut app, events) = setup(dir.path());
	app.preferences.values.codeblock_wrap = false;
	query(&mut app, &events, "wide");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
			&& !app.interaction.overlays.quick_open.preview.pending
	});
	let doc = picker_doc(&app);
	assert!(doc.view().max_scroll() > 100.0);
	let block = &doc.snapshot.blocks[0];
	let rect = block.overflow_rect(0).unwrap();
	assert!(block.layout.overflow[0].content_width > rect.w + 100.0);
	let (x, y) = (
		doc.body().x + rect.x + 5.0,
		doc.body().y + block.y + rect.y + 5.0,
	);
	app.interaction.pointer.move_to((x, y));
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::SHIFT);
	app.handle_window_event(
		&Loop,
		winit::window::WindowId::from_raw(0),
		WindowEvent::MouseWheel {
			device_id: DeviceId::from_raw(0),
			delta: MouseScrollDelta::LineDelta(0.0, -1.0),
			phase: TouchPhase::Moved,
		},
	);
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert_eq!(picker_doc(&app).scroll.offset(), 0.0);
	assert!(picker_doc(&app).horizontal[&(0, 0)] > 0.0);
	let position = picker_doc(&app).horizontal[&(0, 0)];
	let mut body = picker_doc(&app).body();
	body.h -= 48.0;
	app.refresh_quick_open_body(Some(body));
	while app.interaction.overlays.quick_open.preview.pending {
		if matches!(
			events.recv_timeout(Duration::from_secs(10)).unwrap(),
			Event::QuickOpenPreviewReady
		) {
			app.refresh_quick_open_body(Some(body));
		}
	}
	let doc = picker_doc(&app);
	assert_eq!(doc.body(), body);
	assert_eq!(doc.snapshot.blocks[0].overflow_rect(0).unwrap().w, rect.w);
	assert_eq!(
		doc.horizontal.get(&(0, 0)).copied().unwrap_or_default(),
		position
	);
}

#[test]
fn held_picker_thumb_owns_native_pan_input() {
	use markview_platform::event::TouchPhase;
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	let thumb = picker_doc(&app).scrollbar().unwrap().thumb;
	app.interaction
		.pointer
		.move_to((thumb.x + thumb.w / 2.0, thumb.y + 2.0));
	app.pointer_pressed();
	assert!(picker_doc(&app).state.captured());
	let held = app.pointer_owns_input();
	if !held {
		app.trackpad_scroll(
			0.0,
			-50.0,
			TouchPhase::Started,
			crate::app::gestures::Inertia::Native,
		);
	}
	assert_eq!(picker_doc(&app).scroll.offset(), 0.0);
	assert!(held);
}

#[test]
fn touch_tap_opens_picker_image() {
	use markview_platform::event::{DeviceId, Touch, TouchPhase};
	let dir = TestDir::new();
	image::RgbaImage::new(17, 13)
		.save(dir.path().join("picture.png"))
		.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "picture");
	settle(&mut app, &events, preview_has_decoded_images);
	let rect = picker_doc(&app).body();
	for phase in [TouchPhase::Started, TouchPhase::Ended] {
		app.handle_touch(Touch {
			device_id: DeviceId::from_raw(0),
			id: 1,
			phase,
			location: winit::dpi::PhysicalPosition::new(
				(rect.x + 8.0) as f64,
				(rect.y + 6.0) as f64,
			),
			force: None,
		});
	}
	assert!(!app.interaction.overlays.quick_open.open());
	settle(&mut app, &events, |app| {
		app.interaction.overlays.viewer_open()
	});
}

fn send_wheel(
	app: &mut App<Proxy>,
	delta: markview_platform::event::MouseScrollDelta,
	phase: markview_platform::event::TouchPhase,
) {
	use markview_platform::event::{DeviceId, WindowEvent};
	app.handle_window_event(
		&Loop,
		winit::window::WindowId::from_raw(0),
		WindowEvent::MouseWheel {
			device_id: DeviceId::from_raw(0),
			delta,
			phase,
		},
	);
}

#[test]
fn pixel_stream_stays_with_its_preview_after_crossing_results() {
	use markview_platform::event::{MouseScrollDelta, TouchPhase};
	let dir = TestDir::new();
	for i in 0..30 {
		fs::write(
			dir.path().join(format!("long-{i}.md")),
			"row\n\n".repeat(200),
		)
		.unwrap();
	}
	let mut app = long_picker(dir.path());
	assert!(app.quick_open_list().max_scroll() > 100.0);
	let body = picker_doc(&app).body();
	app.interaction
		.pointer
		.move_to((body.x + 20.0, body.y + 20.0));
	let pixel = |y| {
		MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, y))
	};
	send_wheel(&mut app, pixel(0.0), TouchPhase::Started);
	send_wheel(&mut app, pixel(-60.0), TouchPhase::Moved);
	let first = picker_doc(&app).scroll.offset();
	let list = app.quick_open_list().viewport;
	app.interaction
		.pointer
		.move_to((list.x + 10.0, list.y + 10.0));
	send_wheel(&mut app, pixel(-60.0), TouchPhase::Moved);
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert_eq!(
		(
			first,
			picker_doc(&app).scroll.offset(),
			app.interaction.overlays.quick_open.scroll()
		),
		(60.0, 120.0, 0.0)
	);
}

#[test]
fn picker_document_wheel_uses_reading_speed() {
	use markview_platform::event::{MouseScrollDelta, TouchPhase};
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	let body = picker_doc(&app).body();
	app.interaction
		.pointer
		.move_to((body.x + 30.0, body.y + 20.0));
	app.preferences.values.scroll_speed = 2.0;
	let delta = MouseScrollDelta::LineDelta(0.0, -1.0);
	let expected = -crate::app::pointer::wheel_pixels(
		delta,
		app.wheel_notch,
		app.scroll_speed(),
		app.dimensions().2,
		app.viewport_size(),
	)
	.1;
	send_wheel(&mut app, delta, TouchPhase::Moved);
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert_eq!(picker_doc(&app).scroll.offset(), expected);
}

#[test]
#[ignore = "requires a desktop clipboard"]
fn refocused_picker_copies_retained_selection() {
	let _clipboard = CLIPBOARD.lock().unwrap();
	use markview_platform::event::WindowEvent;
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	let body = picker_doc(&app).body();
	app.interaction
		.pointer
		.move_to((body.x + 32.0, body.y + 16.0));
	app.pointer_pressed();
	app.pointer_released();
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::CONTROL);
	assert!(app.quick_open_key(&Key::Character("a".into())));
	let selected = app.selected_text().unwrap();
	for focused in [false, true] {
		app.handle_window_event(
			&Loop,
			winit::window::WindowId::from_raw(0),
			WindowEvent::Focused(focused),
		);
	}
	assert_eq!(app.selected_text().as_ref(), Some(&selected));
	app.clipboard.write("sentinel".into()).unwrap();
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::CONTROL);
	app.key_pressed(&Key::Character("c".into()));
	assert_eq!(app.clipboard.read().unwrap(), selected);
}

#[test]
fn shift_wheel_over_preview_prose_scrolls_vertically() {
	use markview_platform::event::{MouseScrollDelta, TouchPhase};
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	let doc = picker_doc(&app);
	assert!(
		doc.snapshot
			.blocks
			.iter()
			.all(|b| b.layout.overflow.is_empty())
	);
	let body = doc.body();
	app.interaction
		.pointer
		.move_to((body.x + 24.0, body.y + 24.0));
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::SHIFT);
	send_wheel(
		&mut app,
		MouseScrollDelta::LineDelta(0.0, -1.0),
		TouchPhase::Moved,
	);
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert!(picker_doc(&app).scroll.offset() > 0.0);
	assert_eq!(app.interaction.overlays.quick_open.scroll(), 0.0);
}

#[test]
fn resizing_keeps_the_selected_passage_and_scroll_position() {
	let dir = TestDir::new();
	fs::write(
		dir.path().join("passage.md"),
		(0..200)
			.map(|i| format!("Distinct paragraph {i}.\n\n"))
			.collect::<String>(),
	)
	.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "passage");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
			&& !app.interaction.overlays.quick_open.preview.pending
	});
	let doc = picker_doc(&app);
	let body = doc.body();
	app.interaction
		.pointer
		.move_to((body.x + 20.0, body.y + 20.0));
	app.pointer_pressed();
	app.pointer_released();
	let doc = app
		.interaction
		.overlays
		.quick_open
		.preview
		.document
		.as_mut()
		.unwrap();
	let offset = doc.snapshot.blocks[100].y;
	doc.scroll.jump_to(offset);
	let start = doc
		.snapshot
		.semantic_at(100, 0, 0, markview_layout::text::Affinity::After)
		.unwrap();
	let end = doc
		.snapshot
		.semantic_at(100, 0, 8, markview_layout::text::Affinity::After)
		.unwrap();
	doc.state.selection = Some(markview_layout::text::TextSelection {
		anchor: start,
		focus: end,
	});
	let selected = doc.selected_text().unwrap();
	assert_eq!(selected, "Distinct");
	for (w, h) in [(1200.0, 680.0), (1000.0, 680.0)] {
		let next = crate::app::chrome::quick_open::geometry((w, h), 1, 0.0)
			.body
			.unwrap();
		app.refresh_quick_open_body(Some(next));
		while app.interaction.overlays.quick_open.preview.pending {
			let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
			if matches!(event, Event::QuickOpenPreviewReady) {
				app.refresh_quick_open_body(Some(next));
			}
		}
		let doc = picker_doc(&app);
		assert_eq!(doc.body(), next);
		assert_eq!(
			(doc.focused(), doc.selected_text(), doc.scroll.offset()),
			(true, Some(selected.clone()), doc.snapshot.blocks[100].y)
		);
	}
}

#[test]
fn picture_completion_between_press_and_release_keeps_the_click() {
	let dir = TestDir::new();
	image::RgbaImage::new(17, 13)
		.save(dir.path().join("picture.png"))
		.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "picture");
	settle(&mut app, &events, preview_has_decoded_images);
	let doc = picker_doc(&app);
	let rect = doc.body();
	let point = markview_interaction::Point::new(rect.x + 8.0, rect.y + 6.0);
	let src = doc.view().image(point).unwrap().0;
	let next = doc.snapshot.clone();
	let body = app.quick_open_geometry().body.unwrap();
	app.interaction.pointer.move_to((point.x, point.y));
	app.pointer_pressed();
	assert!(picker_doc(&app).state.captured());
	app.interaction.overlays.quick_open.preview.complete(
		Some(next),
		body,
		true,
	);
	assert_eq!(picker_doc(&app).view().image(point).unwrap().0, src);
	app.pointer_released();
	assert!(!app.interaction.overlays.quick_open.open());
	settle(&mut app, &events, |app| {
		app.interaction.overlays.viewer_open()
	});
}

#[test]
fn inline_image_completion_keeps_the_visible_paragraph_anchored() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let source = format!(
		"![](photo.png)\n\n{}",
		(0..100)
			.map(|i| format!("Paragraph {i}\n\n"))
			.collect::<String>()
	);
	let parsed = markview_commonmark::parse(source);
	let mut options = crate::test_support::options();
	options.width = 500.0;
	options.preview = true;
	let body = Rect {
		x: 0.0,
		y: 0.0,
		w: 500.0,
		h: 300.0,
	};
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		ImageSource::Resource(ResourceRef::document("photo.png")),
		"photo.png".into(),
	);
	let mut picker = Preview::default();
	let mut engine = crate::layout::LayoutEngine::new();
	for height in [None, Some(500)] {
		images.entries.insert(
			"photo.png".into(),
			ImageInfo {
				size: height.map(|h| (300, h)),
				version: u64::from(height.is_some()),
				loading: height.is_none(),
				..Default::default()
			},
		);
		images.generation += 1;
		let snap =
			engine.layout_with_images(&parsed.document, &options, &images);
		let passage = snap.blocks[30].y;
		if let Some(doc) = &picker.document {
			assert!(passage - doc.scroll.offset() > 100.0);
		}
		picker.complete(Some(snap), body, false);
		let doc = picker.document.as_mut().unwrap();
		if height.is_none() {
			doc.scroll.jump_to(passage);
		}
		assert_eq!(doc.scroll.offset(), passage);
	}
}

#[test]
fn tapping_prose_gives_the_preview_keyboard_selection() {
	use markview_platform::event::{DeviceId, Touch, TouchPhase};
	let dir = TestDir::new();
	let mut app = long_picker(dir.path());
	assert_eq!(app.focused_input(), Some(TextField::QuickOpen));
	let doc = picker_doc(&app);
	let block = &doc.snapshot.blocks[0];
	let (_, _, cluster) = block.clusters().next().unwrap();
	let body = doc.body();
	let x = body.x + cluster.x + cluster.w / 2.0;
	let y = body.y + block.y + cluster.y + cluster.h / 2.0;
	let point = markview_interaction::Point::new(x, y);
	assert_eq!(
		doc.view().hover(point, false).cursor,
		markview_interaction::Cursor::Text
	);
	for phase in [TouchPhase::Started, TouchPhase::Ended] {
		app.handle_touch(Touch {
			device_id: DeviceId::from_raw(0),
			id: 1,
			phase,
			location: winit::dpi::PhysicalPosition::new(x as f64, y as f64),
			force: None,
		});
	}
	assert_eq!(app.focused_input(), None);
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::CONTROL);
	assert!(app.quick_open_key(&Key::Character("a".into())));
	assert!(app.selected_text().unwrap().starts_with("Paragraph."));
}

#[test]
fn down_during_preview_reflow_keeps_the_selected_file_and_text() {
	let dir = TestDir::new();
	for name in ["target-a.md", "target-b.md"] {
		fs::write(dir.path().join(name), "Selected passage.\n\n".repeat(100))
			.unwrap();
	}
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "target");
	settle(&mut app, &events, |app| {
		!app.interaction.overlays.quick_open.preview.pending
			&& app
				.interaction
				.overlays
				.quick_open
				.preview
				.document
				.is_some()
	});
	assert_eq!(app.interaction.overlays.quick_open.search.matches.len(), 2);
	let body = picker_doc(&app).body();
	app.interaction
		.pointer
		.move_to((body.x + 24.0, body.y + 24.0));
	app.pointer_pressed();
	app.pointer_released();
	assert!(app.preview_input(markview_interaction::ViewInput::SelectAll));
	let selected = app.selected_text().unwrap();
	let next = Rect {
		h: body.h - 80.0,
		..body
	};
	app.refresh_quick_open_body(Some(next));
	assert!(app.interaction.overlays.quick_open.preview.pending);
	assert!(app.quick_open_key(&Key::Named(NamedKey::ArrowDown)));
	assert_eq!(app.interaction.overlays.quick_open.selected(), 0);
	while app.interaction.overlays.quick_open.preview.pending {
		if matches!(
			events.recv_timeout(Duration::from_secs(10)).unwrap(),
			Event::QuickOpenPreviewReady
		) {
			app.refresh_quick_open_body(Some(next));
		}
	}
	assert_eq!(app.selected_text(), Some(selected));
}

#[test]
fn end_in_a_partial_preview_reaches_the_final_paragraph() {
	check_end_in_a_partial_preview(false);
}

#[test]
fn end_in_a_partial_preview_survives_reloading_the_selected_file() {
	check_end_in_a_partial_preview(true);
}

fn check_end_in_a_partial_preview(reload: bool) {
	let dir = TestDir::new();
	let path = dir.path().join("long.md");
	let paragraphs = "A longer document.\n\n".repeat(80);
	let source = format!("{paragraphs}Final paragraph.");
	fs::write(&path, &source).unwrap();
	let (mut app, events) = setup(dir.path());
	let (send, jobs) = mpsc::channel();
	let mut services = crate::services::Services::new(1);
	services.handle.cpu = Arc::new(Held(send));
	app.services = Arc::new(services);
	query(&mut app, &events, "long");
	let task = jobs.recv_timeout(Duration::from_secs(10)).unwrap();
	let body = app.quick_open_geometry().body.unwrap();
	let preview = &mut app.interaction.overlays.quick_open.preview;
	let parsed = markview_commonmark::parse(source);
	let partial = crate::layout::LayoutEngine::new()
		.layout_prefix_cancellable(
			&parsed.document,
			&preview.request.as_ref().unwrap().options,
			&Default::default(),
			body.h * 3.0,
			|| false,
		)
		.unwrap();
	assert!(partial.blocks.len() < parsed.document.blocks().len());
	preview.complete(Some(partial), body, false);
	preview.pending = true;
	preview.document.as_mut().unwrap().state.focused = true;
	app.clear_input_focus();
	assert!(app.quick_open_key(&Key::Named(NamedKey::End)));
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	let partial_end = picker_doc(&app).view().max_scroll();
	assert!(partial_end > 0.0);
	assert_eq!(picker_doc(&app).scroll.offset(), partial_end);
	task.run();
	let final_text = if reload {
		let preview = &app.interaction.overlays.quick_open.preview;
		preview.worker.wait_for_final_publication();
		let replacement = "Updated final paragraph.";
		fs::write(path, format!("{paragraphs}{replacement}")).unwrap();
		app.refresh_quick_open_preview();
		assert!(
			app.interaction
				.overlays
				.quick_open
				.preview
				.document
				.is_none()
		);
		jobs.recv_timeout(Duration::from_secs(10)).unwrap().run();
		replacement
	} else {
		"Final paragraph."
	};
	let worker = &app.interaction.overlays.quick_open.preview.worker;
	worker.wait_for_final_publication();
	let completion = worker.try_recv();
	app.accept_quick_open_completion(completion, body);
	assert!(!app.interaction.overlays.quick_open.preview.pending);
	assert!(text(&app).ends_with(final_text));
	assert!(picker_doc(&app).view().max_scroll() > partial_end);
	assert_eq!(
		picker_doc(&app).scroll.offset(),
		picker_doc(&app).view().max_scroll()
	);
}

#[test]
fn unregistered_image_in_partial_preview_waits_for_its_resource() {
	let dir = TestDir::new();
	image::RgbaImage::new(20, 20)
		.save(dir.path().join("photo.png"))
		.unwrap();
	let source = format!("![](photo.png)\n\n{}", "Paragraph.\n\n".repeat(100));
	fs::write(dir.path().join("article.md"), &source).unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "article");
	let body = app.quick_open_geometry().body.unwrap();
	let preview = &mut app.interaction.overlays.quick_open.preview;
	let options = &preview.request.as_ref().unwrap().options;
	let partial = crate::layout::LayoutEngine::new().layout_prefix_cancellable(
		&markview_commonmark::parse(source).document,
		options,
		&Default::default(),
		body.h,
		|| false,
	);
	// The displayed preview still has its placeholder when the worker finishes.
	preview.complete(partial, body, false);
	preview.pending = true;
	preview.worker.wait_for_final_publication();

	let doc = picker_doc(&app);
	let block = &doc.snapshot.blocks[0];
	let rect = block
		.draws()
		.find_map(|(_, draw, _)| match draw {
			crate::layout::Draw::Image { src, rect, .. } if src.is_empty() => {
				Some(rect)
			}
			_ => None,
		})
		.expect("partial layout contains the pending image");
	let point = (
		doc.body().x + rect.x + rect.w / 2.0,
		doc.body().y + block.y + rect.y + rect.h / 2.0,
	);
	assert!(doc.body().contains(point.0, point.1));
	app.interaction.pointer.move_to(point);
	app.pointer_pressed();
	app.pointer_released();
	assert!(app.interaction.overlays.quick_open.open());
	assert!(app.interaction.overlays.quick_open.preview.pending);
	settle(&mut app, &events, preview_has_decoded_images);
}

#[test]
fn clicking_a_preview_after_a_save_waits_for_the_new_document() {
	let dir = TestDir::new();
	let path = dir.path().join("article.md");
	fs::write(&path, "[Open another file](destination.md)").unwrap();
	fs::write(dir.path().join("destination.md"), "# Destination").unwrap();
	image::RgbaImage::new(20, 20)
		.save(dir.path().join("photo.png"))
		.unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "article");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	let doc = picker_doc(&app);
	let block = &doc.snapshot.blocks[0];
	let (_, link) = block.links().next().unwrap();
	let body = doc.body();
	app.interaction.pointer.move_to((
		body.x + link.x + link.w / 2.0,
		body.y + block.y + link.y + link.h / 2.0,
	));
	fs::write(&path, "![](photo.png)\n\nUpdated document.").unwrap();
	app.pointer_pressed();
	app.pointer_released();
	let picker = &app.interaction.overlays.quick_open;
	assert!(picker.open());
	assert!(picker.preview.pending);
	assert!(picker.preview.document.is_none());
	settle(&mut app, &events, preview_has_decoded_images);
	assert!(text(&app).contains("Updated document."));
	assert!(app.readers.session.path.is_none());
}

#[test]
#[ignore = "requires a desktop clipboard"]
fn closing_picker_after_copy_finishes_the_requested_full_text() {
	let _clipboard = CLIPBOARD.lock().unwrap();
	let dir = TestDir::new();
	let (mut app, events) = deferred_copy_fixture(&dir, |app| {
		app.clipboard.write("sentinel".into()).unwrap();
	});
	app.quick_open_key(&Key::Named(NamedKey::Escape));
	assert!(!app.interaction.overlays.quick_open.open());
	let deadline = Instant::now() + Duration::from_secs(10);
	while app.clipboard.read().unwrap() == "sentinel"
		&& Instant::now() < deadline
	{
		if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
			app.handle_user_event(&Loop, event);
		}
	}
	let copied = app.clipboard.read().unwrap();
	assert!(
		copied.ends_with("Last paragraph."),
		"clipboard ends with {:?}",
		copied.lines().last()
	);
	assert_eq!(copied.matches("Reading text.").count(), 3000);
}

fn deferred_copy_fixture(
	dir: &TestDir,
	setup_clipboard: impl FnOnce(&mut App<Proxy>),
) -> (App<Proxy>, mpsc::Receiver<Event>) {
	fs::write(
		dir.path().join("text-a.md"),
		"Reading text.\n\n".repeat(3000) + "Last paragraph.",
	)
	.unwrap();
	fs::write(dir.path().join("text-b.md"), "Second result.").unwrap();
	let (mut app, events) = setup(dir.path());
	query(&mut app, &events, "text");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.document
			.is_some()
	});
	let body = picker_doc(&app).body();
	app.interaction
		.pointer
		.move_to((body.x + 24.0, body.y + 24.0));
	app.pointer_pressed();
	app.pointer_released();
	setup_clipboard(&mut app);
	app.interaction
		.pointer
		.set_modifiers(winit::keyboard::ModifiersState::CONTROL);
	assert!(app.quick_open_key(&Key::Character("a".into())));
	assert!(app.quick_open_key(&Key::Character("c".into())));
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.copy_pending
			.is_some()
	);

	(app, events)
}

#[test]
#[ignore = "requires a desktop clipboard"]
fn row_change_keeps_a_queued_copy_result() {
	let _clipboard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
	let dir = TestDir::new();
	let (mut app, events) = deferred_copy_fixture(&dir, |app| {
		app.clipboard.write("sentinel".into()).unwrap();
	});
	app.quick_open_key(&Key::Named(NamedKey::Tab));
	app.interaction
		.overlays
		.quick_open
		.preview
		.worker
		.wait_for_final_publication();
	app.quick_open_key(&Key::Named(NamedKey::ArrowDown));
	assert_eq!(
		app.interaction
			.overlays
			.quick_open
			.file()
			.unwrap()
			.name
			.as_ref(),
		"text-b.md"
	);
	settle(&mut app, &events, |app| {
		!app.interaction.overlays.quick_open.preview.pending
	});
	let copied = app.clipboard.read().unwrap();
	assert!(copied.ends_with("Last paragraph."));
}

#[test]
#[ignore = "requires a desktop clipboard"]
fn deferred_copy_yields_to_newer_desktop_clipboard_text() {
	let _clipboard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
	let dir = TestDir::new();
	let (mut app, events) = deferred_copy_fixture(&dir, |app| {
		app.clipboard.write("sentinel".into()).unwrap();
	});
	app.interaction
		.overlays
		.quick_open
		.preview
		.worker
		.wait_for_final_publication();
	app.quick_open_key(&Key::Named(NamedKey::Escape));
	let mut other = markview_platform::Clipboard::default();
	other.write("newer copy".into()).unwrap();
	assert_eq!(app.clipboard.read().unwrap(), "newer copy");
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.detached_copy
			.is_none()
	});
	assert_eq!(other.read().unwrap(), "newer copy");
}

#[test]
#[ignore = "requires a desktop clipboard"]
fn deferred_copy_yields_to_newer_desktop_clipboard_image() {
	let _clipboard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
	let dir = TestDir::new();
	let mut other = ExternalClipboard::new().unwrap();
	let (mut app, events) =
		deferred_copy_fixture(&dir, |_| other.set_pixel(20).unwrap());
	app.quick_open_key(&Key::Named(NamedKey::Escape));
	other.set_pixel(80).unwrap();
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.detached_copy
			.is_none()
	});
	assert_eq!(other.pixel().unwrap(), [80, 0, 0, 255]);
}

#[test]
#[ignore = "requires a desktop clipboard"]
fn deferred_copy_yields_to_newer_link_with_the_same_label() {
	let _clipboard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
	let dir = TestDir::new();
	let mut other = ExternalClipboard::new().unwrap();
	let first = r#"<a href="/a">Read more</a>"#;
	let second = r#"<a href="/b">Read more</a>"#;
	let label = "Read more";
	let (mut app, events) = deferred_copy_fixture(&dir, |_| {
		other.set_html(first, Some(label)).unwrap();
		assert!(other.html().unwrap().contains(first));
		assert_eq!(other.text().unwrap(), label);
	});
	app.quick_open_key(&Key::Named(NamedKey::Escape));
	other.set_html(second, Some(label)).unwrap();
	assert_eq!(other.text().unwrap(), label);
	assert!(other.html().unwrap().contains(second));
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.detached_copy
			.is_none()
	});
	assert!(other.html().unwrap().contains(second));
}
