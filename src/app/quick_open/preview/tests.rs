use super::*;
use crate::{
	app::Event,
	cli::{LaunchOptions, Mode},
	test_support::TestDir,
};
use markview_layout::background::{Executor, Task};
use std::{
	fs,
	sync::{Arc, mpsc},
	time::{Duration, Instant},
};

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
			.snapshot
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
				.snapshot
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
			&& preview.snapshot.is_none()
	});
	fs::write(&path, "# Recovered valid Markdown").unwrap();
	settle(&mut app, &events, |app| {
		app.interaction
			.overlays
			.quick_open
			.preview
			.snapshot
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
			.snapshot
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
		.snapshot
		.as_ref()
		.unwrap()
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
			.snapshot
			.is_some()
	});
	assert!(text(&app).contains("Beta"));
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.snapshot
			.as_ref()
			.unwrap()
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
			.snapshot
			.as_ref()
			.unwrap()
			.width,
		body.w
	);
	query(&mut app, &events, "no-match");
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.snapshot
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
			.snapshot
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
fn empty_invalid_and_unsupported_files_have_stable_preview_states() {
	let dir = TestDir::new();
	fs::write(dir.path().join("empty.md"), "").unwrap();
	fs::write(dir.path().join("invalid.md"), [0xff]).unwrap();
	fs::write(dir.path().join("picture.png"), "not an image").unwrap();
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
			.snapshot
			.is_some()
	});
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.snapshot
			.as_ref()
			.unwrap()
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
			.snapshot
			.is_none()
	);
	query(&mut app, &events, "picture");
	assert!(!app.interaction.overlays.quick_open.preview.pending);
	assert!(
		app.interaction
			.overlays
			.quick_open
			.preview
			.message(crate::lang::Lang::En)
			.is_some()
	);
}

#[test]
fn picker_preview_uses_its_column_with_all_hover_previews_off() {
	use crate::settings::{PreviewKind, PreviewSize};
	let dir = TestDir::new();
	fs::write(
		dir.path().join("target.md"),
		"# Picker content\n\nLong preview. ".repeat(200),
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
			.snapshot
			.is_some()
	});
	let body = app.quick_open_geometry().body.unwrap();
	let preview = &app.interaction.overlays.quick_open.preview;
	assert_eq!(preview.request.as_ref().unwrap().options.width, body.w);
	assert_eq!(
		preview.request.as_ref().unwrap().options.viewport_height,
		body.h
	);
	assert_eq!(preview.snapshot.as_ref().unwrap().width, body.w);
	assert!(text(&app).contains("Picker content"));
	assert!(app.readers.session.path.is_none());
}
