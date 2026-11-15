use super::*;
use markview_interaction::Point;
use std::path::Path;

fn reader(source: &str) -> App<Proxy> {
	let (mut app, _) = app();
	let snapshot =
		picture_reader(source, app.options().width, Default::default());
	app.readers.session.accept(snapshot, app.viewport(), None);
	app.readers.session.security =
		crate::security::Security::local(crate::security::Trust::Trusted);
	app
}

pub(super) fn hover(app: &mut App<Proxy>, url: &str) -> Instant {
	let rect = app
		.readers
		.session
		.display
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().find_map(|(link, mut rect)| {
				if link.url.as_ref() != url {
					return None;
				}
				rect.y += block.y;
				Some(app.view_geometry().window_rect(rect))
			})
		})
		.unwrap();
	app.interaction
		.pointer
		.move_to((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
	let now = Instant::now();
	app.hover_preview(now);
	now
}

pub(super) fn text(app: &App<Proxy>) -> String {
	let floating = app.preview.floating().unwrap();
	let revision = floating.revision.get();
	floating
		.snapshot
		.extract_text(floating.snapshot.select_all(revision).unwrap(), revision)
}

fn file_reader(
	directory: &Path,
	url: &str,
) -> (App<Proxy>, mpsc::Receiver<Event>) {
	let path = directory.join("source.md");
	std::fs::write(&path, format!("[Preview]({url})\n")).unwrap();
	let (mut app, events) = app();
	load(&mut app, &events, path);
	(app, events)
}

#[test]
fn offscreen_section_reuses_delays_and_contains_only_its_section() {
	let source = format!(
		"[Preview](#target)\n\n{}## Target\n\n**Target content**.\n\n### Child\n\nNested content.\n\n## Next\n\nExcluded content.\n",
		"Earlier content.\n\n".repeat(40)
	);
	let mut app = reader(&source);
	let now = hover(&mut app, "#target");
	assert_eq!(app.preview.deadline(), Some(now + PREPARE_DELAY));
	finish_footnote(&mut app, now);
	let content = text(&app);
	assert!(
		content.contains("Target content")
			&& content.contains("Nested content")
	);
	assert!(
		!content.contains("Earlier content")
			&& !content.contains("Excluded content")
	);
	assert!(app.readers.entries().is_empty());
	assert_eq!(app.readers.session.scrolling.offset, 0.0);
}

#[test]
fn visible_sections_do_not_schedule_a_preview_even_with_a_file_alias() {
	let dir = tempfile::tempdir().unwrap();
	for url in ["#target", "source.md#target"] {
		let (mut app, _events) = file_reader(dir.path(), url);
		let snapshot = picture_reader(
			&format!("[Preview]({url})\n\n## Target\n\nContent.\n"),
			app.options().width,
			Default::default(),
		);
		app.readers.session.accept(snapshot, app.viewport(), None);
		let now = hover(&mut app, url);
		app.preview_tick(now + SHOW_DELAY);
		assert!(!app.preview.tracking());
		assert!(app.preview.pending.is_none());
	}
}

#[test]
fn file_previews_resolve_encoded_paths_and_fragments_and_keep_the_source_open()
{
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(dir.path().join("other note.md"), "# Before\n\nExcluded.\n\n## 目标\n\nFile **content**.\n\n## After\n\nExcluded too.\n").unwrap();
	let url = "other%20note.md#%E7%9B%AE%E6%A0%87";
	let (mut app, _events) = file_reader(dir.path(), url);
	let original = app.readers.session.path.clone();
	let now = hover(&mut app, url);
	finish_footnote(&mut app, now);
	assert!(text(&app).contains("File content"));
	assert!(!text(&app).contains("Excluded"));
	assert_eq!(app.readers.session.path, original);
	assert_eq!(
		app.preview.open().unwrap().path.as_deref(),
		Some(
			dir.path()
				.join("other note.md")
				.canonicalize()
				.unwrap()
				.as_path()
		)
	);
}

#[test]
fn file_without_a_fragment_previews_its_opening_paragraphs() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(
		dir.path().join("other.md"),
		"Opening paragraph.\n\nSecond paragraph.\n",
	)
	.unwrap();
	let (mut app, _events) = file_reader(dir.path(), "other.md");
	let now = hover(&mut app, "other.md");
	finish_footnote(&mut app, now);
	assert!(
		text(&app).contains("Opening paragraph")
			&& text(&app).contains("Second paragraph")
	);
}

#[test]
fn missing_files_and_anchors_finish_without_a_popup_or_busy_deadline() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(dir.path().join("other.md"), "# Existing\n").unwrap();
	for url in ["missing.md", "other.md#missing", "#missing"] {
		let (mut app, _events) = file_reader(dir.path(), url);
		let now = hover(&mut app, url);
		finish_footnote(&mut app, now);
		assert!(app.preview.open().is_none());
		assert_eq!(app.preview.deadline(), None);
	}
}

#[test]
fn remote_and_unapproved_file_links_do_not_start_background_work() {
	let dir = tempfile::tempdir().unwrap();
	for url in [
		"https://example.com/other.md",
		"other.md",
		"other.pdf",
		"javascript:alert(1)",
	] {
		let (mut app, _events) = file_reader(dir.path(), url);
		app.readers.session.security = Default::default();
		let now = hover(&mut app, url);
		app.preview_tick(now + SHOW_DELAY);
		assert!(!app.preview.tracking());
		assert!(app.preview.pending.is_none());
	}
}

pub(super) fn click_first_preview_link(app: &mut App<Proxy>) {
	let floating = app.preview.floating().unwrap();
	let rect = floating
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().next().map(|(_, mut rect)| {
				rect.y += block.y;
				rect
			})
		})
		.unwrap();
	let body = floating.body();
	let point = Point::new(
		body.x + rect.x + rect.w / 2.0,
		body.y + rect.y + rect.h / 2.0,
	);
	app.interaction.pointer.move_to((point.x, point.y));
	app.pointer_pressed();
	app.pointer_released();
}

#[test]
fn links_in_a_file_preview_navigate_relative_to_the_previewed_file() {
	let dir = tempfile::tempdir().unwrap();
	let sub = dir.path().join("sub");
	std::fs::create_dir(&sub).unwrap();
	std::fs::write(sub.join("other.md"), "# Other\n\n[Next](next.md#target)\n")
		.unwrap();
	std::fs::write(sub.join("next.md"), "# Target\n").unwrap();
	let (mut app, _events) = file_reader(dir.path(), "sub/other.md");
	let now = hover(&mut app, "sub/other.md");
	finish_footnote(&mut app, now);
	click_first_preview_link(&mut app);
	settle(&mut app, &_events);
	assert!(app.preview.open().is_none());
	assert_eq!(
		app.readers.session.path,
		Some(sub.join("next.md").canonicalize().unwrap())
	);
	assert_eq!(
		app.readers.session.jump_origin.as_ref().unwrap().0,
		markview_document::AnchorId::Fragment("target".into())
	);
}

#[test]
fn footnote_references_in_file_previews_keep_their_typed_destination() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("other.md");
	std::fs::write(&path, "# Other\n\nRead[^note].\n\n[^note]: A note.\n")
		.unwrap();
	let (mut app, _events) = file_reader(dir.path(), "other.md");
	let now = hover(&mut app, "other.md");
	finish_footnote(&mut app, now);
	click_first_preview_link(&mut app);
	settle(&mut app, &_events);
	assert_eq!(app.readers.session.path, Some(path.canonicalize().unwrap()));
	assert!(matches!(
		app.readers
			.session
			.jump_origin
			.as_ref()
			.map(|(target, _)| target),
		Some(markview_document::AnchorId::FootnoteDefinition(_))
	));
}

#[test]
fn file_block_ids_use_the_current_markdown_dialect() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(
		dir.path().join("other.md"),
		"Before.\n\nSelected block. ^Block-ID\n\nAfter.\n",
	)
	.unwrap();
	let (mut app, _events) = file_reader(dir.path(), "other.md#^Block-ID");
	app.preferences.values.markdown_flavor =
		crate::settings::MarkdownFlavor::Obsidian;
	let now = hover(&mut app, "other.md#^Block-ID");
	finish_footnote(&mut app, now);
	assert!(text(&app).contains("Selected block"));
	assert!(
		!text(&app).contains("Before")
			&& !text(&app).contains("After")
			&& !text(&app).contains("^Block-ID")
	);
}

#[test]
fn revoking_file_access_cancels_pending_preview_work() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(dir.path().join("other.md"), "Content.\n").unwrap();
	let (mut app, _events) = file_reader(dir.path(), "other.md");
	let now = hover(&mut app, "other.md");
	app.preview_tick(now + PREPARE_DELAY);
	let token = app.preview.worker.token().unwrap();
	app.readers.session.security = Default::default();
	app.preview_tick(now + SHOW_DELAY);
	assert!(token.is_cancelled());
	assert!(!app.preview.tracking());
}

#[test]
fn file_previews_use_their_own_disclosure_defaults() {
	let dir = tempfile::tempdir().unwrap();
	let source = "# Other\n\n<details open><summary>Summary</summary>\n\nVisible content.\n\n</details>\n";
	std::fs::write(dir.path().join("other.md"), source).unwrap();
	let document = markview_commonmark::parse(source);
	let disclosure = document.blocks()[1].id();
	let (mut app, _events) = file_reader(dir.path(), "other.md");
	std::sync::Arc::make_mut(&mut app.readers.session.details_open)
		.insert(disclosure, false);
	let now = hover(&mut app, "other.md");
	finish_footnote(&mut app, now);
	assert!(text(&app).contains("Visible content"));
}

proptest::proptest! {
	#![proptest_config(proptest::test_runner::Config::with_cases(32))]
	#[test]
	fn target_visibility_follows_the_clipped_viewport(offset in -1000i16..1000, search_open in proptest::bool::ANY) {
		let mut app = reader("# Target\n\nContent.\n");
		app.readers.session.search.open = search_open;
		let target = app.readers.session.display.snapshot.anchor_y("target").unwrap();
		app.readers.session.scrolling.offset = target + f32::from(offset);
		let viewport = app.view_geometry();
		let point = viewport.window_rect(Rect { x: 1.0, y: target, w: 1.0, h: 1.0 });
		let destination = work::Destination { path: None, anchor: Some(markview_document::AnchorId::Fragment("target".into())) };
		proptest::prop_assert_eq!(app.preview_destination_visible(&destination), viewport.clip().intersect(Rect { h: 0.5, ..point }).is_some());
	}
}
