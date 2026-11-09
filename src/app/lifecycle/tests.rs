use super::*;
use crate::{app::SendEvent, cli::LaunchOptions};
use std::{cell::Cell, sync::mpsc};

#[derive(Clone)]
struct Proxy(mpsc::Sender<Event>);
impl SendEvent for Proxy {
	fn try_send(&self, event: Event) -> bool {
		self.0.send(event).is_ok()
	}
}

#[derive(Default)]
struct TestLoop(Cell<usize>);
impl Loop for TestLoop {
	fn exit(&self) {
		self.0.set(self.0.get() + 1);
	}
}

fn reader() -> (App<Proxy>, mpsc::Receiver<Event>) {
	let (sender, events) = mpsc::channel();
	let app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			offline: true,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy(sender),
	);
	(app, events)
}

#[test]
fn close_requests_keep_only_macos_readers_running() {
	for mode in [Mode::Smoke, Mode::Window] {
		let (mut app, _) = reader();
		app.args.mode = mode;
		let event_loop = TestLoop::default();
		app.handle_window_event(
			&event_loop,
			WindowId::dummy(),
			WindowEvent::CloseRequested,
		);
		let keeps_running = markview_platform::close_keeps_app_running()
			&& app.args.mode == Mode::Window;
		assert_eq!(app.frame.closed, keeps_running);
		assert_eq!(event_loop.0.get(), usize::from(!keeps_running));
	}
}

#[test]
fn closing_a_window_discards_tabs_and_stale_work_but_keeps_file_activation() {
	if !markview_platform::close_keeps_app_running() {
		return;
	}
	use crate::app::single_instance::{Start, start};
	for caption in [false, true] {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("document.md");
		std::fs::write(&path, "# Reading\n\nA paragraph.").unwrap();
		let path = path.canonicalize().unwrap();
		let lock = dir.path().join("instance.lock");
		let (mut app, events) = reader();
		let Start::Primary(primary) = start(&lock, true, None).unwrap() else {
			panic!()
		};
		let proxy = app.proxy.clone();
		app.instance = Some(primary.listen(move |event| proxy.try_send(event)));
		app.instance_path = Some(lock.clone());
		app.persistence.initialized = true;
		app.persistence.path = Some(dir.path().join("session.json"));
		app.preferences.values.restore_session = true;
		app.preferences.values.single_instance = true;
		app.preferences.changed(None);
		app.args.mode = Mode::Window;
		app.open(path.clone());
		let version = app.readers.session.version;
		app.open_folder(dir.path().to_owned(), false);
		let pending = loop {
			let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
			if matches!(&event, Event::FolderLoaded { .. }) {
				break event;
			}
		};
		app.reflow_at = Some(Instant::now());
		app.retry_at = Some(Instant::now());
		app.prewarm_at = Some(Instant::now());
		app.animations.set_active(true);
		let event_loop = TestLoop::default();
		if caption {
			app.window_action(&event_loop, super::super::frame::Caption::Close);
		} else {
			app.close_window(&event_loop);
		}
		assert_eq!(event_loop.0.get(), 0);
		assert!(app.frame.closed);
		assert!(!app.animating());
		assert!(app.readers.entries().is_empty());
		assert!(app.renderer.is_none());
		assert!(app.watch.is_none());
		assert!(app.folder.root().is_none());
		app.handle_user_event(&event_loop, pending);
		assert!(
			app.folder.root().is_none(),
			"a closed folder scan must not return"
		);
		assert_eq!(app.tick(&event_loop, Instant::now()), None);
		let saved: serde_json::Value = serde_json::from_slice(
			&std::fs::read(app.persistence.path.as_ref().unwrap()).unwrap(),
		)
		.unwrap();
		assert_eq!(saved["tabs"], serde_json::json!([]));
		assert!(saved["folder"].is_null());
		assert!(matches!(
			start(&lock, true, Some(path.clone())).unwrap(),
			Start::Forwarded(_)
		));
		let activation = loop {
			let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
			if matches!(&event, Event::Activate(_)) {
				break event;
			}
		};
		app.handle_user_event(&event_loop, activation);
		assert_eq!(app.readers.session.path.as_ref(), Some(&path));
		assert!(app.readers.session.version > version);
	}
}
