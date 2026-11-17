use super::*;
use crate::app::folder::Tree;
use std::path::Path;

pub(super) fn sidebar(directory: &Path, theme: Theme) -> App<Proxy> {
	sidebar_with_events(directory, theme).0
}

fn sidebar_with_events(
	directory: &Path,
	theme: Theme,
) -> (App<Proxy>, mpsc::Receiver<Event>) {
	let (mut app, events) = app();
	app.preferences.values.theme = theme;
	app.preferences.values.lang = Some(crate::lang::Lang::En);
	app.preferences.values.stylesheet =
		markview_layout::style::Stylesheet::bundled(theme == Theme::Dark);
	app.ui
		.set_stylesheet(app.preferences.values.stylesheet.clone());
	app.folder.tree = Some(Tree::read(directory.to_owned()).unwrap());
	app.folder.open = true;
	app.folder.stop_animation();
	(app, events)
}

pub(super) fn point(app: &mut App<Proxy>, name: &str) -> (Target, Rect) {
	let id = app
		.folder
		.tree
		.as_ref()
		.unwrap()
		.entries
		.iter()
		.find(|e| e.path.file_name().unwrap() == name)
		.unwrap()
		.id;
	let target = Target::File(id);
	let rect = app.scene_preview_anchor(target).unwrap();
	app.interaction
		.pointer
		.move_to((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
	(target, rect)
}

#[test]
fn files_preview_without_opening_a_document_and_keep_the_display_delay() {
	let dir = crate::test_support::TestDir::new();
	std::fs::write(
		dir.path().join("Notes.md"),
		"# Notes\n\n**Preview content**.\n",
	)
	.unwrap();
	let mut app = sidebar(dir.path(), Theme::Light);
	let (target, row) = point(&mut app, "Notes.md");
	let now = Instant::now();
	app.hover_preview(now);
	assert_eq!(app.preview.deadline(), Some(now + PREPARE_DELAY));
	app.preview_tick(now + PREPARE_DELAY);
	let completion = app.preview.worker.wait(Duration::from_secs(10)).unwrap();
	app.preview.accept(completion);
	app.preview_tick(now + SHOW_DELAY - Duration::from_nanos(1));
	assert!(app.preview.open().is_none());
	finish_footnote(&mut app, now);
	assert_eq!(app.preview.open().unwrap().target, target);
	assert!(links::text(&app).contains("Preview content"));
	assert!(app.readers.session.document.is_none());
	assert!(app.readers.session.path.is_none());
	let popup = app.preview.floating().unwrap();
	assert!(popup.rect.x > row.x + row.w);
	assert!(popup.rect.y >= TOP);
	assert!(app.preview.pending.as_ref().unwrap().source.matches(&app));
	let rect = popup.rect;
	for x in (row.x as u32 + row.w as u32 - 1)..=rect.x as u32 + 1 {
		app.interaction
			.pointer
			.move_to((x as f32, row.y + row.h / 2.0));
		assert!(
			!app.hover_preview(now + Duration::from_millis(200)),
			"preview closed at x={x}"
		);
		assert!(app.preview.floating().is_some());
	}
	app.interaction
		.pointer
		.move_to((rect.x + 10.0, rect.y + 10.0));
	assert!(!app.hover_preview(now + Duration::from_secs(1)));
	assert!(app.preview.floating().is_some());
	app.interaction.pointer.move_to((750.0, 550.0));
	assert!(app.hover_preview(now + Duration::from_secs(2)));
	assert!(!app.preview.tracking());
}

#[test]
fn sidebar_preview_switches_files_refreshes_changes_and_cancels_on_scroll() {
	let dir = crate::test_support::TestDir::new();
	for name in ["A.md", "B.md"] {
		std::fs::write(
			dir.path().join(name),
			format!("# {name}\n\nOriginal content.\n"),
		)
		.unwrap();
	}
	let mut app = sidebar(dir.path(), Theme::Light);
	point(&mut app, "A.md");
	let now = Instant::now();
	app.hover_preview(now);
	finish_footnote(&mut app, now);
	assert!(links::text(&app).contains("A.md"));
	point(&mut app, "B.md");
	assert!(app.hover_preview(now + Duration::from_secs(1)));
	assert!(app.preview.open().is_none());
	finish_footnote(&mut app, now + Duration::from_secs(1));
	assert!(links::text(&app).contains("B.md"));
	std::fs::write(dir.path().join("B.md"), "# Updated content\n").unwrap();
	assert!(
		app.preview
			.observed
			.as_ref()
			.unwrap()
			.invalidate_if_outdated()
	);
	assert!(app.refresh_preview_file());
	settle_preview(&mut app, now + Duration::from_secs(1), |_| true);
	assert!(links::text(&app).contains("Updated content"));
	app.scroll_folder(28.0);
	assert!(app.hover_preview(now + Duration::from_secs(2)));
	assert!(app.preview.open().is_none());
}

#[test]
fn folders_headers_and_sidebar_resizing_do_not_start_previews() {
	let dir = crate::test_support::TestDir::new();
	std::fs::create_dir(dir.path().join("Folder")).unwrap();
	std::fs::write(dir.path().join("Notes.md"), "Content.").unwrap();
	let mut app = sidebar(dir.path(), Theme::Light);
	point(&mut app, "Folder");
	assert!(app.target_at_cursor().is_none());
	let drawer = app.folder_rect();
	app.interaction.pointer.move_to((20.0, drawer.y + 10.0));
	assert!(app.target_at_cursor().is_none());
	point(&mut app, "Notes.md");
	let y = app.interaction.pointer.cursor().1;
	app.interaction.pointer.move_to((drawer.x + drawer.w, y));
	assert!(app.target_at_cursor().is_none());
}

#[test]
fn sidebar_pictures_load_real_pixels() {
	let dir = crate::test_support::TestDir::new();
	image::RgbaImage::from_pixel(16, 12, image::Rgba([200, 50, 30, 255]))
		.save(dir.path().join("Picture.png"))
		.unwrap();
	let mut app = sidebar(dir.path(), Theme::Light);
	point(&mut app, "Picture.png");
	let now = Instant::now();
	app.hover_preview(now);
	app.preview_tick(now + PREPARE_DELAY);
	settle_preview(&mut app, now, |floating| {
		!floating.snapshot.images.decoded().is_empty()
	});
	assert!(
		!app.preview
			.floating()
			.unwrap()
			.snapshot
			.images
			.decoded()
			.is_empty()
	);
}

fn disclosure_from_untrusted_session(security: crate::security::Security) {
	use markview_layout::scene::LinkTarget;
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("Notes.md");
	std::fs::write(
		&path,
		"# Notes\n\n<details><summary>Details</summary>\n\nBody.\n\n</details>\n",
	)
	.unwrap();
	let path = path.canonicalize().unwrap();
	let (mut app, events) = sidebar_with_events(dir.path(), Theme::Light);
	let snapshot = picture_reader(
		"Current untrusted content.\n",
		app.options().width,
		Default::default(),
	);
	app.readers.session.accept(snapshot, app.viewport(), None);
	app.readers.session.security = security;
	assert!(app.readers.session.security.check_file(&path).is_err());
	point(&mut app, "Notes.md");
	let now = Instant::now();
	app.hover_preview(now);
	finish_footnote(&mut app, now);
	let floating = app.preview.floating().unwrap();
	let rect = floating
		.snapshot
		.blocks
		.iter()
		.find_map(|block| {
			block.links().find_map(|(link, mut rect)| {
				if !matches!(link.target, LinkTarget::Disclosure(_)) {
					return None;
				}
				rect.y += block.y;
				Some(rect)
			})
		})
		.unwrap();
	let body = floating.body();
	let click = (
		body.x + rect.x + rect.w / 2.0,
		body.y + rect.y + rect.h / 2.0,
	);
	assert!(body.contains(click.0, click.1));
	assert!(app.readers.session.path.is_none());
	app.interaction.pointer.move_to(click);
	assert!(app.refresh_preview_hover().is_some());
	assert!(
		app.preview
			.floating()
			.unwrap()
			.state
			.hover
			.link
			.as_ref()
			.is_some_and(|link| matches!(
				link.target,
				LinkTarget::Disclosure(_)
			))
	);
	app.pointer_pressed();
	app.pointer_released();
	assert_eq!(app.readers.session.path.as_ref(), Some(&path));
	settle(&mut app, &events);
	app.apply_anchor();
	let id = app
		.readers
		.session
		.document
		.as_ref()
		.unwrap()
		.blocks()
		.iter()
		.find_map(|block| {
			let mut found = None;
			markview_document::visit_blocks(block, &mut |node| {
				if matches!(
					node.kind(),
					markview_document::BlockKind::Details { .. }
				) {
					found = Some(node.id());
				}
			});
			found
		})
		.unwrap();
	assert_eq!(app.readers.session.details_open.get(&id), Some(&true));
	assert_eq!(
		app.readers.session.security.origin.trust(),
		crate::security::Trust::Trusted
	);
}

#[test]
fn sidebar_disclosure_transfers_from_clipboard() {
	disclosure_from_untrusted_session(crate::security::Security::default());
}

#[test]
fn sidebar_disclosure_transfers_from_web() {
	disclosure_from_untrusted_session(crate::security::Security::web(
		"https://example.test/".into(),
	));
}

#[test]
fn sidebar_disguised_picture_matches_normal_opening() {
	let dir = crate::test_support::TestDir::new();
	let path = dir.path().join("Picture.md");
	let expected =
		image::RgbaImage::from_pixel(16, 12, image::Rgba([200, 50, 30, 255]));
	expected
		.save_with_format(&path, image::ImageFormat::Png)
		.unwrap();
	assert!(crate::file::is_picture(&path));
	assert!(!crate::file::is_image(&path));
	let (mut reader, events) = app();
	load(&mut reader, &events, path.clone());
	let deadline = Instant::now() + Duration::from_secs(10);
	while reader
		.readers
		.session
		.display
		.snapshot
		.images
		.decoded()
		.is_empty()
	{
		let event = events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.expect("normal opening must decode the disguised picture");
		accept_ready(&mut reader, event);
	}
	let normal = reader
		.readers
		.session
		.display
		.snapshot
		.images
		.decoded()
		.into_values()
		.next()
		.unwrap();
	assert_eq!(&*normal.rgba, expected.as_raw());
	let mut app = sidebar(dir.path(), Theme::Light);
	point(&mut app, "Picture.md");
	let now = Instant::now();
	app.hover_preview(now);
	app.preview_tick(now + PREPARE_DELAY);
	settle_preview(&mut app, now, |floating| {
		let images = &floating.snapshot.images;
		!images.entries.is_empty()
			&& images.entries.values().all(|info| {
				!info.loading && (info.size.is_some() || info.error.is_some())
			})
	});
	let images = &app.preview.floating().unwrap().snapshot.images;
	let key = images
		.key(&markview_layout::image::ImageSource::Resource(
			markview_layout::image::ResourceRef::document(
				crate::file::STANDALONE_IMAGE_SOURCE,
			),
		))
		.unwrap();
	assert!(
		images.entries[key].error.is_none(),
		"{:?}",
		images.entries[key].error
	);
	let decoded = images.decoded();
	assert_eq!(&*decoded[key].rgba, &*normal.rgba);
}

proptest::proptest! {
	#[test]
	fn sidebar_placement_stays_right_and_centers_unless_an_edge_prevents_it(
		width in 100.0_f32..1200.0, height in 100.0_f32..1000.0,
		y in 0.0_f32..1000.0, card_width in 10.0_f32..600.0, card_height in 10.0_f32..600.0,
	) {
		let bounds = Rect { x: 240.0, y: 40.0, w: width, h: height };
		let anchor = Rect { x: 0.0, y, w: 240.0, h: 28.0 };
		let rect = card::place_right(anchor, (card_width, card_height), bounds);
		proptest::prop_assert_eq!(rect.x, 248.0);
		proptest::prop_assert!(rect.x + rect.w <= bounds.x + bounds.w - 7.99);
		proptest::prop_assert!(rect.y >= bounds.y + 8.0);
		proptest::prop_assert!(rect.y + rect.h <= bounds.y + bounds.h - 7.99);
		let centered = anchor.y + anchor.h / 2.0 - rect.h / 2.0;
		let expected = centered.clamp(bounds.y + 8.0, bounds.y + 8.0 + (bounds.h - 16.0 - rect.h));
		proptest::prop_assert!((rect.y - expected).abs() < 0.001);
	}
}

#[test]
fn sidebar_previews_match_rendering_baselines() -> anyhow::Result<()> {
	if !markview_platform::test_support::desktop_goldens() {
		return Ok(());
	}
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	let mut baselines = crate::test_support::render_goldens::Baselines::new(
		root.join("tests/goldens/sidebar-preview"),
		root.join("artifacts/sidebar-preview-goldens"),
	)?;
	let mut renderer = pollster::block_on(crate::render::Renderer::new(None))?;
	let dir = crate::test_support::TestDir::new();
	for i in 0..25 {
		std::fs::write(dir.path().join(format!("{i:02} - Reading notes.md")), format!(
			"# Reading notes {i:02}\n\nA file preview keeps your place in the current document.\n\n## Typography\n\n**Bold text**, *italic text*, and a [local link](00%20-%20Reading%20notes.md).\n\n- Hover over a file\n- Read its formatted content\n- Scroll to see more\n\n{}",
			"More notes to read in this preview.\n\n".repeat(12)
		)).unwrap();
	}
	for (theme, suffix, scale) in [
		(Theme::Light, "light", 1.0),
		(Theme::Dark, "dark-125", 1.25),
	] {
		for (index, position) in [(0, "top"), (7, "center"), (24, "bottom")] {
			let mut app = sidebar(dir.path(), theme);
			// Stable labels keep temporary paths out of the rendered chrome.
			app.folder.tree.as_mut().unwrap().name = "Reading library".into();
			let snapshot = picture_reader(
				"# Your reading stays here\n\nHover over a file in the sidebar to preview it.\n\nNo tab switch is needed.\n",
				app.options().width,
				Default::default(),
			);
			app.readers.session.accept(snapshot, app.viewport(), None);
			app.readers.session.path = Some("Current document.md".into());
			let (_, row) =
				point(&mut app, &format!("{index:02} - Reading notes.md"));
			let now = Instant::now();
			app.hover_preview(now);
			finish_footnote(&mut app, now);
			let floating = app.preview.floating().unwrap();
			assert!(floating.rect.x > row.x + row.w);
			if position == "center" {
				assert!(
					(floating.rect.y + floating.rect.h / 2.0
						- row.y
						- row.h / 2.0)
						.abs()
						< 0.01
				);
			}
			let actual =
				super::golden::capture_frame(&mut renderer, &mut app, scale)?;
			baselines.record(&format!("{position}-{suffix}"), &actual)?;
		}
	}
	baselines.finish()
}
