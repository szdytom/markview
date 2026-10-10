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
	fn try_send(&self, event: Event) -> bool {
		self.0.send(event).is_ok()
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
		let mut engine = LayoutEngine::new();
		let mut snapshot = engine.layout(&document, &options);
		// The harness has no frame loop to accept later syntax colors.
		if engine.wait_highlights() {
			snapshot = engine.layout(&document, &options);
		}
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
fn slash_opens_empty_search_without_using_selection_or_previous_results() {
	use winit::keyboard::Key;
	let mut h = Harness::new("needle");
	let slash = Key::Character("/".into());
	assert!(h.app.key_pressed(&slash));
	h.settle();
	assert!(h.app.readers.session.search.open);
	assert!(h.app.readers.session.search.input.text().is_empty());
	assert_eq!(
		h.app.interaction.focus,
		Some(Command::FocusInput(TextField::Search))
	);
	for already_open in [false, true] {
		h.query("needle");
		h.app.navigate_search(false);
		h.settle();
		assert_eq!(h.app.readers.session.search.current, Some(0));
		if !already_open {
			h.app.close_search();
		}
		let selection = h.app.readers.session.snapshot.select_all(1);
		h.app.interaction.selection = selection;
		h.app.readers.session.search.retained =
			Some(h.app.readers.session.search.matches[0].clone());
		assert!(h.app.key_pressed(&slash));
		let search = &h.app.readers.session.search;
		assert!(search.open);
		assert!(search.input.text().is_empty());
		assert!(search.query.is_empty());
		assert!(search.matches.is_empty());
		assert_eq!(search.current, None);
		assert_eq!(search.retained, None);
		assert_eq!(h.app.interaction.selection, selection);
		h.settle();
		assert!(h.app.readers.session.search.matches.is_empty());
		h.app.interaction.selection = None;
	}
}

#[test]
fn slash_respects_panels_modals_and_modifier_chords() {
	use crate::state::Modal;
	use winit::keyboard::{Key, ModifiersState};
	let mut h = Harness::new("needle");
	let slash = Key::Character("/".into());
	for modifiers in [
		ModifiersState::CONTROL,
		ModifiersState::SUPER,
		ModifiersState::ALT,
	] {
		h.app.interaction.modifiers = modifiers;
		assert!(!h.app.key_pressed(&slash));
		assert!(!h.app.readers.session.search.open);
	}
	h.app.interaction.modifiers = ModifiersState::empty();
	for panel in [
		PanelPage::Settings(crate::state::PanelTab::Generic),
		PanelPage::Export,
	] {
		h.app.interaction.show_panel(panel);
		assert!(h.app.key_pressed(&slash));
		assert!(!h.app.readers.session.search.open);
	}
	h.app.interaction.show_panel(PanelPage::Closed);
	h.app.interaction.modal = Some(Modal::OpenLocal {
		path: "file.txt".into(),
		dir: ".".into(),
		document_dir: None,
	});
	h.app.key_pressed(&slash);
	assert!(!h.app.readers.session.search.open);
	assert!(h.app.interaction.modal.is_some());
	h.app.interaction.modal = None;
	h.app.readers.session.path = None;
	h.app.key_pressed(&slash);
	assert!(!h.app.readers.session.search.open);
}

#[test]
fn opening_search_uses_document_selection_and_submits_it() {
	let mut h = Harness::new("选中文字");
	for already_open in [false, true] {
		h.query("previous");
		if !already_open {
			h.app.close_search();
		}
		let mut selection =
			h.app.readers.session.snapshot.select_all(1).unwrap();
		if already_open {
			std::mem::swap(&mut selection.anchor, &mut selection.focus);
		}
		h.app.interaction.selection = Some(selection);
		h.app.open_search();
		h.settle();
		let search = &h.app.readers.session.search;
		assert!(search.open);
		assert_eq!(search.input.text(), "选中文字");
		assert_eq!(search.input.selected_text(), Some("选中文字"));
		assert_eq!(search.query, "选中文字");
		assert_eq!(search.matches.len(), 1);
		assert_eq!(h.app.interaction.selection, Some(selection));
		assert_eq!(
			h.app.interaction.focus,
			Some(Command::FocusInput(TextField::Search))
		);
		h.app.interaction.selection = None;
	}
}

#[test]
fn opening_search_without_valid_selected_text_preserves_query() {
	let mut h = Harness::new("needle");
	h.query("needle");
	let stale = h.app.readers.session.snapshot.select_all(0).unwrap();
	let mut empty = h.app.readers.session.snapshot.select_all(1).unwrap();
	empty.focus = empty.anchor;
	for selection in [None, Some(empty), Some(stale)] {
		h.app.close_search();
		h.app.interaction.selection = selection;
		h.app.open_search();
		h.settle();
		assert_eq!(h.app.readers.session.search.input.text(), "needle");
		assert_eq!(h.app.readers.session.search.matches.len(), 1);
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
	h.app.readers.session.scrolling.offset = 800.0;
	let selection = h.app.readers.session.snapshot.select_all(1).unwrap();
	h.app.interaction.selection = Some(selection);
	let copied = h.app.readers.session.snapshot.extract_text(selection, 1);
	h.query("needle");
	assert_eq!(h.app.readers.session.search.matches.len(), 161);
	assert_eq!(h.app.readers.session.search.current, None);
	assert_eq!(h.app.readers.session.scrolling.offset, 800.0);
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
	assert_eq!(h.app.readers.session.scrolling.offset, 800.0);
	assert_eq!(h.app.readers.session.search.input.text(), "needle");
}
#[test]
fn navigation_starts_near_viewport_wraps_and_opens_only_ancestors() {
	let source = format!(
		"{}\n<details>\n<summary>outer</summary>\n\n<details>\n<summary>inner</summary>\n\nneedle\n\n</details>\n\n<details>\n<summary>unrelated</summary>\n\nother\n\n</details>\n</details>\n",
		"needle.\n\n".repeat(40)
	);
	let mut h = Harness::new(&source);
	h.app.readers.session.scrolling.offset = 600.0;
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
	let index_generation = worker.control.generation.load(Ordering::Relaxed);
	let latest = worker.cancel();
	assert_eq!(
		worker.control.generation.load(Ordering::Relaxed),
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
		worker.control.generation.load(Ordering::Relaxed),
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
	assert!(
		worker.control.generation.load(Ordering::Relaxed) > index_generation
	);
	let result = rx.recv_timeout(Duration::from_secs(10)).unwrap();
	assert_eq!(result.sequence, reloaded);
	assert_eq!(result.matches.len(), 1);
}

#[test]
fn search_states_match_rendering_baselines() -> anyhow::Result<()> {
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
		let mut stylesheet =
			(*markview_core::style::Stylesheet::bundled(dark)).clone();
		stylesheet.set_cjk_type(markview_core::style::CjkType::Sc);
		stylesheet.merge(&markview_core::style::Stylesheet::parse("format_version=2\nversion=1\n[[rule]]\nwhen=['search']\nbackground='#E9BA4550'\n[[rule]]\nwhen=['search_current']\nbackground='#E49B2390'\n[[rule]]\nwhen=['selection']\nbackground='#315D8650'")?);
		let stylesheet = Arc::new(stylesheet);
		h.app.preferences.values.stylesheet = stylesheet.clone();
		h.app.preferences.values.lang = Some(lang);
		h.app.ui.stylesheet = stylesheet.clone();
		h.query("needle");
		h.app.navigate_search(false);
		assert!(
			h.app.readers.session.snapshot.blocks[2].draws().any(
				|(_, draw, _)| matches!(draw, Draw::Glyph(glyph) if matches!(glyph.paint, Paint::Color(_)))
			),
			"Search snapshots must include settled syntax highlighting"
		);
		let selection = h
			.app
			.readers
			.session
			.snapshot
			.search_selection(&h.app.readers.session.search.matches[1], 1);
		h.app.interaction.selection = selection;
		h.app.interaction.focus = None;
		let overlay = h.app.overlay();
		let session = &h.app.readers.session;
		let view = View {
			selection,
			revision: 1,
			width: (1200.0 * scale) as u32,
			height: (800.0 * scale) as u32,
			scale,
			scroll: session.scrolling.offset,
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
		#[cfg(target_os = "linux")]
		{
			anyhow::ensure!(
				renderer.adapter_name.starts_with("llvmpipe")
					&& renderer.adapter_name.ends_with("(Vulkan, Cpu)"),
				"Search baselines require Lavapipe"
			);
			crate::test_support::render_goldens::assert_glyphs(&overlay)?;
			let actual = image::open(&path)?.into_rgba8();
			let baseline = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join(format!("tests/goldens/search/{name}.png"));
			if std::env::var("MARKVIEW_UPDATE_RENDER_GOLDENS").as_deref()
				== Ok("1")
			{
				std::fs::create_dir_all(baseline.parent().unwrap())?;
				actual.save(&baseline)?;
			} else {
				crate::test_support::render_goldens::compare(
					&baseline,
					&actual,
					path.parent().unwrap(),
					name,
				)?;
			}
		}
	}
	Ok(())
}

#[test]
fn poisoned_search_inbox_accepts_new_requests_and_shuts_down() {
	let (tx, rx) = mpsc::channel();
	let worker = Worker::new(move |result| {
		let _ = tx.send(result);
	});
	crate::test_support::poison(&worker.inbox.0);
	worker.submit(Request {
		path: PathBuf::from("note.md"),
		content: 1,
		sequence: worker.cancel(),
		document: Arc::new(markview_core::document::parse(
			"A recovered search.",
		)),
		query: "recovered".into(),
		options: SearchOptions::default(),
	});
	assert_eq!(
		rx.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.matches
			.len(),
		1
	);
	assert!(!worker.inbox.0.is_poisoned());
	crate::test_support::poison(&worker.inbox.0);
	drop(worker);
}

#[test]
fn a_panicked_search_thread_does_not_panic_on_drop() {
	let worker = Worker {
		inbox: Arc::new((Mutex::new(Inbox::default()), Condvar::new())),
		control: Arc::new(crate::mailbox::Control::default()),
		handle: Some(std::thread::spawn(|| panic!("injected worker failure"))),
	};
	crate::test_support::poison(&worker.inbox.0);
	drop(worker);
}

#[test]
fn search_navigation_cancels_restoration_before_later_layouts_arrive() {
	let source = format!(
		"needle\n\n{}needle\n\n{}",
		"Paragraph.\n\n".repeat(80),
		"Following.\n\n".repeat(200)
	);
	let mut h = Harness::new(&source);
	h.query("needle");
	let full = h.app.readers.session.snapshot.clone();
	let document = h.app.readers.session.document.clone().unwrap();
	for hit in [0, 1] {
		let session = &mut h.app.readers.session;
		session.snapshot = full.clone();
		session.scrolling.offset = full.blocks.last().unwrap().y;
		session.saved_reading =
			Some(crate::app::session::Reading::capture(session));
		session.snapshot.blocks.truncate(40);
		session.snapshot.height = session.snapshot.blocks[39].y
			+ session.snapshot.blocks[39].height();
		session.snapshot_complete = false;
		session.layout_pending = true;
		session.scrolling.offset = 0.0;
		session.search.current = Some(hit);
		session.search.pending_navigation = true;
		h.app.apply_search_navigation();
		assert!(h.app.readers.session.saved_reading.is_none());
		assert_eq!(h.app.readers.session.search.pending_navigation, hit == 1);
		let navigated = h.app.readers.session.scrolling.offset;
		let viewport = h.app.viewport();
		h.app.readers.session.accept(
			crate::worker::ReaderSnapshot {
				blocked_images: Default::default(),
				document: document.clone(),
				layout: full.clone(),
				content_version: 1,
				complete: true,
				parse_complete: true,
				remote_deferred: 0,
			},
			viewport,
			None,
		);
		assert!(
			(h.app.readers.session.scrolling.offset - navigated).abs() < 0.01
		);
	}
}
