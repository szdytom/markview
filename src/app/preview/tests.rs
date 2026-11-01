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
	assert!(!app.readers.session.snapshot.blocks.is_empty());
	assert!(!app.readers.entries()[0].session.snapshot.blocks.is_empty());

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
	std::fs::write(&first, "Reading paragraph.\n\n".repeat(100)).unwrap();
	std::fs::write(&second, "# Second\n").unwrap();
	load(&mut app, &events, first);
	let block = 12;
	app.readers.session.scrolling.offset =
		app.readers.session.snapshot.blocks[block].y + 5.0;
	app.readers.session.horizontal.insert((block, 0), 24.0);
	let original = app.readers.session.snapshot.clone();
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
		assert!(session.snapshot.blocks.is_empty());
		assert!(app.readers.session.document.is_some());
		app.overlay();
		let scene = app.preview.scene(&app.readers, Theme::Light).unwrap();
		assert_eq!(scene.scroll, scroll);
		assert_eq!(scene.snapshot.width, original.width);
		assert!(scene.snapshot.blocks.len() < original.blocks.len());
		assert_eq!(scene.snapshot.blocks[0].id, original.blocks[block].id);
		assert_eq!(scene.horizontal.get(&(0, 0)), Some(&24.0));
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
		images.entries.insert(
			alias.into(),
			ImageInfo {
				version: 7,
				size: Some((1, 1)),
				error: None,
			},
		);
		images.pixels.insert(alias.into(), 7, pixels.clone());
	}
	app.readers.session.snapshot = crate::layout::LayoutEngine::new()
		.layout_with_images(
			&document,
			&crate::test_support::options(),
			&images,
		);
	app.readers.session.document = Some(document);
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
	images.entries.insert(
		"picture.png".into(),
		ImageInfo {
			version: 1,
			size: Some((100, 140)),
			error: None,
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
	app.readers.session.snapshot = snapshot;
	app.readers.session.document = Some(document);
	app.readers.open("/tmp/second.md".into(), now);
	app.readers.release_inactive(now + Duration::from_secs(21));

	let page = app.readers.entries()[0].preview.as_ref().unwrap();
	assert_eq!(page.snapshot.blocks.len(), 1);
	assert!(page.snapshot.images.pixels.get("picture.png", 1).is_some());
	assert!(!page.snapshot.images.decoded().is_empty());
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
		"# Images\n\n| {headers} |\n| {} |\n| {cells} |\n",
		vec!["---"; 20].join(" | ")
	)));
	let mut images = ImageSnapshot::default();
	for i in 0..20 {
		let alias = format!("image-{i}.png");
		images.entries.insert(
			alias.clone(),
			ImageInfo {
				version: 1,
				size: Some((100, 100)),
				error: None,
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
		app.readers.session.snapshot = snapshot.clone();
		app.readers.session.document = Some(document.clone());
		app.readers.session.scrolling.offset = table.y;
		app.readers
			.session
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
