use super::*;
use crate::app::Event;
use crate::{
	cli::{LaunchOptions, Mode},
	layout::LayoutEngine,
};
use std::{
	sync::mpsc,
	time::{Duration, Instant},
};

#[derive(Clone)]
struct Proxy(mpsc::Sender<Event>);
impl SendEvent for Proxy {
	fn send(&self, event: Event) {
		let _ = self.0.send(event);
	}
}
struct Harness {
	app: App<Proxy>,
	events: mpsc::Receiver<Event>,
	dir: tempfile::TempDir,
}
impl Harness {
	fn new(source: &str) -> Self {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("one.md");
		std::fs::write(&path, source).unwrap();
		// Match the canonical tab identity used by `App::open`.
		let path = std::fs::canonicalize(path).unwrap();
		let (tx, events) = mpsc::channel();
		let mut app = App::new(
			LaunchOptions {
				mode: Mode::Smoke,
				options: crate::test_support::options(),
				..Default::default()
			},
			Proxy(tx),
		);
		app.ui = crate::test_support::shaper();
		app.preferences.values = crate::settings::ReaderSettings::default();
		app.readers.open(path, Instant::now());
		let document = Arc::new(markview_core::document::parse(source));
		let options = app.options();
		let snapshot = LayoutEngine::new().layout(&document, &options);
		let session = &mut app.readers.session;
		session.document = Some(document.clone());
		session.search.document = Some(document);
		session.snapshot = snapshot;
		session.content_version = 1;
		session.accepted_revision = 1;
		session.parse_complete = true;
		session.snapshot_complete = true;
		session.requested_options = Some(options);
		Self { app, events, dir }
	}
	fn query(&mut self, query: &str) {
		self.app.open_search();
		self.app
			.readers
			.session
			.search
			.input
			.set_text(&mut self.app.ui, query);
		self.app.search_changed();
		self.settle();
	}
	fn settle(&mut self) {
		let deadline = Instant::now() + Duration::from_secs(10);
		while self.app.readers.session.search.preparing
			|| self.app.readers.session.search.pending_navigation
		{
			assert!(
				Instant::now() < deadline,
				"search or navigation did not settle"
			);
			let event =
				self.events.recv_timeout(Duration::from_secs(10)).unwrap();
			match event {
				Event::SearchReady(result) => self.app.search_ready(result),
				Event::Parsed {
					path,
					content_version,
					document,
				} if self.app.readers.session.path.as_ref() == Some(&path)
					&& self.app.readers.session.content_version
						== content_version =>
				{
					self.app.readers.session.parse_complete = true;
					self.app.readers.session.search.document = Some(document);
					self.app.search_tick();
				}
				Event::Ready(mut update)
					if update.version == self.app.readers.session.version =>
				{
					if let Some(Ok(reader)) = update.result.take() {
						let viewport = self.app.viewport();
						self.app.readers.session.accept(
							reader,
							viewport,
							update.counts,
						);
						self.app.readers.session.displayed_version =
							update.version;
						self.app.apply_search_navigation();
					}
				}
				_ => {}
			}
		}
	}
}
#[test]
fn edits_submit_immediately_and_latest_query_wins_without_a_timer() {
	let mut h = Harness::new("needle and needles");
	h.app.open_search();
	for query in ["n", "ne", "nee", "needle", "needles"] {
		h.app
			.readers
			.session
			.search
			.input
			.set_text(&mut h.app.ui, query);
		h.app.search_changed();
		let search = &h.app.readers.session.search;
		assert_eq!(search.query, query);
		assert!(!search.dirty, "the request must already be submitted");
		assert!(search.preparing);
	}
	h.settle();
	assert_eq!(h.app.readers.session.search.matches.len(), 1);
	h.app
		.readers
		.session
		.search
		.input
		.set_text(&mut h.app.ui, "");
	h.app.search_changed();
	h.settle();
	assert!(h.app.readers.session.search.matches.is_empty());
	assert!(h.app.draw_search_highlights().is_empty());
}

#[test]
fn live_query_preserves_scroll_selection_and_disclosures() {
	let source = format!(
		"{}\n\n<details>\n<summary>hidden</summary>\n\nneedle\n\n</details>\n",
		"needle and **needle**.\n\n".repeat(80)
	);
	let mut h = Harness::new(&source);
	h.app.readers.session.scroll = 800.0;
	let selection = h.app.readers.session.snapshot.select_all(1).unwrap();
	h.app.interaction.selection = Some(selection);
	let copied = h.app.readers.session.snapshot.extract_text(selection, 1);
	h.query("needle");
	assert_eq!(h.app.readers.session.search.matches.len(), 161);
	assert_eq!(h.app.readers.session.search.current, None);
	assert_eq!(h.app.readers.session.scroll, 800.0);
	assert!(h.app.readers.session.details_open.is_empty());
	assert_eq!(h.app.interaction.selection, Some(selection));
	assert_eq!(
		h.app.readers.session.snapshot.extract_text(selection, 1),
		copied
	);
	assert!(!h.app.draw_search_highlights().is_empty());
	assert_eq!(h.app.bottom(), 44.0);
	let drawer = h.app.outline_drawer();
	assert_eq!(drawer.y + drawer.h, h.app.dimensions().1 - h.app.bottom());
	h.app.close_search();
	assert!(h.app.draw_search_highlights().is_empty());
	assert_eq!(h.app.bottom(), 28.0);
	assert_eq!(h.app.readers.session.scroll, 800.0);
	assert_eq!(h.app.readers.session.search.input.text(), "needle");
}
#[test]
fn navigation_starts_near_viewport_wraps_and_opens_only_ancestors() {
	let source = format!(
		"{}\n<details>\n<summary>outer</summary>\n\n<details>\n<summary>inner</summary>\n\nneedle\n\n</details>\n\n<details>\n<summary>unrelated</summary>\n\nother\n\n</details>\n</details>\n",
		"needle.\n\n".repeat(40)
	);
	let mut h = Harness::new(&source);
	h.app.readers.session.scroll = 600.0;
	h.query("needle");
	h.app.navigate_search(false);
	let first = h.app.readers.session.search.current.unwrap();
	assert!(first > 0);
	h.app.readers.session.search.current = Some(39);
	h.app.navigate_search(false);
	h.settle();
	assert_eq!(h.app.readers.session.search.current, Some(40));
	assert_eq!(h.app.readers.session.details_open.len(), 2);
	let hit = &h.app.readers.session.search.matches[40];
	assert!(
		h.app
			.readers
			.session
			.snapshot
			.search_selection(hit, 1)
			.is_some()
	);
	h.app.navigate_search(false);
	assert_eq!(h.app.readers.session.search.current, Some(0));
	h.app.navigate_search(true);
	assert_eq!(h.app.readers.session.search.current, Some(40));
	h.app.close_search();
	assert_eq!(h.app.readers.session.details_open.len(), 2);
}
#[test]
fn horizontal_navigation_reveals_code_and_table_targets() {
	let source = format!(
		"```\n{}needle\n```\n\n{} needle |\n{}---|\n",
		"x".repeat(600),
		"| X ".repeat(20),
		"|---".repeat(20)
	);
	let mut h = Harness::new(&source);
	h.app.preferences.values.width = 300.0;
	let options = h.app.options();
	h.app.readers.session.snapshot = LayoutEngine::new()
		.layout(h.app.readers.session.document.as_ref().unwrap(), &options);
	h.app.readers.session.requested_options = Some(options);
	h.query("needle");
	assert_eq!(h.app.readers.session.search.matches.len(), 2);
	for index in [0, 1] {
		h.app.readers.session.search.current =
			Some(if index == 0 { 1 } else { 0 });
		h.app.navigate_search(false);
		let session = &h.app.readers.session;
		let hit = &session.search.matches[index];
		let selection = session.snapshot.search_selection(hit, 1).unwrap();
		let rects =
			session
				.snapshot
				.selection_rects(selection, &session.horizontal, 1);
		assert!(!rects.is_empty());
		assert!(
			session
				.horizontal
				.iter()
				.any(|((b, _), offset)| *b == hit.block && *offset > 0.0)
		);
	}
}
#[test]
fn horizontal_navigation_reveals_rtl_anchor() {
	let source =
		format!("```\n{}مرحبا{}\n```", "س".repeat(100), "س".repeat(100));
	let mut h = Harness::new(&source);
	h.app.preferences.values.width = 300.0;
	let options = h.app.options();
	h.app.readers.session.snapshot = LayoutEngine::new()
		.layout(h.app.readers.session.document.as_ref().unwrap(), &options);
	h.app.readers.session.requested_options = Some(options);
	h.query("مرحبا");
	h.app.navigate_search(false);
	let session = &h.app.readers.session;
	let hit = &session.search.matches[0];
	let selection = session.snapshot.search_selection(hit, 1).unwrap();
	let block = &session.snapshot.blocks[selection.anchor.block];
	let node = &block.layout.text[selection.anchor.node];
	let anchor = node
		.clusters
		.iter()
		.find(|c| c.range.contains(&selection.anchor.offset))
		.unwrap();
	assert!(anchor.rtl);
	let (oi, overflow) = block
		.layout
		.overflow
		.iter()
		.enumerate()
		.find(|(_, o)| o.commands.contains(&anchor.command))
		.unwrap();
	let offset = session
		.horizontal
		.get(&(hit.block, oi))
		.copied()
		.unwrap_or_default();
	assert!(offset > 0.0);
	assert!(anchor.rect.x - offset >= overflow.rect.x - 0.5);
	assert!(
		anchor.rect.x + anchor.rect.w - offset
			<= overflow.rect.x + overflow.rect.w + 0.5
	);
}
#[test]
fn parsed_full_document_searches_before_layout_and_rejects_stale_results() {
	let mut h = Harness::new("first\n\nneedle\n");
	h.app.readers.session.snapshot.blocks.truncate(1);
	h.app.readers.session.snapshot_complete = false;
	h.query("needle");
	assert_eq!(h.app.readers.session.search.matches.len(), 1);
	let result = || Result {
		path: h.app.readers.session.path.clone().unwrap(),
		content: 1,
		sequence: h.app.readers.session.search.sequence,
		matches: Arc::default(),
	};
	let mut stale_query = result();
	stale_query.sequence -= 1;
	let mut stale_file = result();
	stale_file.content += 1;
	let mut stale_tab = result();
	stale_tab.path = h.dir.path().join("other.md");
	for stale in [stale_query, stale_file, stale_tab] {
		h.app.search_ready(stale);
		assert_eq!(h.app.readers.session.search.matches.len(), 1);
	}
	h.app.readers.session.content_version += 1;
	assert!(h.app.draw_search_highlights().is_empty());
	let document = Arc::new(markview_core::document::parse("needle needle"));
	h.app.readers.session.search.document = Some(document);
	h.app.search_changed();
	h.app.search_tick();
	h.settle();
	assert_eq!(h.app.readers.session.search.matches.len(), 2);
}
#[test]
fn search_state_belongs_to_each_tab_and_panels_preserve_query() {
	let mut h = Harness::new("needle");
	h.query("needle");
	h.app.navigate_search(false);
	h.app.action(Command::SearchWord);
	let one = h.app.readers.session.path.clone().unwrap();
	let two = h.dir.path().join("two.md");
	std::fs::write(&two, "second needle").unwrap();
	h.app.open(two);
	assert!(!h.app.readers.session.search.open);
	assert!(h.app.readers.session.search.input.text().is_empty());
	let first = h.app.readers.find(&one).unwrap();
	h.app.select_tab(first);
	assert!(!h.app.readers.session.search.open);
	assert_eq!(h.app.readers.session.search.input.text(), "needle");
	assert!(h.app.readers.session.search.options.whole_word);
	assert_eq!(h.app.readers.session.search.current, Some(0));
	h.app.open_search();
	h.app.action(Command::Settings);
	assert!(!h.app.readers.session.search.open);
	h.app.open_search();
	assert!(!h.app.interaction.panel_open());
	assert_eq!(
		h.app.interaction.focus,
		Some(Command::FocusInput(TextField::Search))
	);
	h.app.action(Command::Export);
	assert!(!h.app.readers.session.search.open);
	assert_eq!(h.app.readers.session.search.input.text(), "needle");
}
#[test]
fn document_switches_close_search_and_keep_each_query() {
	let mut h = Harness::new("needle");
	h.query("needle");
	let one = h.app.readers.session.path.clone().unwrap();
	h.app.select_tab(0);
	assert!(h.app.readers.session.search.open);
	let two = h.dir.path().join("two.md");
	std::fs::write(&two, "second needle").unwrap();
	h.app.open(two.clone());
	let two = std::fs::canonicalize(two).unwrap();
	assert!(!h.app.readers.session.search.open);
	h.query("second");
	h.app.open(one.clone());
	assert!(!h.app.readers.session.search.open);
	assert_eq!(h.app.readers.session.search.input.text(), "needle");
	assert!(h.app.interaction.focus.is_none());
	assert!(h.app.draw_search_highlights().is_empty());
	h.app.open_search();
	h.settle();
	assert_eq!(h.app.readers.session.search.matches.len(), 1);
	h.app.select_tab(h.app.readers.find(&two).unwrap());
	assert!(!h.app.readers.session.search.open);
	assert_eq!(h.app.readers.session.search.input.text(), "second");
	h.app.open_search();
	h.app.close_tab(h.app.readers.active());
	assert_eq!(h.app.readers.session.path.as_ref(), Some(&one));
	assert!(!h.app.readers.session.search.open);
	assert_eq!(h.app.readers.session.search.input.text(), "needle");
	h.app.open_search();
	let background = h.dir.path().join("background.md");
	assert!(h.app.readers.open_background(background.clone(), None));
	h.app.close_tab(h.app.readers.find(&background).unwrap());
	assert!(h.app.readers.session.search.open);
	h.app.close_tab(h.app.readers.active());
	assert!(!h.app.readers.session.search.open);
}

#[test]
fn worker_latest_request_wins_after_cancellation() {
	let (tx, rx) = mpsc::channel();
	let worker = Worker::new(move |result| {
		let _ = tx.send(result);
	});
	let document =
		Arc::new(markview_core::document::parse("needle ".repeat(20000)));
	let old = worker.cancel();
	worker.submit(Request {
		path: "one".into(),
		content: 1,
		sequence: old,
		document: document.clone(),
		query: "needle".into(),
		options: SearchOptions::default(),
	});
	let index_generation = worker.index_generation.load(Ordering::Relaxed);
	let latest = worker.cancel();
	assert_eq!(
		worker.index_generation.load(Ordering::Relaxed),
		index_generation
	);
	worker.submit(Request {
		path: "one".into(),
		content: 1,
		sequence: latest,
		document,
		query: "absent".into(),
		options: SearchOptions::default(),
	});
	assert_eq!(
		worker.index_generation.load(Ordering::Relaxed),
		index_generation,
		"typing must preserve in-progress indexing"
	);
	loop {
		let result = rx.recv_timeout(Duration::from_secs(10)).unwrap();
		if result.sequence == latest {
			assert!(result.matches.is_empty());
			break;
		}
	}
	let reloaded = worker.cancel();
	worker.submit(Request {
		path: "one".into(),
		content: 2,
		sequence: reloaded,
		document: Arc::new(markview_core::document::parse("needle")),
		query: "needle".into(),
		options: SearchOptions::default(),
	});
	assert!(worker.index_generation.load(Ordering::Relaxed) > index_generation);
	let result = rx.recv_timeout(Duration::from_secs(10)).unwrap();
	assert_eq!(result.sequence, reloaded);
	assert_eq!(result.matches.len(), 1);
}

#[test]
#[ignore = "requires a GPU; writes artifacts/search/*.png"]
fn search_bar_and_highlight_gpu_frames() -> anyhow::Result<()> {
	use crate::{
		lang::Lang,
		render::{Renderer, Theme, View},
	};
	for (name, dark, lang, scale) in [
		("light", false, Lang::En, 1.0),
		("dark", true, Lang::ZhHans, 1.0),
		("traditional-2x", false, Lang::ZhHant, 2.0),
		("japanese", false, Lang::Ja, 1.25),
	] {
		let mut h = Harness::new(
			"# Find in this document\n\nSearch for **needle** in this paragraph. Another needle is here.\n\n```rust\nlet needle = 42;\n```\n\n| Name | Value |\n|---|---|\n| needle | 42 |\n\n<details>\n<summary>Collapsed content</summary>\n\nneedle\n\n</details>\n",
		);
		let stylesheet = markview_core::style::Stylesheet::bundled(dark);
		h.app.preferences.values.stylesheet = stylesheet.clone();
		h.app.preferences.values.lang = Some(lang);
		h.app.ui.stylesheet = stylesheet.clone();
		h.query("needle");
		h.app.navigate_search(false);
		let selection = h
			.app
			.readers
			.session
			.snapshot
			.search_selection(&h.app.readers.session.search.matches[1], 1);
		h.app.interaction.selection = selection;
		let overlay = h.app.overlay();
		let session = &h.app.readers.session;
		let view = View {
			selection,
			revision: 1,
			width: (1200.0 * scale) as u32,
			height: (800.0 * scale) as u32,
			scale,
			scroll: session.scroll,
			left: h.app.view_geometry().left,
			top: h.app.content_top() + 10.0,
			bottom: HEIGHT + 10.0,
			theme: if dark { Theme::Dark } else { Theme::Light },
			horizontal: &session.horizontal,
			hovered_link: None,
			hovered_overflow: None,
			held_overflow: None,
		};
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		renderer.set_stylesheet(stylesheet);
		let texture = renderer.offscreen(view.width, view.height);
		let submission = renderer.render(
			&session.snapshot,
			&view,
			&overlay,
			&texture.create_view(&Default::default()),
		)?;
		renderer.wait(Some(submission))?;
		let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join(format!("artifacts/search/{name}.png"));
		std::fs::create_dir_all(path.parent().unwrap())?;
		renderer.save_png(&texture, &path)?;
	}
	Ok(())
}
