use super::*;
use crate::state::SettingsCommand;
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
fn new_window_starts_blank_even_with_a_saved_session() {
	let dir = tempfile::tempdir().unwrap();
	let path = document(dir.path(), "saved.md");
	{
		let mut saved = app(dir.path());
		primary(&mut saved);
		saved.readers.open(path, Instant::now());
		saved.flush_session();
	}
	let mut fresh = app(dir.path());
	fresh.args.new_window = true;
	primary(&mut fresh);
	assert!(fresh.owns_session());
	assert!(fresh.readers.entries().is_empty());
	assert!(fresh.readers.session.path.is_none());
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
fn missing_active_falls_back_to_recent_and_clipboard_tabs_are_excluded() {
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
	assert_eq!(restored.active(), 1);
	assert_eq!(restored.session.path, Some(document(dir.path(), "b.md")));
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
	independent.action(Command::Settings(SettingsCommand::RestoreSession));
	independent.restore_session();
	assert!(independent.preferences.values.single_instance);
	assert!(!independent.owns_session());
	assert!(independent.readers.entries().is_empty());
	independent.flush_session();
	independent.action(Command::Settings(SettingsCommand::SingleInstance));
	assert!(!independent.preferences.values.restore_session);
	assert_eq!(fs::read(&path).unwrap(), original);
	owner.action(Command::Settings(SettingsCommand::RestoreSession));
	assert!(!owner.preferences.values.restore_session);
	assert!(owner.preferences.values.single_instance);
	assert!(!path.exists());
	owner.action(Command::Settings(SettingsCommand::RestoreSession));
	owner.action(Command::Settings(SettingsCommand::SingleInstance));
	assert!(!owner.preferences.values.restore_session);
	owner.action(Command::Settings(SettingsCommand::RestoreSession));
	owner
		.readers
		.open(document(dir.path(), "b.md"), Instant::now());
	drop(independent);
	drop(owner);
	let Start::Primary(_primary) = single_instance::start(
		&dir.path().join("instance.lock"),
		false,
		None,
		false,
	)
	.unwrap() else {
		panic!()
	};
	assert_eq!(Session::read(&path).unwrap().tabs.len(), 2);
}

#[test]
fn damaged_and_future_sessions_are_ignored_and_reset_clears_session() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("session.json");
	for bytes in ["broken", r#"{"version":3,"active":0,"tabs":[]}"#] {
		fs::write(&path, bytes).unwrap();
		let mut app = app(dir.path());
		primary(&mut app);
		assert!(app.readers.entries().is_empty());
		app.action(Command::Settings(SettingsCommand::Reset));
		assert!(!app.preferences.values.restore_session);
		assert!(!path.exists());
	}
}

#[test]
fn a_version_one_session_keeps_its_tab_and_scroll_fallback() {
	let dir = tempfile::tempdir().unwrap();
	let path = document(dir.path(), "legacy.md");
	let session_path = dir.path().join("session.json");
	fs::write(
		&session_path,
		serde_json::json!({
			"version": 1,
			"active": 0,
			"tabs": [{
				"path": path.clone(),
				"reading": {
					"content": 9,
					"position": {
						"content": 7,
						"occurrence": 2,
						"node": 5,
						"byte": 31,
						"local": 12.5,
						"fallback": 345.0
					},
					"details": {"42": true}
				},
				"search": {
					"open": true,
					"query": "needle",
					"case_sensitive": true,
					"whole_word": false
				}
			}],
			"recent": [0]
		})
		.to_string(),
	)
	.unwrap();
	let session = Session::read(&session_path).unwrap();
	assert_eq!(session.version, 4);
	assert_eq!(session.tabs.len(), 1);
	let tab = &session.tabs[0];
	// Version 1 stored source-derived identities that the semantic kernel no
	// longer defines: the migration drops those structure fields and the
	// collapse ids, and keeps the tab, its search and its scroll fallback.
	assert_eq!(tab.reading.position.fallback, 345.0);
	assert_eq!(tab.reading.position.input, 0);
	assert_eq!(tab.reading.position.frontend, 0);
	assert!(tab.reading.position.path.is_empty());
	assert_eq!(tab.reading.position.field, None);
	assert_eq!(tab.reading.position.byte, 0);
	assert_eq!(tab.reading.position.local, 0.0);
	assert!(tab.reading.details.0.is_empty());
	assert_eq!(tab.search.query, "needle");
	let mut tabs = Tabs::default();
	tabs.restore_session(session);
	assert_eq!(tabs.session.path.as_ref(), Some(&path));
	let restored = tabs.session.saved_reading.as_ref().unwrap();
	assert_eq!(restored.position.fallback, 345.0);
	assert_eq!(restored.position.field, None);
}

fn accept(
	session: &mut ReaderSession,
	source: &str,
	options: &LayoutOptions,
	complete: bool,
) {
	let doc = crate::loaded::LoadedDocument::markdown(Arc::new(
		markview_commonmark::parse(source),
	));
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
	original.scrolling.offset = original.display.snapshot.blocks[80].y;
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
		let top = app
			.readers
			.session
			.display
			.snapshot
			.anchor_y("top")
			.unwrap();
		app.apply_anchor();
		assert!(app.readers.session.pending_anchor.is_none());
		assert_eq!(app.readers.session.scrolling.target, Some(top));
		assert_eq!(app.readers.session.scrolling.offset, 0.0);
		accept(&mut app.readers.session, &source, &options, true);
		let viewport = app.viewport();
		app.readers
			.session
			.advance_scroll(Instant::now() + Duration::from_secs(1), viewport);
		assert!(app.readers.session.saved_reading.is_none());
		assert_eq!(app.readers.session.scrolling.offset, top);
		assert!(app.readers.session.scrolling.target.is_none());
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
		original.display.snapshot.blocks.last().unwrap().y + 300.0;
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
		let target = saved
			.position
			.resolve(&restored.display.snapshot, restored.document.as_deref())
			.unwrap();
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
			<= changed.display.snapshot.scroll_limit(200.0)
	);
}

#[test]
fn last_line_reading_position_survives_release_and_progressive_restoration() {
	let source = format!("{}Last line.", "Reading paragraph.\n\n".repeat(60));
	let options = crate::test_support::options();
	let mut original = ReaderSession::default();
	accept(&mut original, &source, &options, true);
	original.scroll_by(f32::INFINITY, 200.0);
	let end = original.display.snapshot.scroll_limit(200.0);
	assert_eq!(original.scrolling.offset, end);
	assert!(end > original.display.snapshot.height - 200.0);
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
		accept(&mut restored, &source, &options, false);
		assert!(restored.saved_reading.is_some());
		accept(&mut restored, &source, &options, true);
		assert!(restored.saved_reading.is_none());
		assert!((restored.scrolling.offset - end).abs() < 0.01);
		assert!(restored.scrolling.target.is_none());
	}
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
	let parsed = crate::loaded::LoadedDocument::markdown(Arc::new(
		markview_commonmark::parse(source),
	));
	let id = parsed
		.blocks()
		.iter()
		.find(|b| matches!(b.kind(), BlockKind::Details { .. }))
		.unwrap()
		.id();
	let mut original = ReaderSession {
		accepted_content_id: parsed.document().content_key().get(),
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
		folder: None,
		folder_open: false,
		version: 1,
		active: 0,
		tabs: vec![SavedTab {
			origin: legacy_origin(),
			path: path.clone(),
			reading: saved,
			search: SavedSearch::default(),
		}],
		recent: Vec::new(),
	});
	assert!(tabs.session.details_open.is_empty());
	let loaded = crate::loaded::LoadedDocument::markdown(Arc::new(
		markview_commonmark::parse(source),
	));
	assert!(tabs.session.restore_disclosures(&loaded));
	assert_eq!(tabs.session.details_open.get(&id), Some(&true));
	let mut restored_options = crate::test_support::options();
	restored_options.details_open = tabs.session.details_open.clone();
	accept(&mut tabs.session, source, &restored_options, true);
	assert!(
		tabs.session
			.display
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
			document: crate::loaded::LoadedDocument::markdown(Arc::new(
				markview_commonmark::parse("Changed content."),
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
	session.scrolling.offset = session.display.snapshot.blocks[2].y;
	let position = Position::capture(
		&session.display.snapshot,
		session.document.as_deref(),
		session.scrolling.offset,
	);
	// The structural path selects the third identical paragraph.
	let expected = session
		.document
		.as_deref()
		.unwrap()
		.path_of(session.display.snapshot.blocks[2].id)
		.unwrap();
	assert_eq!(
		position.path,
		expected.iter().map(|i| *i as u32).collect::<Vec<_>>()
	);
	assert_eq!(
		position
			.resolve(&session.display.snapshot, session.document.as_deref()),
		Some(session.scrolling.offset)
	);
}

#[test]
fn pending_positions_keep_current_disclosure_choices() {
	let mut session = ReaderSession {
		saved_reading: Some(Reading::default()),
		..Default::default()
	};
	session.details_open = Arc::new(BTreeMap::from([(
		markview_document::BlockId::new(42).unwrap(),
		true,
	)]));
	assert!(Reading::capture(&session).details.0.is_empty());
}

#[test]
fn external_open_before_window_creation_restores_then_selects_the_requested_tab()
 {
	let dir = tempfile::tempdir().unwrap();
	let a = document(dir.path(), "a.md");
	let b = document(dir.path(), "b.md");
	Session {
		folder: None,
		folder_open: false,
		version: 1,
		active: 0,
		tabs: vec![
			SavedTab {
				origin: legacy_origin(),
				path: a,
				reading: Reading::default(),
				search: SavedSearch::default(),
			},
			SavedTab {
				origin: legacy_origin(),
				path: b.clone(),
				reading: Reading::default(),
				search: SavedSearch::default(),
			},
		],
		recent: Vec::new(),
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

#[test]
fn clipboard_exclusion_uses_the_canonical_temporary_directory() {
	if !markview_platform::test_support::special_files() {
		return;
	}
	let dir = tempfile::tempdir().unwrap();
	let paste = tempfile::tempdir().unwrap();
	let link = dir.path().join("paste");
	markview_platform::test_support::symlink(paste.path(), &link).unwrap();
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
	let block = &session.display.snapshot.blocks[0];
	let first = &block.layout.text[0].clusters.at(0);
	let last_bottom = block
		.layout
		.text
		.iter()
		.flat_map(|n| &n.clusters)
		.map(|c| c.rect.y + c.rect.h)
		.fold(0.0, f32::max);
	let below_text =
		(block.y + last_bottom + session.display.snapshot.blocks[1].y) * 0.5;
	assert!(below_text > block.y + last_bottom);
	for (scroll, text_anchor) in
		[(block.y + first.rect.y, true), (below_text, false)]
	{
		session.scrolling.offset = scroll;
		let saved = Reading::capture(&session);
		assert_eq!(saved.position.field.is_some(), text_anchor);
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
fn image_and_caption_positions_survive_serialization_and_image_loading() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let source = format!(
		"# Heading\n\n![Markview](picture.svg)\n\n{}",
		"Following paragraph.\n\n".repeat(40)
	);
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		ImageSource::Resource(ResourceRef::document("picture.svg")),
		"picture".into(),
	);
	images.entries.insert(
		"picture".into(),
		ImageInfo {
			version: 1,
			size: Some((512, 512)),
			svg: true,
			..Default::default()
		},
	);
	let mut pending = images.clone();
	pending
		.entries
		.insert("picture".into(), ImageInfo::default());
	let mut metadata = images.clone();
	metadata.entries.get_mut("picture").unwrap().loading = true;
	let mut failed = pending.clone();
	failed.entries.get_mut("picture").unwrap().error =
		Some("Image unavailable".into());
	for width in [371.42856, 760.0] {
		let options = LayoutOptions {
			width,
			font_size: 18.,
			..crate::test_support::options()
		};
		let layout = |images: &ImageSnapshot| {
			let document = crate::loaded::LoadedDocument::markdown(Arc::new(
				markview_commonmark::parse(source.as_str()),
			));
			ReaderSnapshot {
				layout: LayoutEngine::new()
					.layout_with_images(&document, &options, images),
				document,
				blocked_images: Default::default(),
				content_version: 1,
				complete: true,
				parse_complete: true,
				remote_deferred: 0,
			}
		};
		let mut original = ReaderSession::default();
		original.accept(layout(&images), 200., None);
		let picture = &original.display.snapshot.blocks[1];
		let caption = &picture.layout.text[1].clusters.at(0);
		let caption_y =
			picture.y + picture.rect(caption.command, caption.rect).unwrap().y;
		for scroll in [
			picture.y + 1.,
			picture.y + picture.height() * 0.75,
			caption_y,
			original.display.snapshot.blocks[2].y + 1.,
		] {
			original.scrolling.offset = scroll;
			let saved: Reading = serde_json::from_slice(
				&serde_json::to_vec(&Reading::capture(&original)).unwrap(),
			)
			.unwrap();
			for (state, initial) in [
				("ready", &images),
				("metadata", &metadata),
				("pending", &pending),
				("failed", &failed),
			] {
				let mut restored = ReaderSession {
					saved_reading: Some(saved.clone()),
					..Default::default()
				};
				restored.accept(layout(initial), 200., None);
				if state == "metadata" {
					let mut reopened = ReaderSession {
						saved_reading: Some(Reading::capture(&restored)),
						..Default::default()
					};
					reopened.accept(layout(&images), 200., None);
					assert!(
						(reopened.scrolling.offset - scroll).abs() < 0.01,
						"reopening during metadata: {scroll} -> {}",
						reopened.scrolling.offset
					);
				}
				if state == "failed" {
					assert!(restored.saved_reading.is_none());
					assert!(restored.scrolling.offset.is_finite());
					continue;
				}
				if state == "pending"
					&& scroll < original.display.snapshot.blocks[2].y
				{
					assert!(restored.saved_reading.is_some());
					assert_eq!(Reading::capture(&restored), saved);
				}
				restored.accept(layout(&images), 200., None);
				assert!(restored.saved_reading.is_none());
				assert!(
					(restored.scrolling.offset - scroll).abs() < 0.01,
					"state={state}, width={width}, scroll={scroll}, restored={}",
					restored.scrolling.offset
				);
			}
		}
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

#[test]
fn pending_folder_restoration_survives_autosave_and_exit() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path().canonicalize().unwrap();
	let path = root.join("session.json");
	for open in [false, true] {
		Session {
			folder: Some(root.clone()),
			folder_open: open,
			version: 1,
			active: 0,
			tabs: vec![],
			recent: Vec::new(),
		}
		.write(&path)
		.unwrap();
		{
			let mut restored = app(&root);
			primary(&mut restored);
			assert!(restored.folder.tree.is_none());
			let now = Instant::now();
			restored.session_tick(now);
			restored.session_tick(now + Duration::from_secs(2));
			assert!(restored.persistence.deadline.is_none());
			let saved = Session::read(&path).unwrap();
			assert_eq!(saved.folder, Some(root.clone()));
			assert_eq!(saved.folder_open, open);
			restored.flush_session();
		}
		let mut restored = app(&root);
		primary(&mut restored);
		assert!(restored.folder.tree.is_none());
		restored.flush_session();
		let saved = Session::read(&path).unwrap();
		assert_eq!(saved.folder, Some(root.clone()));
		assert_eq!(saved.folder_open, open);
		restored.folder_loaded(
			1,
			false,
			super::super::folder::Tree::read(root.clone()),
		);
		restored.flush_session();
		assert_eq!(Session::read(&path).unwrap(), saved);
	}
}

#[test]
fn pending_folder_root_ignores_stale_results_and_clears_on_failure() {
	let dir = tempfile::tempdir().unwrap();
	let mut app = app(dir.path());
	primary(&mut app);
	let root = dir.path().join("missing");
	app.open_folder(dir.path().join("superseded"), false);
	app.open_folder(root.clone(), false);
	app.folder_loaded(1, false, Err(anyhow::anyhow!("stale")));
	app.flush_session();
	let path = dir.path().join("session.json");
	assert_eq!(Session::read(&path).unwrap().folder, Some(root));
	let now = Instant::now();
	app.session_tick(now);
	app.session_tick(now + Duration::from_secs(2));
	app.folder_loaded(2, false, Err(anyhow::anyhow!("missing")));
	app.session_tick(now + Duration::from_secs(3));
	app.session_tick(now + Duration::from_secs(5));
	assert_eq!(Session::read(&path).unwrap().folder, None);
}

#[test]
fn folder_root_is_saved_with_the_browsing_session() {
	let dir = tempfile::tempdir().unwrap();
	let root = dir.path().canonicalize().unwrap();
	let path = document(&root, "note.md");
	let mut app = app(&root);
	primary(&mut app);
	app.folder_loaded(0, false, super::super::folder::Tree::read(root.clone()));
	app.open(path);
	app.flush_session();
	let saved = Session::read(&root.join("session.json")).unwrap();
	assert_eq!(saved.folder, Some(root.clone()));
	assert!(!saved.folder_open);
	assert_eq!(saved.tabs.len(), 1);
	app.toggle_folder();
	app.flush_session();
	assert!(
		Session::read(&root.join("session.json"))
			.unwrap()
			.folder_open
	);
}

#[test]
fn recent_order_and_each_tabs_search_state_roundtrip() {
	let dir = tempfile::tempdir().unwrap();
	let a = document(dir.path(), "a.md");
	let b = document(dir.path(), "b.md");
	{
		let mut app = app(dir.path());
		app.ui = crate::test_support::shaper();
		primary(&mut app);
		app.readers.open(a.clone(), Instant::now());
		app.open_search();
		app.readers
			.session
			.search
			.input
			.set_text(&mut app.ui, "needle");
		app.readers.session.search.options.case_sensitive = true;
		app.readers.session.search.options.whole_word = true;
		app.search_changed();
		app.readers.open(b.clone(), Instant::now());
		app.readers.session_mut(1).search.query = "second".into();
		app.readers
			.session_mut(1)
			.search
			.input
			.set_text(&mut app.ui, "second");
		app.readers.select(0, Instant::now());
		app.flush_session();
	}
	let saved = Session::read(&dir.path().join("session.json")).unwrap();
	assert_eq!(saved.recent, vec![0, 1]);
	assert!(saved.tabs[0].search.open);
	assert_eq!(saved.tabs[0].search.query, "needle");
	assert!(saved.tabs[0].search.case_sensitive);
	assert!(saved.tabs[0].search.whole_word);
	assert!(!saved.tabs[1].search.open);
	assert_eq!(saved.tabs[1].search.query, "second");

	let mut restored = app(dir.path());
	restored.ui = crate::test_support::shaper();
	primary(&mut restored);
	assert_eq!(restored.readers.session.path, Some(a));
	assert!(restored.readers.session.search.open);
	assert_eq!(restored.readers.session.search.input.text(), "needle");
	assert!(restored.readers.session.search.options.case_sensitive);
	assert!(restored.readers.session.search.options.whole_word);
	assert_eq!(restored.focused_input(), None);
	// A restored inactive query still has no text yet, but saving it again
	// must not replace it with the empty editor.
	restored.flush_session();
	let saved = Session::read(&dir.path().join("session.json")).unwrap();
	assert_eq!(saved.tabs[1].search.query, "second");
	restored.close_tab(restored.readers.active());
	assert_eq!(restored.readers.session.path, Some(b));
}

#[test]
fn restored_web_tabs_carry_a_readable_label() {
	// The strip and its metrics read `page_title`, so a restored web tab that
	// is not fetched until selected must already have one.
	let dir = tempfile::tempdir().unwrap();
	let mut app = app(dir.path());
	let urls = [
		"https://example.org/blog/post",
		"https://news.example.com/story",
	];
	let tabs = urls
		.iter()
		.map(|url| SavedTab {
			origin: crate::security::Origin::Web((*url).to_owned()),
			path: PathBuf::from("web-page"),
			reading: Reading::default(),
			search: SavedSearch::default(),
		})
		.collect();
	app.readers.restore_session(Session {
		folder: None,
		folder_open: false,
		version: 2,
		active: 0,
		tabs,
		recent: vec![0, 1],
	});
	assert_eq!(app.readers.entries().len(), 2);
	assert_eq!(
		app.readers.entries()[0].page_title.as_deref(),
		Some("example.org")
	);
	assert_eq!(
		app.readers.entries()[1].page_title.as_deref(),
		Some("news.example.com")
	);
	for index in 0..2 {
		let label = app.readers.label(index);
		assert!(!label.contains("web:"));
	}
}
