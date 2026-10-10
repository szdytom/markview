use super::*;
use crate::{
	app::{
		SendEvent,
		single_instance::{self, Start},
	},
	cli::{LaunchOptions, Mode},
	layout::{LayoutEngine, LayoutOptions},
	state::Command,
	worker::ReaderSnapshot,
};
#[derive(Clone)]
struct Proxy;
impl SendEvent for Proxy {
	fn try_send(&self, _: super::super::Event) -> bool {
		true
	}
}
fn app(dir: &Path) -> App<Proxy> {
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy,
	);
	app.persistence.path = Some(dir.join("session.json"));
	app.instance_path = Some(dir.join("instance.lock"));
	app
}
fn primary(app: &mut App<Proxy>) {
	app.preferences.values.restore_session = true;
	app.preferences.values.single_instance = true;
	app.register_instance();
	assert!(app.instance.is_some());
	app.restore_session();
}
fn document(dir: &Path, name: &str) -> PathBuf {
	let path = dir.join(name);
	fs::write(&path, "Reading paragraph.").unwrap();
	fs::canonicalize(path).unwrap()
}

#[test]
fn roundtrip_tabs_order_active_dedup_close_empty_and_invalid_files() {
	let dir = tempfile::tempdir().unwrap();
	let a = document(dir.path(), "a.md");
	let b = document(dir.path(), "b.md");
	let gone = document(dir.path(), "gone.md");
	let path = dir.path().join("session.json");
	{
		let mut app = app(dir.path());
		primary(&mut app);
		app.readers.open(a.clone(), Instant::now());
		app.readers.session.scrolling.offset = 123.0;
		app.readers.open(b.clone(), Instant::now());
		app.readers.session.scrolling.offset = 456.0;
		app.readers.open(gone.clone(), Instant::now());
		app.readers.move_tab(0, 1);
		app.readers.select(0, Instant::now());
		app.flush_session();
	}
	fs::remove_file(gone).unwrap();
	let mut restored = app(dir.path());
	primary(&mut restored);
	assert_eq!(restored.readers.entries().len(), 2);
	assert_eq!(restored.readers.entries()[0].path, b);
	assert_eq!(restored.readers.session.path.as_ref(), Some(&b));
	assert!(restored.readers.entries()[1].session.document.is_none());
	assert_eq!(
		restored
			.readers
			.session
			.saved_reading
			.as_ref()
			.unwrap()
			.position
			.fallback,
		456.0
	);
	assert_eq!(
		restored.readers.entries()[1]
			.session
			.saved_reading
			.as_ref()
			.unwrap()
			.position
			.fallback,
		123.0
	);
	restored.flush_session();
	assert_eq!(
		Session::read(&path).unwrap().tabs[1]
			.reading
			.position
			.fallback,
		123.0
	);
	restored.open(a.clone());
	assert_eq!(restored.readers.entries().len(), 2);
	assert_eq!(restored.readers.session.path.as_ref(), Some(&a));
	restored.close_tab(1);
	restored.flush_session();
	assert_eq!(Session::read(&path).unwrap().tabs.len(), 1);
	restored.close_tab(0);
	restored.flush_session();
	assert!(Session::read(&path).unwrap().tabs.is_empty());
}

#[test]
fn active_missing_selects_closest_and_clipboard_tabs_are_excluded() {
	let dir = tempfile::tempdir().unwrap();
	let mut tabs = Tabs::default();
	for name in ["a.md", "gone.md", "b.md"] {
		tabs.open(document(dir.path(), name), Instant::now());
	}
	tabs.select(1, Instant::now());
	let saved = tabs.capture_session(Path::new("/excluded"));
	fs::remove_file(dir.path().join("gone.md")).unwrap();
	let mut restored = Tabs::default();
	restored.restore_session(saved);
	assert_eq!(restored.active(), 0);
	let paste = tempfile::tempdir().unwrap();
	restored.open(document(paste.path(), "clipboard.md"), Instant::now());
	let saved = restored.capture_session(paste.path());
	assert_eq!(saved.tabs.len(), 2);
	assert_eq!(saved.active, 1);
}

#[test]
fn lock_ownership_setting_linkage_debounce_and_shutdown_save() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("session.json");
	let mut owner = app(dir.path());
	primary(&mut owner);
	owner
		.readers
		.open(document(dir.path(), "a.md"), Instant::now());
	let now = Instant::now();
	owner.session_tick(now);
	assert!(!path.exists());
	owner.session_tick(now + Duration::from_secs(1));
	let original = fs::read(&path).unwrap();
	let mut independent = app(dir.path());
	independent.action(Command::RestoreSession);
	independent.restore_session();
	assert!(independent.preferences.values.single_instance);
	assert!(independent.instance.is_none());
	assert!(independent.readers.entries().is_empty());
	independent.flush_session();
	independent.action(Command::SingleInstance);
	assert!(!independent.preferences.values.restore_session);
	assert_eq!(fs::read(&path).unwrap(), original);
	owner.action(Command::RestoreSession);
	assert!(!owner.preferences.values.restore_session);
	assert!(owner.preferences.values.single_instance);
	assert!(!path.exists());
	owner.action(Command::RestoreSession);
	owner.action(Command::SingleInstance);
	assert!(!owner.preferences.values.restore_session);
	owner.action(Command::RestoreSession);
	owner
		.readers
		.open(document(dir.path(), "b.md"), Instant::now());
	drop(owner);
	let Start::Primary(_primary) =
		single_instance::start(&dir.path().join("instance.lock"), false, None)
			.unwrap()
	else {
		panic!()
	};
	assert_eq!(Session::read(&path).unwrap().tabs.len(), 2);
}

#[test]
fn damaged_and_future_sessions_are_ignored_and_reset_clears_session() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("session.json");
	for bytes in ["broken", r#"{"version":2,"active":0,"tabs":[]}"#] {
		fs::write(&path, bytes).unwrap();
		let mut app = app(dir.path());
		primary(&mut app);
		assert!(app.readers.entries().is_empty());
		app.action(Command::Reset);
		assert!(!app.preferences.values.restore_session);
		assert!(!path.exists());
	}
}

fn accept(
	session: &mut ReaderSession,
	source: &str,
	options: &LayoutOptions,
	complete: bool,
) {
	let doc = Arc::new(markview_core::document::parse(source));
	let mut layout = LayoutEngine::new().layout(&doc, options);
	if !complete {
		layout.blocks.truncate(2);
		layout.height = layout.blocks.last().map_or(0.0, |b| b.y + b.height());
	}
	session.accept(
		ReaderSnapshot {
			blocked_images: Default::default(),
			document: doc,
			layout,
			content_version: 1,
			complete,
			parse_complete: true,
			remote_deferred: 0,
		},
		200.0,
		None,
	);
}
use std::sync::Arc;
#[test]
fn queued_fragments_override_restoration_during_progressive_layout() {
	let dir = tempfile::tempdir().unwrap();
	let path = document(dir.path(), "reader.md");
	let source = format!("# Top\n\n{}", "Reading paragraph.\n\n".repeat(100));
	let options = crate::test_support::options();
	let mut original = ReaderSession::default();
	accept(&mut original, &source, &options, true);
	original.scrolling.offset = original.snapshot.blocks[80].y;
	let saved = Reading::capture(&original);
	for background in [false, true] {
		let mut app = app(dir.path());
		if background {
			app.readers
				.open(document(dir.path(), "other.md"), Instant::now());
		}
		let index = app.readers.restore_tab(path.clone(), saved.clone());
		app.readers.queue_anchor(index, None);
		let session = if background {
			&app.readers.entries()[index].session
		} else {
			&app.readers.session
		};
		assert!(session.saved_reading.is_some());
		app.readers.queue_anchor(index, Some("top".into()));
		if background {
			app.readers.select(index, Instant::now());
		}
		accept(&mut app.readers.session, &source, &options, false);
		app.apply_anchor();
		assert!(app.readers.session.pending_anchor.is_none());
		assert!(app.readers.session.scrolling.target.is_none());
		assert_eq!(app.readers.session.scrolling.offset, 0.0);
		accept(&mut app.readers.session, &source, &options, true);
		assert!(app.readers.session.saved_reading.is_none());
		assert_eq!(app.readers.session.scrolling.offset, 0.0);
	}
}

#[test]
fn text_position_survives_width_font_changes_progressive_layout_and_release() {
	let source = format!(
		"{}\n\n{}",
		"Opening.\n\n".repeat(10),
		"A long reading paragraph with many words. ".repeat(150)
	);
	let mut options = crate::test_support::options();
	options.width = 700.0;
	let mut original = ReaderSession::default();
	accept(&mut original, &source, &options, true);
	original.scrolling.offset =
		original.snapshot.blocks.last().unwrap().y + 300.0;
	let saved = Reading::capture(&original);
	for released in [false, true] {
		let mut restored = if released {
			original.clone()
		} else {
			ReaderSession::default()
		};
		if released {
			restored.release_heavy();
		} else {
			restored.saved_reading = Some(saved.clone());
		}
		options.width = 350.0;
		options.font_size = 24.0;
		accept(&mut restored, &source, &options, false);
		assert!(restored.saved_reading.is_some());
		accept(&mut restored, &source, &options, true);
		assert!(restored.saved_reading.is_none());
		let target = saved.position.resolve(&restored.snapshot).unwrap();
		assert!((restored.scrolling.offset - target).abs() < 0.01);
		assert!(restored.scrolling.offset > original.scrolling.offset);
	}
	let mut changed = ReaderSession {
		saved_reading: Some(saved),
		..Default::default()
	};
	accept(&mut changed, "Changed short content.", &options, true);
	assert!(changed.saved_reading.is_none());
	assert!(
		changed.scrolling.offset
			<= crate::state::scroll_limit(changed.snapshot.height, 200.0)
	);
}

#[test]
fn disclosures_apply_before_layout_and_changed_content_discards_overrides() {
	use crate::{
		app::{Event, window::Loop},
		document::BlockKind,
	};
	struct StubLoop;
	impl Loop for StubLoop {
		fn exit(&self) {}
	}
	let source = "<details>\n<summary>Summary</summary>\n\nHidden paragraph.\n\n</details>\n\nFollowing paragraph.";
	let parsed = Arc::new(markview_core::document::parse(source));
	let id = parsed
		.blocks
		.iter()
		.find(|b| matches!(b.kind, BlockKind::Details { .. }))
		.unwrap()
		.id;
	let mut original = ReaderSession {
		accepted_content_id: parsed.content_id,
		details_open: Arc::new(BTreeMap::from([(id, true)])),
		..Default::default()
	};
	let mut options = crate::test_support::options();
	options.details_open = original.details_open.clone();
	accept(&mut original, source, &options, true);
	let saved = Reading::capture(&original);
	let dir = tempfile::tempdir().unwrap();
	let path = document(dir.path(), "details.md");
	fs::write(&path, source).unwrap();
	let mut tabs = Tabs::default();
	tabs.restore_session(Session {
		version: 1,
		active: 0,
		tabs: vec![SavedTab {
			origin: legacy_origin(),
			path: path.clone(),
			reading: saved,
		}],
	});
	assert_eq!(tabs.session.details_open.get(&id), Some(&true));
	let mut restored_options = crate::test_support::options();
	restored_options.details_open = tabs.session.details_open.clone();
	accept(&mut tabs.session, source, &restored_options, true);
	assert!(
		tabs.session
			.snapshot
			.blocks
			.iter()
			.flat_map(|b| &b.layout.text)
			.any(|n| n.text.contains("Hidden paragraph"))
	);
	assert!(!tabs.session.load_all_images);
	let mut app = app(dir.path());
	app.readers = tabs;
	app.readers.session.release_heavy();
	app.handle_user_event(
		&StubLoop,
		Event::Parsed {
			path,
			content_version: app.readers.session.content_version,
			document: Arc::new(markview_core::document::parse(
				"Changed content.",
			)),
		},
	);
	assert!(app.readers.session.details_open.is_empty());
	assert!(
		app.readers
			.session
			.saved_reading
			.as_ref()
			.unwrap()
			.details
			.is_empty()
	);
}

#[test]
fn repeated_blocks_restore_the_correct_occurrence() {
	let source =
		"Same paragraph.\n\nSame paragraph.\n\nSame paragraph.\n\nFollowing.";
	let options = crate::test_support::options();
	let mut session = ReaderSession::default();
	accept(&mut session, source, &options, true);
	session.scrolling.offset = session.snapshot.blocks[2].y;
	let position =
		Position::capture(&session.snapshot, session.scrolling.offset);
	assert_eq!(position.occurrence, 2);
	assert_eq!(
		position.resolve(&session.snapshot),
		Some(session.scrolling.offset)
	);
}

#[test]
fn pending_positions_keep_current_disclosure_choices() {
	let mut session = ReaderSession {
		saved_reading: Some(Reading::default()),
		..Default::default()
	};
	session.details_open = Arc::new(BTreeMap::from([(42, true)]));
	assert_eq!(Reading::capture(&session).details.get(&42), Some(&true));
}

#[test]
fn external_open_before_window_creation_restores_then_selects_the_requested_tab()
 {
	let dir = tempfile::tempdir().unwrap();
	let a = document(dir.path(), "a.md");
	let b = document(dir.path(), "b.md");
	Session {
		version: 1,
		active: 0,
		tabs: vec![
			SavedTab {
				origin: legacy_origin(),
				path: a,
				reading: Reading::default(),
			},
			SavedTab {
				origin: legacy_origin(),
				path: b.clone(),
				reading: Reading::default(),
			},
		],
	}
	.write(&dir.path().join("session.json"))
	.unwrap();
	let mut app = app(dir.path());
	app.preferences.values.restore_session = true;
	app.preferences.values.single_instance = true;
	app.register_instance();
	app.open(b.clone());
	assert_eq!(app.readers.entries().len(), 2);
	assert_eq!(app.readers.session.path.as_ref(), Some(&b));
	assert_eq!(app.readers.active(), 1);
}

#[cfg(unix)]
#[test]
fn clipboard_exclusion_uses_the_canonical_temporary_directory() {
	let dir = tempfile::tempdir().unwrap();
	let paste = tempfile::tempdir().unwrap();
	let link = dir.path().join("paste");
	std::os::unix::fs::symlink(paste.path(), &link).unwrap();
	let mut tabs = Tabs::default();
	tabs.open(document(paste.path(), "clipboard.md"), Instant::now());
	assert!(tabs.capture_session(&link).tabs.is_empty());
}

#[test]
fn block_relative_and_text_positions_roundtrip_without_shifting() {
	let source = format!(
		"```\ncode\n```\n\n{}",
		"Following paragraph.\n\n".repeat(100)
	);
	let options = crate::test_support::options();
	let mut session = ReaderSession::default();
	accept(&mut session, &source, &options, true);
	let block = &session.snapshot.blocks[0];
	let first = &block.layout.text[0].clusters[0];
	let last_bottom = block
		.layout
		.text
		.iter()
		.flat_map(|n| &n.clusters)
		.map(|c| c.rect.y + c.rect.h)
		.fold(0.0, f32::max);
	let below_text =
		(block.y + last_bottom + session.snapshot.blocks[1].y) * 0.5;
	assert!(below_text > block.y + last_bottom);
	for (scroll, text_anchor) in
		[(block.y + first.rect.y, true), (below_text, false)]
	{
		session.scrolling.offset = scroll;
		let saved = Reading::capture(&session);
		assert_eq!(saved.position.node.is_some(), text_anchor);
		let saved: Reading =
			serde_json::from_slice(&serde_json::to_vec(&saved).unwrap())
				.unwrap();
		let mut restored = ReaderSession {
			saved_reading: Some(saved),
			..Default::default()
		};
		accept(&mut restored, &source, &options, true);
		assert!((restored.scrolling.offset - scroll).abs() < 0.01);
	}
}

#[test]
fn restoration_keeps_modes_for_the_same_path_and_discards_resource_grants() {
	use crate::security::{Origin, Resource, Security, Trust};
	let dir = tempfile::tempdir().unwrap();
	let path = document(dir.path(), "local.md");
	let image = dir.path().join("image.png");
	let mut tabs = Tabs::default();
	tabs.open(path.clone(), Instant::now());
	tabs.open(path.clone(), Instant::now());
	tabs.session.security = Security::local(Trust::Untrusted);
	tabs.session.security.grant(Resource::File(image.clone()));
	let saved = tabs.capture_session(&dir.path().join("paste"));
	let session_path = dir.path().join("session.json");
	saved.write(&session_path).unwrap();
	let encoded = fs::read_to_string(&session_path).unwrap();
	assert!(!encoded.contains("image.png"));
	let mut restored = Tabs::default();
	restored.restore_session(Session::read(&session_path).unwrap());
	assert_eq!(restored.entries().len(), 2);
	assert_eq!(
		restored.session.security.origin,
		Origin::Local(Trust::Untrusted)
	);
	assert!(restored.session.security.check_file(&image).is_err());
	restored.select(0, Instant::now());
	assert_eq!(
		restored.session.security.origin,
		Origin::Local(Trust::Trusted)
	);
}
