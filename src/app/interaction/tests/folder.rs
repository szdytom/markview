use super::*;
use crate::app::folder::Tree;
use crate::security::{Origin, Trust};
use crate::state::{PanelPage, PanelTab};

#[test]
fn picked_paths_open_documents_or_folder_trees_and_cancel_releases_the_dialog()
{
	use std::sync::mpsc;
	#[derive(Clone)]
	struct Proxy(mpsc::Sender<Event>);
	impl SendEvent for Proxy {
		fn try_send(&self, event: Event) -> bool {
			self.0.send(event).is_ok()
		}
	}
	let dir = crate::test_support::TestDir::new();
	let file = dir.path().join("standalone.md");
	std::fs::write(&file, SOURCE).unwrap();
	let (send, receive) = mpsc::channel();
	let mut app = App::new(
		crate::cli::LaunchOptions {
			mode: crate::cli::Mode::Smoke,
			offline: true,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy(send),
	);
	app.persistence.initialized = true;
	app.dialog_open = true;
	app.handle_user_event(&StubLoop, Event::Picked(None));
	assert!(!app.dialog_open);
	assert!(app.readers.entries().is_empty());
	app.dialog_open = true;
	app.handle_user_event(&StubLoop, Event::Picked(Some(file.clone())));
	assert!(!app.dialog_open);
	assert_eq!(app.readers.session.path.as_ref(), Some(&file));
	assert!(app.folder.root().is_none());
	for name in ["first", "second"] {
		let root = dir.path().join(name);
		std::fs::create_dir(&root).unwrap();
		let readme = root.join("README.md");
		std::fs::write(&readme, SOURCE).unwrap();
		app.dialog_open = true;
		app.handle_user_event(&StubLoop, Event::Picked(Some(root.clone())));
		assert!(!app.dialog_open);
		loop {
			let event = receive.recv_timeout(Duration::from_secs(5)).unwrap();
			if matches!(&event, Event::FolderLoaded { .. }) {
				app.handle_user_event(&StubLoop, event);
				break;
			}
		}
		assert_eq!(app.folder.root(), Some(&root));
		assert_eq!(app.readers.session.path.as_ref(), Some(&readme));
		assert!(app.folder.open || app.phone_folder_open());
		app.dialog_open = true;
		app.handle_user_event(&StubLoop, Event::Picked(None));
		assert!(!app.dialog_open);
		assert_eq!(app.folder.root(), Some(&root));
		assert_eq!(app.readers.session.path.as_ref(), Some(&readme));
	}
}

fn middle_click(app: &mut App<StubProxy>, at: (f32, f32)) {
	app.gestures.allow_mouse();
	app.handle_window_event(
		&StubLoop,
		WindowId::from_raw(0),
		WindowEvent::CursorMoved {
			device_id: DeviceId::from_raw(0),
			position: PhysicalPosition::new(f64::from(at.0), f64::from(at.1)),
		},
	);
	for state in [ElementState::Pressed, ElementState::Released] {
		app.handle_window_event(
			&StubLoop,
			WindowId::from_raw(0),
			WindowEvent::MouseInput {
				device_id: DeviceId::from_raw(0),
				button: MouseButton::Middle,
				state,
			},
		);
	}
}

fn point_on(app: &mut App<StubProxy>, command: ChromeCommand) -> (f32, f32) {
	let button = app
		.buttons()
		.into_iter()
		.find(|button| button.action == Command::Chrome(command))
		.unwrap();
	let rect = button.widget().hit_rect().unwrap();
	(rect.x + rect.w / 2., rect.y + rect.h / 2.)
}

#[test]
fn folder_middle_click_opens_background_tabs_and_preserves_the_current_page() {
	let dir = crate::test_support::TestDir::new();
	let root = dir.path().to_owned();
	let current = root.join("a.md");
	let note = root.join("b.md");
	let image = root.join("image.png");
	for path in [&current, &note, &image] {
		std::fs::write(path, SOURCE).unwrap();
	}
	for phone in [false, true] {
		// Cover a fresh blank page, an active document and a blank page with
		// parked tabs.
		for page in 0..3 {
			let (mut app, _) = reader(SOURCE, 760.);
			app.tab_strip.phone = phone;
			if page > 0 {
				app.readers.open(current.clone(), Instant::now());
			}
			if page == 2 {
				app.readers.new_page(Instant::now());
			}
			app.folder.tree = Some(Tree::read(root.clone()).unwrap());
			app.set_folder_open(true);
			app.advance_drawers(Instant::now() + Duration::from_secs(1));
			app.scroll_folder(14.);
			let active = app.readers.active();
			let path = app.readers.session.path.clone();
			let document = app.readers.session.document.clone();
			let revision = app.readers.session.content_version;
			let version = app.readers.session.version;
			app.readers.session.scrolling.offset = 31.;
			let scroll = app.folder.scroll.offset();
			let count = app.readers.entries().len();
			for target in [&note, &image] {
				let id = app
					.folder
					.tree
					.as_ref()
					.unwrap()
					.entries
					.iter()
					.find(|entry| &entry.path == target)
					.unwrap()
					.id;
				let at = point_on(&mut app, ChromeCommand::FolderEntry(id));
				middle_click(&mut app, at);
				middle_click(&mut app, at);
			}
			if page > 0 {
				let at = point_on(&mut app, ChromeCommand::FolderEntry(0));
				middle_click(&mut app, at);
			}
			assert_eq!(app.readers.entries().len(), count + 2);
			assert_eq!(app.readers.active(), active);
			assert_eq!(app.readers.session.path, path);
			assert_eq!(app.readers.session.content_version, revision);
			assert_eq!(app.readers.session.version, version);
			assert_eq!(app.readers.session.scrolling.offset, 31.);
			if let Some(document) = document {
				assert!(Arc::ptr_eq(
					app.readers.session.document.as_ref().unwrap(),
					&document,
				));
			}
			assert_eq!(app.folder.scroll.offset(), scroll);
			assert_eq!(app.phone_folder_open(), phone);
			assert_eq!(app.folder.open, !phone);
			let index = app.readers.find(&note).unwrap();
			let session = &app.readers.entries()[index].session;
			assert_eq!(session.security.origin, Origin::Local(Trust::Trusted));
			assert!(session.document.is_none());
			app.select_tab(index);
			assert_eq!(app.readers.session.path, Some(note.clone()));
		}
	}
}

#[test]
fn folder_middle_click_ignores_directories_controls_and_covered_rows() {
	use crate::app::modal::{Message, Modal};
	let dir = crate::test_support::TestDir::new();
	std::fs::create_dir(dir.path().join("notes")).unwrap();
	std::fs::write(dir.path().join("note.md"), SOURCE).unwrap();
	let (mut app, _) = reader(SOURCE, 760.);
	app.readers
		.open(dir.path().join("current.md"), Instant::now());
	app.folder.tree = Some(Tree::read(dir.path().into()).unwrap());
	app.set_folder_open(true);
	app.advance_drawers(Instant::now() + Duration::from_secs(1));
	for command in [
		ChromeCommand::FolderEntry(0),
		ChromeCommand::FolderShowExcluded,
		ChromeCommand::FolderCollapseAll,
	] {
		let at = point_on(&mut app, command);
		middle_click(&mut app, at);
	}
	assert!(app.folder.tree.as_ref().unwrap().collapsed(0));
	assert!(!app.folder.tree.as_ref().unwrap().show_excluded);
	let surface = app.folder_surface();
	middle_click(&mut app, (surface.x + 20., surface.y + surface.h - 20.));
	let at = point_on(&mut app, ChromeCommand::FolderEntry(1));
	app.interaction
		.overlays
		.modal
		.send(Message::Open(Modal::OpenLocal {
			path: "other.bin".into(),
			dir: dir.path().into(),
			document_dir: None,
		}));
	middle_click(&mut app, at);
	app.interaction.overlays.modal.send(Message::Dismiss);
	app.show_panel(PanelPage::Settings(PanelTab::Generic));
	middle_click(&mut app, at);
	assert_eq!(app.readers.entries().len(), 1);
}
