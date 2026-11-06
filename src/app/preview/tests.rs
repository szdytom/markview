use super::*;
use crate::{
	app::{Event, SendEvent},
	cli::{LaunchOptions, Mode},
};
use std::{path::PathBuf, sync::mpsc};

/// Captures the app's events so a test can feed layout results back.
#[derive(Clone)]
struct Proxy(mpsc::Sender<Event>);
impl SendEvent for Proxy {
	fn try_send(&self, event: Event) -> bool {
		self.0.send(event).is_ok()
	}
}

fn app() -> (App<Proxy>, mpsc::Receiver<Event>) {
	let (tx, events) = mpsc::channel();
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			offline: true,
			width: 800,
			height: 600,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy(tx),
	);
	app.preferences.values = Default::default();
	(app, events)
}

/// Opens a file and runs its layout results through the session.
fn load(app: &mut App<Proxy>, events: &mpsc::Receiver<Event>, path: PathBuf) {
	app.open(path);
	settle(app, events);
}
fn settle(app: &mut App<Proxy>, events: &mpsc::Receiver<Event>) {
	let deadline = Instant::now() + Duration::from_secs(10);
	while app.readers.session.layout_pending {
		let event = events
			.recv_timeout(deadline.saturating_duration_since(Instant::now()))
			.expect("the document did not render");
		let Event::Ready(update) = event else {
			continue;
		};
		if update.version == app.readers.session.version
			&& Some(&update.path) == app.readers.session.path.as_ref()
			&& let Some(result) = update.result
		{
			let reader = result.unwrap();
			let viewport = app.viewport();
			if app.readers.session.can_display(&reader, viewport) {
				app.readers.session.accept(reader, viewport, update.counts);
				app.readers.session.displayed_version = update.version;
			}
		}
	}
}

fn card(tab: u64) -> Open {
	Open {
		target: Target::Tab(tab),
		card: Card {
			title: vec!["notes.md".into()],
			detail: "/tmp/notes.md".into(),
			body: true,
		},
	}
}

fn picture_reader(
	source: &str,
	width: f32,
	info: markview_layout::image::ImageInfo,
) -> crate::worker::ReaderSnapshot {
	use markview_layout::image::{ImageSnapshot, ImageSource, ResourceRef};
	let document = crate::loaded::LoadedDocument::markdown(
		std::sync::Arc::new(markview_commonmark::parse(source)),
	);
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		ImageSource::Resource(ResourceRef::document("picture.png")),
		"picture".into(),
	);
	images.entries.insert("picture".into(), info);
	let options = crate::layout::LayoutOptions {
		width,
		..crate::test_support::options()
	};
	crate::worker::ReaderSnapshot {
		layout: crate::layout::LayoutEngine::new()
			.layout_with_images(&document, &options, &images),
		document,
		blocked_images: Default::default(),
		content_version: 1,
		complete: true,
		parse_complete: true,
		remote_deferred: 0,
	}
}

#[test]
fn a_held_hover_opens_only_after_the_delay() {
	let mut preview = Preview::default();
	let now = Instant::now();
	assert!(!preview.tracking());
	assert_eq!(preview.due(now + SHOW_DELAY), None);

	assert!(!preview.hover(Some(Target::Tab(7)), now));
	assert!(preview.tracking());
	assert_eq!(preview.deadline(), Some(now + SHOW_DELAY));
	assert_eq!(preview.due(now), None);
	assert_eq!(
		preview.due(now + SHOW_DELAY - Duration::from_millis(1)),
		None
	);
	assert_eq!(preview.due(now + SHOW_DELAY), Some(Target::Tab(7)));

	preview.show(card(7));
	assert_eq!(preview.due(now + Duration::from_secs(1)), None);
	assert_eq!(preview.deadline(), None);
	assert_eq!(preview.open().map(|open| open.target), Some(Target::Tab(7)));
}

#[test]
fn crossing_targets_restarts_the_delay_and_clears_the_old_card() {
	let mut preview = Preview::default();
	let now = Instant::now();
	preview.hover(Some(Target::Tab(1)), now);
	preview.show(card(1));
	assert!(preview.open().is_some());

	// Moving to another target closes the card at once and holds the new delay.
	assert!(
		preview.hover(Some(Target::Tab(2)), now + Duration::from_millis(20))
	);
	assert!(preview.open().is_none());
	let due = now + Duration::from_millis(20) + SHOW_DELAY;
	assert_eq!(preview.due(due - Duration::from_millis(1)), None);
	assert_eq!(preview.due(due), Some(Target::Tab(2)));

	// Motion inside the same target keeps the original deadline.
	preview.hover(Some(Target::Tab(2)), now + Duration::from_millis(60));
	assert_eq!(preview.deadline(), Some(due));
}

#[test]
fn leaving_every_target_forgets_the_hover() {
	let mut preview = Preview::default();
	let now = Instant::now();
	preview.hover(Some(Target::Tab(3)), now);
	assert!(!preview.hover(None, now + Duration::from_millis(10)));
	assert!(!preview.tracking());
	assert_eq!(preview.deadline(), None);

	preview.hover(Some(Target::Tab(3)), now);
	preview.show(card(3));
	assert!(preview.hover(None, now + Duration::from_millis(20)));
	assert!(preview.open().is_none());
	assert!(!preview.tracking());
}

#[test]
fn the_chrome_body_rect_belongs_to_the_open_card() {
	let mut preview = Preview::default();
	let rect = Rect {
		x: 10.0,
		y: 40.0,
		w: 240.0,
		h: 150.0,
	};
	preview.set_body(Some(rect));
	assert_eq!(preview.body(), None);
	preview.hover(Some(Target::Tab(1)), Instant::now());
	preview.show(card(1));
	assert_eq!(preview.body(), None);
	preview.set_body(Some(rect));
	assert_eq!(preview.body(), Some(rect));
	preview.dismiss();
	assert_eq!(preview.body(), None);
}

/// The application-level flow: a hovering pointer arms the delay, the tick
/// opens the card, and motion away closes it.
#[test]
fn a_resting_pointer_opens_the_card_after_the_delay() {
	let (mut app, _events) = app();
	let second = std::path::PathBuf::from("/tmp/second.md");
	app.readers
		.open(std::path::PathBuf::from("/tmp/first.md"), Instant::now());
	app.readers.open(second.clone(), Instant::now());
	let layout = app.tab_layout();
	let cursor = (layout.rects[0].x + 20.0, 20.0);
	app.interaction.pointer.move_to(cursor);
	let now = Instant::now();

	assert!(!app.hover_preview(now));
	assert!(app.preview.open().is_none());
	app.preview_tick(now + Duration::from_millis(50));
	assert!(app.preview.open().is_none());

	app.preview_tick(now + SHOW_DELAY);
	let open = app.preview.open().expect("the card did not open");
	assert_eq!(open.target, Target::Tab(app.readers.entries()[0].id));
	assert_eq!(open.card.title, ["first.md"]);
	assert!(open.card.detail.ends_with("first.md"));
	assert!(!open.card.body, "an unrendered tab must not reserve a body");

	// The chrome paints a card, and the preview never opens inside a panel.
	let draws = app.overlay();
	assert!(draws.iter().any(|draw| matches!(
		draw,
		crate::layout::Draw::Box {
			condition: markview_layout::style::Condition::Preview,
			radius: 0.0,
			..
		}
	)));
	app.interaction.panel = crate::state::PanelPage::Tabs;
	app.preview_tick(now + Duration::from_secs(1));
	assert!(app.preview.open().is_none());
	app.interaction.panel = crate::state::PanelPage::Closed;

	// Opening again and moving off the strip closes the card at once.
	app.interaction.pointer.move_to(cursor);
	app.hover_preview(now + Duration::from_secs(2));
	let at = now + Duration::from_secs(2) + SHOW_DELAY;
	app.preview_tick(at);
	assert!(app.preview.open().is_some());
	app.interaction.pointer.move_to((400.0, 300.0));
	assert!(app.hover_preview(at + Duration::from_millis(1)));
	assert!(app.preview.open().is_none());
}

/// The active page is already on screen, so its card only names it; another
/// tab's rendered page still gets a body.
#[test]
fn the_active_tab_opens_a_text_only_card() {
	use std::fs;

	let (mut app, events) = app();
	let dir = tempfile::tempdir().unwrap();
	let first = dir.path().join("first.md");
	let second = dir.path().join("second.md");
	fs::write(&first, "# First\n\nReading paragraph.\n").unwrap();
	fs::write(&second, "# Second\n\nReading paragraph.\n").unwrap();
	load(&mut app, &events, first);
	load(&mut app, &events, second);
	assert!(!app.readers.session.display.snapshot.blocks.is_empty());
	assert!(
		!app.readers.entries()[0]
			.session
			.display
			.snapshot
			.blocks
			.is_empty()
	);

	// Hovering a tab inside a slot the open slide has not filled yet would
	// miss it, so read the strip at rest.
	app.tab_strip.stop_animation();
	let layout = app.tab_layout();
	let now = Instant::now();
	app.interaction
		.pointer
		.move_to((layout.rects[1].x + 5.0, 20.0));
	app.hover_preview(now);
	app.preview_tick(now + SHOW_DELAY);
	let open = app
		.preview
		.open()
		.expect("the active tab's card did not open");
	assert_eq!(open.target, Target::Tab(app.readers.entries()[1].id));
	assert!(!open.card.body, "the active tab must not render a body");
	assert_eq!(open.card.title, ["second.md"]);
	assert!(open.card.detail.ends_with("second.md"));
	app.overlay();
	assert_eq!(app.preview.body(), None);

	// The inactive tab still previews its rendered page.
	app.interaction
		.pointer
		.move_to((layout.rects[0].x + 5.0, 20.0));
	app.hover_preview(now + Duration::from_secs(1));
	app.preview_tick(now + Duration::from_secs(1) + SHOW_DELAY);
	let open = app
		.preview
		.open()
		.expect("the inactive tab's card did not open");
	assert_eq!(open.target, Target::Tab(app.readers.entries()[0].id));
	assert!(open.card.body);

	// Making that tab active ends the card describing it as a preview.
	app.select_tab(0);
	assert!(app.preview.open().is_none());
}

#[test]
fn an_idle_tab_keeps_its_page_preview_after_releasing_its_document() {
	let (mut app, events) = app();
	let dir = tempfile::tempdir().unwrap();
	let first = dir.path().join("first.md");
	let second = dir.path().join("second.md");
	std::fs::write(&first, "Reading paragraph.\n\n".repeat(300)).unwrap();
	std::fs::write(&second, "# Second\n").unwrap();
	load(&mut app, &events, first);
	let block = 80;
	app.readers.session.scrolling.offset =
		app.readers.session.display.snapshot.blocks[block].y + 5.0;
	app.readers
		.session
		.display
		.horizontal
		.insert((block, 0), 24.0);
	let original = app.readers.session.display.snapshot.clone();
	let scroll = app.readers.session.scrolling.offset;
	load(&mut app, &events, second);
	let layout = app.tab_layout();
	app.interaction
		.pointer
		.move_to((layout.rects[0].x + 5.0, 20.0));
	let now = Instant::now();
	app.hover_preview(now);
	app.preview_tick(now + SHOW_DELAY);
	assert!(app.preview.open().unwrap().card.body);

	// Releasing an idle tab must preserve both an open card and later hovers.
	for elapsed in [Duration::from_secs(21), Duration::from_secs(86_400)] {
		let at = now + elapsed;
		app.readers.release_inactive(at);
		let session = &app.readers.entries()[0].session;
		assert!(session.document.is_none());
		assert!(session.display.snapshot.blocks.is_empty());
		assert!(app.readers.session.document.is_some());
		app.overlay();
		let scene = app.preview.scene(&app.readers, Theme::Light).unwrap();
		assert_eq!(scene.scroll, scroll);
		assert_eq!(scene.snapshot.width, original.width);
		assert!(scene.snapshot.blocks.len() < original.blocks.len());
		let first = original.blocks.partition_point(|b| {
			b.y + b.height() < (scroll - app.viewport()).max(0.)
		});
		assert_eq!(scene.snapshot.blocks[0].id, original.blocks[first].id);
		assert_eq!(scene.horizontal.get(&(block - first, 0)), Some(&24.0));
		app.dismiss_preview();
		app.hover_preview(at);
		app.preview_tick(at + SHOW_DELAY);
		assert!(app.preview.open().unwrap().card.body);
	}

	// Activating the tab still reloads its full document at the saved position.
	app.select_tab(0);
	assert!(app.preview.open().is_none());
	assert!(app.readers.session.layout_pending);
	assert_eq!(app.readers.session.scrolling.offset, scroll);
	assert!(app.readers.entries()[0].preview.is_none());
	assert!(app.readers.session.display.cached_slice.is_some());
	let cached = &app.readers.session.display.snapshot;
	let local = cached
		.blocks
		.iter()
		.position(|b| b.id == original.blocks[block].id)
		.unwrap();
	assert!(std::sync::Arc::ptr_eq(
		&cached.blocks[local].layout,
		&original.blocks[block].layout
	));
	let counts = app.readers.session.counts;
	app.scroll_by(30.0);
	settle(&mut app, &events);
	assert!(app.readers.session.display.cached_slice.is_none());
	assert!(app.readers.session.saved_reading.is_none());
	assert!(
		(app.readers.session.scrolling.offset - scroll - 30.0).abs() < 0.01
	);
	assert_eq!(app.readers.session.counts, counts);
}

#[test]
fn scrolling_beyond_the_slice_and_switching_again_preserves_the_new_position() {
	let (mut app, events) = app();
	let dir = tempfile::tempdir().unwrap();
	let first = dir.path().join("first.md");
	let second = dir.path().join("second.md");
	std::fs::write(&first, "Repeated paragraph.\n\n".repeat(300)).unwrap();
	std::fs::write(&second, "# Second\n").unwrap();
	load(&mut app, &events, first);
	app.readers.session.scrolling.offset =
		app.readers.session.display.snapshot.blocks[80].y;
	load(&mut app, &events, second);
	app.readers
		.release_inactive(Instant::now() + Duration::from_secs(21));
	app.select_tab(0);
	assert!(app.readers.session.display.cached_slice.is_some());
	app.scroll_by(app.viewport() * 4.0);
	let expected = app.readers.session.scrolling.offset;
	app.select_tab(1);
	assert!(app.readers.entries()[0].preview.is_none());
	app.select_tab(0);
	settle(&mut app, &events);
	assert!((app.readers.session.scrolling.offset - expected).abs() < 0.01);
}

#[test]
fn a_slice_tracks_repeated_blocks_when_an_earlier_image_changes_height() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let source = format!(
		"![Image](picture.png)\n\n{}",
		"Repeated paragraph.\n\n".repeat(300)
	);
	let layout = |height| {
		let document = crate::loaded::LoadedDocument::markdown(
			std::sync::Arc::new(markview_commonmark::parse(source.as_str())),
		);
		let mut images = ImageSnapshot::default();
		images.keys.insert(
			ImageSource::Resource(ResourceRef::document("picture.png")),
			"picture".into(),
		);
		images.entries.insert(
			"picture".into(),
			ImageInfo {
				size: Some((100, height)),
				..Default::default()
			},
		);
		crate::worker::ReaderSnapshot {
			layout: crate::layout::LayoutEngine::new().layout_with_images(
				&document,
				&crate::test_support::options(),
				&images,
			),
			document,
			blocked_images: Default::default(),
			content_version: 1,
			complete: true,
			parse_complete: true,
			remote_deferred: 0,
		}
	};
	let mut session = ReaderSession::default();
	session.accept(layout(100), 200., None);
	session.scrolling.offset = session.display.snapshot.blocks[80].y + 5.;
	let page = Page::capture(&session).unwrap();
	session.release_heavy();
	page.restore(&mut session);
	session.scroll_by(30., 200.);
	let rebuilt = layout(300);
	let expected = rebuilt.layout.blocks[80].y + 35.;
	assert!(session.can_display(&rebuilt, 200.));
	session.accept(rebuilt, 200., None);
	assert!((session.scrolling.offset - expected).abs() < 0.01);
}

#[test]
fn cached_image_loading_state_does_not_move_the_reading_position() {
	use markview_layout::image::ImageInfo;
	let source = format!(
		"# Heading\n\n![Image](picture.png)\n\n{}",
		"Following paragraph.\n\n".repeat(40)
	);
	for width in [371.42856, 760.] {
		for local in [1., 100.] {
			let mut session = ReaderSession::default();
			session.accept(
				picture_reader(
					&source,
					width,
					ImageInfo {
						loading: true,
						size: Some((512, 512)),
						..Default::default()
					},
				),
				200.,
				None,
			);
			let scroll = session.display.snapshot.blocks[1].y + local;
			session.scrolling.offset = scroll;
			let page = Page::capture(&session).unwrap();
			// Return before geometry is released, with no saved position to mask a shift.
			page.restore(&mut session);
			assert!(session.display.cached_slice.is_some());
			assert!(session.saved_reading.is_none());
			let ready = picture_reader(
				&source,
				width,
				ImageInfo {
					version: 1,
					size: Some((512, 512)),
					..Default::default()
				},
			);
			assert!(session.can_display(&ready, 200.));
			session.accept(ready, 200., None);
			assert!(
				(session.scrolling.offset - scroll).abs() < 0.01,
				"width={width}, local={local}: {scroll} -> {}",
				session.scrolling.offset
			);
		}
	}
}

#[test]
fn a_slice_follows_text_below_a_picture_resized_inside_one_block() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let lines = (0..30)
		.map(|i| format!("Target paragraph {i} below the picture."))
		.collect::<Vec<_>>()
		.join("\n>\n> ");
	let source = format!(
		"{}> ![Image](picture.png)\n>\n> {lines}\n\n{}",
		"Filler paragraph.\n\n".repeat(60),
		"Trailing paragraph.\n\n".repeat(20)
	);
	let layout = |height| {
		let document = crate::loaded::LoadedDocument::markdown(
			std::sync::Arc::new(markview_commonmark::parse(source.as_str())),
		);
		let mut images = ImageSnapshot::default();
		images.keys.insert(
			ImageSource::Resource(ResourceRef::document("picture.png")),
			"picture".into(),
		);
		images.entries.insert(
			"picture".into(),
			ImageInfo {
				loading: false,
				size: Some((100, height)),
				..Default::default()
			},
		);
		crate::worker::ReaderSnapshot {
			layout: crate::layout::LayoutEngine::new().layout_with_images(
				&document,
				&crate::test_support::options(),
				&images,
			),
			document,
			blocked_images: Default::default(),
			content_version: 1,
			complete: true,
			parse_complete: true,
			remote_deferred: 0,
		}
	};
	let mut session = ReaderSession::default();
	session.accept(layout(100), 200., None);
	let index = session
		.display
		.snapshot
		.blocks
		.iter()
		.position(|b| {
			b.layout
				.draws
				.iter()
				.any(|d| matches!(d, Draw::Image { .. }))
		})
		.unwrap();
	let block = &session.display.snapshot.blocks[index];
	let (node, cluster, rect) = block
		.clusters()
		.filter(|(node, _, _)| block.layout.text[*node].search_field.is_some())
		.max_by(|(_, _, a), (_, _, b)| a.y.total_cmp(&b.y))
		.unwrap();
	let scroll = block.y + rect.y;
	let start = cluster.range.start;
	assert!(
		scroll - block.y > 300.,
		"the picture sits above the anchored window"
	);
	session.scrolling.offset = scroll;
	let page = Page::capture(&session).unwrap();
	session.release_heavy();
	page.restore(&mut session);
	// A tab restored without a reading position relies on the cached slice.
	session.saved_reading = None;
	let rebuilt = layout(500);
	let after = &rebuilt.layout.blocks[index];
	let next = after.layout.text[node]
		.clusters
		.iter()
		.find(|c| c.range.start == start)
		.unwrap();
	let expected = after.y + after.rect(next.command, next.rect).unwrap().y;
	assert!(expected > scroll + 10., "the picture pushed the text down");
	assert!(session.can_display(&rebuilt, 200.));
	session.accept(rebuilt, 200., None);
	assert!((session.scrolling.offset - expected).abs() < 0.01);
}

#[test]
fn an_inactive_tab_pins_only_its_visible_image_pixels() {
	use markview_layout::image::{ImageInfo, ImageSnapshot, Pixels};
	use std::sync::Arc;

	let (mut app, _events) = app();
	let now = Instant::now();
	app.readers.open("/tmp/images.md".into(), now);
	let document = Arc::new(markview_commonmark::parse(format!(
		"![Visible](visible.png)\n\n{}![Offscreen](offscreen.png)\n",
		"Reading paragraph.\n\n".repeat(100),
	)));
	let mut images = ImageSnapshot {
		generation: 42,
		..Default::default()
	};
	let pixels = Arc::new(Pixels {
		width: 1,
		height: 1,
		rgba: Arc::from([0, 0, 0, 255]),
	});
	for alias in ["visible.png", "offscreen.png"] {
		images.keys.insert(
			markview_layout::image::ImageSource::Resource(
				markview_layout::image::ResourceRef::document(alias),
			),
			alias.into(),
		);
		images.entries.insert(
			alias.into(),
			ImageInfo {
				loading: false,
				version: 7,
				size: Some((1, 1)),
				error: None,
				svg: false,
			},
		);
		images.pixels.insert(alias.into(), 7, pixels.clone());
	}
	app.readers.session.display.snapshot = crate::layout::LayoutEngine::new()
		.layout_with_images(
			&document,
			&crate::test_support::options(),
			&images,
		);
	app.readers.session.document =
		Some(crate::loaded::LoadedDocument::markdown(document));
	app.readers.open("/tmp/second.md".into(), now);
	images.pixels.replace(Default::default());
	app.readers.release_inactive(now + Duration::from_secs(21));

	let page = app.readers.entries()[0].preview.as_ref().unwrap();
	assert_eq!(page.snapshot.images.generation, 42);
	assert_eq!(page.snapshot.images.entries.len(), 1);
	assert!(page.snapshot.images.entries.contains_key("visible.png"));
	assert!(Arc::ptr_eq(
		&page.snapshot.images.pixels.get("visible.png", 7).unwrap(),
		&pixels
	));
	assert!(
		page.snapshot
			.images
			.pixels
			.get("offscreen.png", 7)
			.is_none()
	);
	assert_eq!(Arc::strong_count(&pixels), 2);
}

#[test]
fn a_centered_picture_keeps_its_block_and_pixels_in_the_tab_preview() {
	use markview_layout::image::{ImageInfo, ImageSnapshot, Pixels};
	use std::sync::Arc;

	let document = Arc::new(markview_commonmark::parse("![](picture.png)"));
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		markview_layout::image::ImageSource::Resource(
			markview_layout::image::ResourceRef::document("picture.png"),
		),
		"picture.png".into(),
	);
	images.entries.insert(
		"picture.png".into(),
		ImageInfo {
			loading: false,
			version: 1,
			size: Some((100, 140)),
			error: None,
			svg: false,
		},
	);
	images.pixels.insert(
		"picture.png".into(),
		1,
		Arc::new(Pixels {
			width: 1,
			height: 1,
			rgba: Arc::from([0, 0, 0, 255]),
		}),
	);
	let options = crate::layout::LayoutOptions {
		width: 400.0,
		viewport_height: 1000.0,
		..crate::test_support::options()
	};
	let snapshot = crate::layout::LayoutEngine::new()
		.layout_with_images(&document, &options, &images);
	assert!(snapshot.centered);

	let (mut app, _events) = app();
	let now = Instant::now();
	app.readers.open("/tmp/picture.md".into(), now);
	app.readers.session.display.snapshot = snapshot;
	app.readers.session.document =
		Some(crate::loaded::LoadedDocument::markdown(document));
	app.readers.open("/tmp/second.md".into(), now);
	app.readers.release_inactive(now + Duration::from_secs(21));

	let page = app.readers.entries()[0].preview.as_ref().unwrap();
	assert_eq!(page.snapshot.blocks.len(), 1);
	assert!(page.snapshot.images.pixels.get("picture.png", 1).is_some());
	assert!(!page.snapshot.images.decoded().is_empty());
}

#[test]
fn an_offscreen_pending_image_does_not_hold_back_a_restored_tab() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	// Both pictures share one long quote block; only the first is visible.
	let source = format!(
		"> ![First](first.png)\n>\n{}> ![Second](second.png)\n",
		"> Paragraph.\n>\n".repeat(100)
	);
	let document = crate::loaded::LoadedDocument::markdown(
		std::sync::Arc::new(markview_commonmark::parse(source.as_str())),
	);
	let mut images = ImageSnapshot::default();
	for name in ["first.png", "second.png"] {
		images.keys.insert(
			ImageSource::Resource(ResourceRef::document(name)),
			name.into(),
		);
		images.entries.insert(
			name.into(),
			ImageInfo {
				loading: false,
				size: Some((10, 10)),
				..Default::default()
			},
		);
	}
	let reader = |images: &ImageSnapshot| crate::worker::ReaderSnapshot {
		layout: crate::layout::LayoutEngine::new().layout_with_images(
			&document,
			&crate::test_support::options(),
			images,
		),
		document: document.clone(),
		blocked_images: Default::default(),
		content_version: 1,
		complete: true,
		parse_complete: true,
		remote_deferred: 0,
	};
	let mut session = ReaderSession::default();
	session.accept(reader(&images), 200., None);
	session.scrolling.offset = 0.;
	let page = Page::capture(&session).unwrap();
	assert!(page.snapshot.images.entries.contains_key("first.png"));
	assert!(!page.snapshot.images.entries.contains_key("second.png"));
	// The tab returns before its full geometry is released.
	page.restore(&mut session);
	let mut pending = images.clone();
	pending.entries.insert(
		"second.png".into(),
		ImageInfo {
			loading: true,
			size: None,
			..Default::default()
		},
	);
	assert!(session.can_display(&reader(&pending), 200.));
	pending.entries.insert(
		"first.png".into(),
		ImageInfo {
			loading: true,
			size: None,
			..Default::default()
		},
	);
	assert!(!session.can_display(&reader(&pending), 200.));
}

#[test]
fn a_horizontally_offscreen_pending_image_does_not_hold_back_a_restored_tab() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let headers = (0..20).map(|_| "Column").collect::<Vec<_>>().join(" | ");
	let cells = (0..20)
		.map(|i| format!("![Image](image-{i}.png)"))
		.collect::<Vec<_>>()
		.join(" | ");
	let document = crate::loaded::LoadedDocument::markdown(
		std::sync::Arc::new(markview_commonmark::parse(format!(
			"# Images\n\n| {headers} |\n| {} |\n| {cells} |\n",
			vec!["---"; 20].join(" | ")
		))),
	);
	let mut images = ImageSnapshot::default();
	for i in 0..20 {
		let name = format!("image-{i}.png");
		images.keys.insert(
			ImageSource::Resource(ResourceRef::document(&name)),
			name.clone(),
		);
		images.entries.insert(
			name,
			ImageInfo {
				loading: false,
				size: Some((10, 10)),
				..Default::default()
			},
		);
	}
	let reader = |images: &ImageSnapshot| crate::worker::ReaderSnapshot {
		layout: crate::layout::LayoutEngine::new().layout_with_images(
			&document,
			&crate::test_support::options(),
			images,
		),
		document: document.clone(),
		blocked_images: Default::default(),
		content_version: 1,
		complete: true,
		parse_complete: true,
		remote_deferred: 0,
	};
	let mut session = ReaderSession::default();
	session.accept(reader(&images), 200., None);
	let table = session
		.display
		.snapshot
		.blocks
		.iter()
		.position(|b| !b.layout.overflow.is_empty())
		.unwrap();
	session.scrolling.offset = session.display.snapshot.blocks[table].y;
	let page = Page::capture(&session).unwrap();
	assert!(page.snapshot.images.entries.contains_key("image-0.png"));
	assert!(!page.snapshot.images.entries.contains_key("image-19.png"));
	// The tab returns before its full geometry is released.
	page.restore(&mut session);
	let mut pending = images.clone();
	pending.entries.insert(
		"image-19.png".into(),
		ImageInfo {
			loading: true,
			size: None,
			..Default::default()
		},
	);
	assert!(session.can_display(&reader(&pending), 200.));
	pending.entries.insert(
		"image-0.png".into(),
		ImageInfo {
			loading: true,
			size: None,
			..Default::default()
		},
	);
	assert!(!session.can_display(&reader(&pending), 200.));
}

#[test]
fn a_revived_tab_keeps_its_pinned_range_and_reading_position() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let source = format!(
		"{}![Image](picture.png)\n\n{}",
		"Paragraph.\n\n".repeat(100),
		"Tail.\n\n".repeat(100)
	);
	let document = crate::loaded::LoadedDocument::markdown(
		std::sync::Arc::new(markview_commonmark::parse(source.as_str())),
	);
	let mut images = ImageSnapshot::default();
	images.keys.insert(
		ImageSource::Resource(ResourceRef::document("picture.png")),
		"picture".into(),
	);
	images.entries.insert(
		"picture".into(),
		ImageInfo {
			loading: false,
			size: Some((100, 100)),
			..Default::default()
		},
	);
	let snapshot = crate::layout::LayoutEngine::new().layout_with_images(
		&document,
		&crate::test_support::options(),
		&images,
	);
	let reader = crate::worker::ReaderSnapshot {
		layout: snapshot,
		document,
		blocked_images: Default::default(),
		content_version: 1,
		complete: true,
		parse_complete: true,
		remote_deferred: 0,
	};
	let mut session = ReaderSession::default();
	session.accept(reader, 200., None);
	let image = session
		.display
		.snapshot
		.blocks
		.iter()
		.position(|b| {
			b.layout
				.draws
				.iter()
				.any(|draw| matches!(draw, Draw::Image { .. }))
		})
		.unwrap();
	session.scrolling.offset = session.display.snapshot.blocks[image].y;
	let page = Page::capture(&session).unwrap();
	let pinned = page.slice.range.clone();
	// The tab returns before its full geometry is released.
	page.restore(&mut session);
	let cached = session.display.cached_slice.as_ref().unwrap();
	assert_eq!(cached.range, pinned);
	assert!(pinned.end < session.display.snapshot.height);
	assert!(!cached.covers(
		session.display.snapshot.height - 200.,
		200.,
		session.display.snapshot.height
	));
	// Releasing the geometry still names the source text, not a pixel offset.
	session.release_heavy();
	assert!(session.saved_reading.is_some());
}

#[test]
fn discarding_a_cached_page_keeps_the_semantic_reading_position() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, ResourceRef,
	};
	let source = format!(
		"{}![Image](picture.png)\n\nUNIQUE TARGET paragraph.\n\n{}",
		"Filler paragraph long enough to wrap on a narrow page.\n\n".repeat(80),
		"Tail.\n\n".repeat(20)
	);
	let layout = |width: f32| {
		let document = crate::loaded::LoadedDocument::markdown(
			std::sync::Arc::new(markview_commonmark::parse(source.as_str())),
		);
		let mut images = ImageSnapshot::default();
		images.keys.insert(
			ImageSource::Resource(ResourceRef::document("picture.png")),
			"picture".into(),
		);
		images.entries.insert(
			"picture".into(),
			ImageInfo {
				loading: false,
				size: Some((100, 100)),
				..Default::default()
			},
		);
		let options = crate::layout::LayoutOptions {
			width,
			..crate::test_support::options()
		};
		crate::worker::ReaderSnapshot {
			layout: crate::layout::LayoutEngine::new()
				.layout_with_images(&document, &options, &images),
			document,
			blocked_images: Default::default(),
			content_version: 1,
			complete: true,
			parse_complete: true,
			remote_deferred: 0,
		}
	};
	let mut session = ReaderSession::default();
	session.accept(layout(800.), 200., None);
	let target = session
		.display
		.snapshot
		.blocks
		.iter()
		.position(|b| {
			b.layout
				.text
				.iter()
				.any(|n| n.text.contains("UNIQUE TARGET"))
		})
		.unwrap();
	session.scrolling.offset = session.display.snapshot.blocks[target].y + 5.;
	let page = Page::capture(&session).unwrap();
	// The tab returns before its geometry is released, keeping its document.
	page.restore(&mut session);
	assert!(session.display.cached_slice.is_some());
	session.discard_cached_page();
	assert!(session.saved_reading.is_some());
	session.accept(layout(400.), 200., None);
	let index = session
		.display
		.snapshot
		.blocks
		.partition_point(|b| b.y <= session.scrolling.offset)
		.saturating_sub(1);
	assert!(
		session.display.snapshot.blocks[index]
			.layout
			.text
			.iter()
			.any(|n| n.text.contains("UNIQUE TARGET"))
	);
}

#[test]
fn discarding_a_cached_page_waits_for_a_fragment_anchor() {
	use markview_layout::image::ImageInfo;
	let source = format!(
		"# Target\n\n{}![Image](picture.png)\n\n{}",
		"Filler paragraph with enough words to wrap across a narrow reading column.\n\n".repeat(80),
		"Trailing paragraph keeps the complete document beyond the initial prefix.\n\n".repeat(400),
	);
	let reader = |width| {
		picture_reader(
			&source,
			width,
			ImageInfo {
				size: Some((100, 100)),
				..Default::default()
			},
		)
	};
	for released in [false, true] {
		let mut session = ReaderSession::default();
		session.accept(reader(800.), 200., None);
		session.scrolling.offset = session.display.snapshot.blocks[81].y + 1.;
		let page = Page::capture(&session).unwrap();
		if released {
			session.release_heavy();
		}
		page.restore(&mut session);
		// A queued link replaces the old reading position before activation.
		session.saved_reading = None;
		let anchor = markview_document::AnchorId::Fragment("target".into());
		session.pending_anchor = Some(crate::state::PendingAnchor {
			target: anchor.clone(),
			revision: None,
		});
		session.discard_cached_page();
		assert!(!session.snapshot_complete);
		assert!(session.resolve_anchor(200.).is_none());
		let complete = reader(400.);
		let expected = complete.layout.anchor_target_y(&anchor).unwrap();
		let mut prefix = complete.clone();
		let end = prefix
			.layout
			.blocks
			.partition_point(|b| b.y < session.scrolling.offset + 200.);
		assert!(
			end < 81,
			"the first prefix must precede the old reading position"
		);
		prefix.layout.blocks.truncate(end);
		let last = prefix.layout.blocks.last().unwrap();
		prefix.layout.height = last.y + last.height();
		prefix.complete = false;
		assert!(session.can_display(&prefix, 200.));
		session.accept(prefix, 200., None);
		assert_eq!(session.resolve_anchor(200.), Some(Ok(())));
		assert!((session.scrolling.offset - expected).abs() < 0.01);
		session.accept(complete, 200., None);
		assert!(
			(session.scrolling.offset - expected).abs() < 0.01,
			"released={released}: target={expected}, actual={}",
			session.scrolling.offset
		);
		assert!(session.saved_reading.is_none());
	}
}

#[test]
fn overflowing_table_previews_pin_only_images_in_the_scrolled_clip() {
	use markview_layout::image::{ImageInfo, ImageSnapshot, Pixels};
	use std::sync::Arc;

	let headers = (0..20).map(|_| "Column").collect::<Vec<_>>().join(" | ");
	let cells = (0..20)
		.map(|i| format!("![Image](image-{i}.png)"))
		.collect::<Vec<_>>()
		.join(" | ");
	let document = Arc::new(markview_commonmark::parse(format!(
		"# Images\n\n{}| {headers} |\n| {} |\n| {cells} |\n\n{}",
		"Filler paragraph.\n\n".repeat(80),
		vec!["---"; 20].join(" | "),
		"Tail paragraph.\n\n".repeat(40)
	)));
	let mut images = ImageSnapshot::default();
	for i in 0..20 {
		let alias = format!("image-{i}.png");
		images.keys.insert(
			markview_layout::image::ImageSource::Resource(
				markview_layout::image::ResourceRef::document(alias.clone()),
			),
			alias.clone(),
		);
		images.entries.insert(
			alias.clone(),
			ImageInfo {
				loading: false,
				version: 1,
				size: Some((100, 100)),
				error: None,
				svg: false,
			},
		);
		images.pixels.insert(
			alias,
			1,
			Arc::new(Pixels {
				width: 1,
				height: 1,
				rgba: Arc::from([0, 0, 0, 255]),
			}),
		);
	}
	let options = crate::layout::LayoutOptions {
		width: 760.0,
		..crate::test_support::options()
	};
	let snapshot = crate::layout::LayoutEngine::new()
		.layout_with_images(&document, &options, &images);
	let (table_index, table) = snapshot
		.blocks
		.iter()
		.enumerate()
		.find(|(_, block)| !block.layout.overflow.is_empty())
		.unwrap();
	let overflow = &table.layout.overflow[0];
	assert!(overflow.content_width > overflow.rect.w * 2.0);
	for offset in [0.0, overflow.content_width - overflow.rect.w] {
		let (mut app, _events) = app();
		let now = Instant::now();
		app.readers.open("/tmp/table.md".into(), now);
		let reader = crate::worker::ReaderSnapshot {
			layout: snapshot.clone(),
			document: crate::loaded::LoadedDocument::markdown(document.clone()),
			blocked_images: Default::default(),
			content_version: 1,
			complete: true,
			parse_complete: true,
			remote_deferred: 0,
		};
		app.readers.session.accept(reader.clone(), 200., None);
		app.readers.session.scrolling.offset = table.y;
		app.readers
			.session
			.display
			.horizontal
			.insert((table_index, 0), offset);
		app.readers.open("/tmp/second.md".into(), now);
		app.readers.release_inactive(now + Duration::from_secs(21));
		let page = app.readers.entries()[0].preview.as_ref().unwrap();
		let retained = page.snapshot.images.decoded();
		assert_eq!(retained.len(), 7);
		assert_eq!(page.snapshot.images.entries.len(), retained.len());
		assert_eq!(retained.contains_key("image-0.png"), offset == 0.0);
		assert_eq!(retained.contains_key("image-19.png"), offset != 0.0);

		// Restoring the slice and then the full layout keeps the table's offset.
		assert!(page.slice.first > 0);
		assert!(app.readers.select(0, now));
		let session = &mut app.readers.session;
		let first = session.display.cached_slice.as_ref().unwrap().first;
		assert_eq!(session.display.viewport, 200.);
		assert_eq!(
			session.display.horizontal[&(table_index - first, 0)],
			offset
		);
		assert!(session.can_display(&reader, 200.));
		session.accept(reader, 200., None);
		assert!(session.display.cached_slice.is_none());
		assert_eq!(session.display.horizontal[&(table_index, 0)], offset);
		assert!((session.scrolling.offset - table.y).abs() < 0.01);
	}
}

/// The wheel slides tabs under a resting pointer, so the card closes and its
/// hover follows the tab that arrives without waiting for pointer motion.
#[test]
fn scrolling_the_tab_strip_rearms_the_hover() {
	let (mut app, _events) = app();
	let now = Instant::now();
	for index in 0..30 {
		app.readers
			.open(PathBuf::from(format!("/tmp/scroll-tab-{index}.md")), now);
	}
	let layout = app.tab_layout();
	assert!(layout.max_scroll > 0.0);
	app.interaction
		.pointer
		.move_to((layout.rects[0].x + 5.0, 20.0));
	app.hover_preview(now);
	app.preview_tick(now + SHOW_DELAY);
	let before = app.preview.open().expect("the card did not open").target;

	assert!(app.scroll_tabs(200.0));
	assert!(
		app.preview.open().is_none(),
		"the card must close while tabs slide under it"
	);
	assert!(
		app.preview.deadline().is_some(),
		"the strip must re-arm the hover without motion"
	);
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert!(!app.control_scroll_animating());
	let target = app
		.target_at_cursor()
		.expect("a tab rests under the pointer");
	assert_ne!(target, before, "the strip scrolled to another tab");
	app.preview_tick(Instant::now() + Duration::from_secs(2));
	assert_eq!(app.preview.open().map(|open| open.target), Some(target));
}

/// Losing focus dismisses the card, and a strip still easing must not arm it
/// again in the inactive window.
#[test]
fn scrolling_does_not_reopen_the_card_in_an_unfocused_window() {
	let (mut app, _events) = app();
	let now = Instant::now();
	for index in 0..30 {
		app.readers
			.open(PathBuf::from(format!("/tmp/unfocused-tab-{index}.md")), now);
	}
	let layout = app.tab_layout();
	app.interaction
		.pointer
		.move_to((layout.rects[0].x + 5.0, 20.0));
	app.hover_preview(now);
	app.preview_tick(now + SHOW_DELAY);
	assert!(app.preview.open().is_some());

	app.scroll_tabs(200.0);
	app.frame.focused = false;
	app.dismiss_preview();
	app.advance_control_scroll(Instant::now() + Duration::from_secs(1));
	assert!(!app.preview.tracking());
	app.preview_tick(Instant::now() + Duration::from_secs(2));
	assert!(app.preview.open().is_none());
}

#[test]
fn oversized_geometry_uses_the_slow_path_without_pinning_a_preview() {
	use std::sync::Arc;
	let (mut app, _events) = app();
	let now = Instant::now();
	app.readers.open("/tmp/large.md".into(), now);
	let document = crate::loaded::LoadedDocument::markdown(Arc::new(
		markview_commonmark::parse("Code."),
	));
	let mut layout = crate::layout::LayoutEngine::new()
		.layout(&document, &crate::test_support::options());
	Arc::make_mut(&mut layout.blocks[0].layout)
		.draws
		.reserve(CACHE_BUDGET / std::mem::size_of::<Draw>() + 1);
	let weak = Arc::downgrade(&layout.blocks[0].layout);
	app.readers.session.document = Some(document);
	app.readers.session.display.snapshot = layout;
	app.readers.open("/tmp/other.md".into(), now);
	assert!(app.readers.entries()[0].preview.is_none());
	assert!(
		app.readers.entries()[0]
			.session
			.display
			.snapshot
			.blocks
			.is_empty()
	);
	assert!(weak.upgrade().is_none());
	app.readers.select(0, now);
	assert!(app.readers.session.display.snapshot.blocks.is_empty());
}

#[test]
fn cache_budget_evicts_the_oldest_preview_and_its_pixels() {
	use markview_layout::image::{
		ImageInfo, ImageSnapshot, ImageSource, Pixels, ResourceRef,
	};
	use std::sync::Arc;
	let (mut app, _events) = app();
	let now = Instant::now();
	let mut first = None;
	for i in 0..12 {
		app.readers.open(
			format!("/tmp/picture-{i}.md").into(),
			now + Duration::from_secs(i),
		);
		let document = crate::loaded::LoadedDocument::markdown(Arc::new(
			markview_commonmark::parse("![Picture](picture.png)"),
		));
		let pixels = Arc::new(Pixels {
			width: 1024,
			height: 1536,
			rgba: vec![0; 1024 * 1536 * 4].into(),
		});
		if i == 0 {
			first = Some(Arc::downgrade(&pixels));
		}
		let mut images = ImageSnapshot::default();
		images.keys.insert(
			ImageSource::Resource(ResourceRef::document("picture.png")),
			"picture.png".into(),
		);
		images.entries.insert(
			"picture.png".into(),
			ImageInfo {
				size: Some((1024, 1536)),
				..Default::default()
			},
		);
		images.pixels.insert("picture.png".into(), 0, pixels);
		app.readers.session.display.snapshot =
			crate::layout::LayoutEngine::new().layout_with_images(
				&document,
				&crate::test_support::options(),
				&images,
			);
		app.readers.session.document = Some(document);
	}
	assert!(app.readers.entries()[0].preview.is_none());
	assert!(first.unwrap().upgrade().is_none());
	assert!(
		app.readers
			.entries()
			.iter()
			.filter_map(|t| t.preview.as_ref())
			.map(Page::bytes)
			.sum::<usize>()
			<= CACHE_BUDGET
	);
	assert!(app.readers.entries()[10].preview.is_some());
}

#[test]
fn changed_content_and_geometry_discard_the_resume_snapshot() {
	let (mut app, events) = app();
	let dir = tempfile::tempdir().unwrap();
	let first = dir.path().join("first.md");
	let second = dir.path().join("second.md");
	std::fs::write(&first, "Paragraph.\n\n".repeat(200)).unwrap();
	std::fs::write(&second, "Other.").unwrap();
	load(&mut app, &events, first.clone());
	load(&mut app, &events, second);
	app.readers
		.release_inactive(Instant::now() + Duration::from_secs(21));
	std::fs::write(&first, "Changed content.\n\n".repeat(200)).unwrap();
	app.select_tab(0);
	assert!(app.readers.session.display.cached_slice.is_none());
	settle(&mut app, &events);
	assert!(
		app.readers
			.session
			.display
			.snapshot
			.full_reading_text()
			.contains("Changed content")
	);
	assert_eq!(
		app.readers.session.counts_content_id,
		Some(app.readers.session.accepted_content_id)
	);
	app.select_tab(1);
	settle(&mut app, &events);
	app.readers
		.release_inactive(Instant::now() + Duration::from_secs(21));
	app.preferences.values.font_size += 2.;
	app.select_tab(0);
	assert!(app.readers.session.display.cached_slice.is_none());
}
