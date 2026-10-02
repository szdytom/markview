use super::*;
use crate::{
	cli::{LaunchOptions, Mode},
	state::{PanelPage, PanelTab},
};
use markview_core::background::{Executor, Task};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

#[derive(Clone)]
struct Proxy(Sender<Event>);
impl SendEvent for Proxy {
	fn try_send(&self, event: Event) -> bool {
		self.0.send(event).is_ok()
	}
}
struct Queue(Sender<Task>);
impl Executor for Queue {
	fn try_submit(&self, task: Task) -> Result<(), Task> {
		self.0.send(task).map_err(|error| error.0)
	}
}
fn app() -> (App<Proxy>, Receiver<Task>, Receiver<Event>) {
	let (send, events) = mpsc::channel();
	let (queue, tasks) = mpsc::channel();
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			options: crate::test_support::options(),
			..Default::default()
		},
		Proxy(send),
	);
	let mut services = crate::services::Services::new(1);
	services.handle.cpu = Arc::new(Queue(queue));
	app.services = Arc::new(services);
	(app, tasks, events)
}
fn task(tasks: &Receiver<Task>) -> Task {
	tasks.recv_timeout(Duration::from_secs(5)).unwrap()
}
fn completion(events: &Receiver<Event>) -> Completion {
	loop {
		if let Event::SettingsLoaded(result) =
			events.recv_timeout(Duration::from_secs(5)).unwrap()
		{
			return *result;
		}
	}
}

#[test]
fn styles_draw_feedback_before_work_and_refresh_without_losing_the_list() {
	let (mut app, tasks, events) = app();
	app.action(Command::SettingsTab(PanelTab::Styles));
	let work = task(&tasks);
	let initial = app.preferences.style_entries.clone();
	assert!(!initial.is_empty());
	assert!(app.interaction.styles_open());
	assert_eq!(
		app.settings_load()
			.unwrap()
			.message(crate::lang::Lang::En)
			.as_deref(),
		Some("Loading…")
	);
	assert!(!app.chrome().overlay().is_empty());
	let kind = app.settings_kind();
	app.settings_resources.presented(kind);
	assert!(
		app.buttons()
			.iter()
			.filter(|b| dependent(b.action))
			.all(|b| !b.enabled)
	);
	assert!(
		app.buttons()
			.iter()
			.filter(|b| matches!(b.action, Command::SettingsTab(_)))
			.all(|b| b.enabled)
	);
	app.action(Command::Styles);
	assert!(
		tasks.try_recv().is_err(),
		"same input must share its pending request"
	);
	assert_eq!(app.preferences.style_entries.len(), initial.len());
	work.run();
	app.settings_loaded(completion(&events));
	assert!(!app.settings_load().unwrap().blocked());
	assert!(app.font_panel.view().catalog.is_empty());
	assert_eq!(app.font_panel.view().choices.generation, 0);
	app.action(Command::Styles);
	let work = task(&tasks);
	app.interaction.show_panel(PanelPage::Closed);
	work.run();
	app.settings_loaded(completion(&events));
	assert!(
		!app.interaction.panel_open(),
		"completion must not reopen a closed panel"
	);
}

#[test]
fn a_fast_result_still_presents_feedback_before_shaping_the_first_list() {
	let (mut app, tasks, events) = app();
	app.action(Command::Styles);
	task(&tasks).run();
	app.settings_loaded(completion(&events));
	assert!(matches!(app.settings_load().unwrap().status, Status::Ready));
	assert!(app.settings_load().unwrap().blocked());
	assert_eq!(
		app.settings_load()
			.unwrap()
			.message(crate::lang::Lang::En)
			.as_deref(),
		Some("Loading…")
	);
	assert!(app.chrome().style_entries.is_empty());
	assert!(!app.chrome().overlay().is_empty());
	let kind = app.settings_kind();
	assert!(
		app.settings_resources.presented(kind),
		"the first feedback frame schedules the ready body"
	);
	assert!(!app.settings_load().unwrap().blocked());
	assert!(!app.chrome().style_entries.is_empty());
	app.action(Command::Styles);
	let work = task(&tasks);
	assert_eq!(
		app.settings_load()
			.unwrap()
			.message(crate::lang::Lang::En)
			.as_deref(),
		Some("Refreshing…")
	);
	assert!(!app.chrome().style_entries.is_empty());
	work.run();
	app.settings_loaded(completion(&events));
}

#[test]
fn fonts_load_catalogue_and_candidates_independently_and_reject_old_results() {
	let (mut app, tasks, events) = app();
	app.action(Command::SettingsTab(PanelTab::Fonts));
	let catalog = task(&tasks);
	assert_eq!(
		app.settings_load()
			.unwrap()
			.message(crate::lang::Lang::En)
			.as_deref(),
		Some("Loading…")
	);
	assert!(!app.chrome().overlay().is_empty());
	let kind = app.settings_kind();
	app.settings_resources.presented(kind);
	assert_eq!(app.font_panel.view().choices.generation, 0);
	catalog.run();
	app.settings_loaded(completion(&events));
	assert!(!app.font_panel.view().catalog.is_empty());
	assert_eq!(app.font_panel.view().choices.generation, 0);
	app.action(Command::Fonts(super::super::font_panel::Command::Choosers));
	task(&tasks).run();
	let old = completion(&events);
	app.fonts_config.revision += 1;
	app.refresh_settings_resources(false);
	let fresh = task(&tasks);
	let kind = app.settings_kind();
	app.settings_resources.presented(kind);
	app.settings_loaded(old);
	assert!(app.settings_load().unwrap().blocked());
	fresh.run();
	app.settings_loaded(completion(&events));
	assert!(!app.settings_load().unwrap().blocked());
	assert!(
		app.font_panel
			.resolve(
				crate::settings::FontRole::Serif,
				super::super::font_panel::Selection {
					catalog_generation: app
						.font_panel
						.view()
						.choices
						.generation,
					index: 0
				}
			)
			.is_some()
	);
	let generation = app.font_panel.view().choices.generation;
	app.action(Command::SettingsTab(PanelTab::Generic));
	app.action(Command::SettingsTab(PanelTab::Fonts));
	assert!(
		tasks.try_recv().is_err(),
		"unchanged candidates should be a cache hit"
	);
	assert_eq!(app.font_panel.view().choices.generation, generation);
}

#[test]
fn failed_requests_leave_feedback_and_can_be_retried() {
	let (mut app, tasks, events) = app();
	app.action(Command::SettingsTab(PanelTab::Fonts));
	let work = task(&tasks);
	let version = app.settings_resources.loads[Kind::Catalog.index()].version;
	app.settings_loaded(Completion {
		kind: Kind::Catalog,
		version,
		result: Err(anyhow::anyhow!("test failure")),
		started: Instant::now(),
	});
	assert!(
		app.settings_load()
			.unwrap()
			.message(crate::lang::Lang::En)
			.unwrap()
			.contains("test failure")
	);
	assert!(
		app.buttons()
			.iter()
			.any(|b| b.action == Command::RetrySettingsLoad && b.enabled)
	);
	assert!(!app.chrome().overlay().is_empty());
	app.action(Command::RetrySettingsLoad);
	let retry = task(&tasks);
	work.run();
	app.settings_loaded(completion(&events));
	assert!(matches!(
		app.settings_load().unwrap().status,
		Status::Loading
	));
	retry.run();
	app.settings_loaded(completion(&events));
	assert!(matches!(app.settings_load().unwrap().status, Status::Ready));
}

#[test]
fn downloaded_face_cache_tracks_changed_and_removed_files() {
	// A local cache uses the same scan and identity rules without global user files.
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("font.otf");
	let bytes = include_bytes!(
		"../../../crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf"
	);
	std::fs::write(&path, bytes).unwrap();
	let mut cache = FaceCache::default();
	let before = cache.read(Some(dir.path()));
	assert_eq!(before.len(), 1);
	assert_eq!(cache.read(Some(dir.path()))[0].families, before[0].families);
	std::fs::write(&path, b"invalid font").unwrap();
	assert!(cache.read(Some(dir.path())).is_empty());
	assert_eq!(cache.entries.len(), 1);
	std::fs::remove_file(&path).unwrap();
	assert!(cache.read(Some(dir.path())).is_empty());
	assert!(cache.entries.is_empty());
}

#[test]
fn reader_and_pdf_catalogues_remain_independent_during_refresh() {
	let (mut app, tasks, events) = app();
	app.action(Command::ExportStyles);
	let export = task(&tasks);
	assert_eq!(app.settings_kind(), Some(Kind::Export));
	assert!(!app.chrome().overlay().is_empty());
	app.action(Command::SettingsTab(PanelTab::Styles));
	let reader = task(&tasks);
	let _draws = app.chrome().overlay();
	let kind = app.settings_kind();
	app.settings_resources.presented(kind);
	export.run();
	app.settings_loaded(completion(&events));
	assert!(app.settings_load().unwrap().blocked());
	assert!(!app.settings_resources.export_entries.is_empty());
	reader.run();
	app.settings_loaded(completion(&events));
	assert!(!app.settings_load().unwrap().blocked());
	let reader_ids: Vec<_> = app
		.preferences
		.style_entries
		.iter()
		.map(|e| e.id.clone())
		.collect();
	app.action(Command::ExportStyles);
	task(&tasks).run();
	app.settings_loaded(completion(&events));
	assert_eq!(
		reader_ids,
		app.preferences
			.style_entries
			.iter()
			.map(|e| e.id.clone())
			.collect::<Vec<_>>()
	);
	app.settings_resources.invalidate();
	app.refresh_settings_resources(false);
	let work = task(&tasks);
	app.settings_resources.invalidate();
	work.run();
	app.settings_loaded(completion(&events));
	assert!(matches!(app.settings_load().unwrap().status, Status::Idle));
}

#[test]
#[ignore = "measures system fonts and prints settings click→overlay timings"]
fn settings_feedback_measurement() {
	let (send, events) = mpsc::channel();
	let mut app = App::new(
		LaunchOptions {
			mode: Mode::Smoke,
			..Default::default()
		},
		Proxy(send),
	);
	// An opened reader has already drawn its toolbar and initialized UI fonts.
	app.action(Command::SettingsTab(PanelTab::Generic));
	let _draws = app.chrome().overlay();
	for tab in [PanelTab::Styles, PanelTab::Fonts] {
		let before = Instant::now();
		let entries = crate::stylesheet::catalog(
			crate::stylesheet::directory().as_deref(),
			app.preferences.values.style.as_deref(),
		);
		let builtin = markview_core::style::Stylesheet::builtin();
		let sheets =
			std::iter::once(("builtin", builtin.font_families.as_slice()))
				.chain(entries.iter().map(|entry| {
					(entry.id.as_str(), entry.font_families.as_slice())
				}));
		let _catalog = crate::fonts::catalog(
			sheets,
			crate::fonts::directory().as_deref(),
			&app.fonts_config,
		);
		if tab == PanelTab::Fonts {
			Choices::default().refresh(&app.fonts_config);
		}
		let synchronous_ms = before.elapsed().as_secs_f64() * 1000.;
		for pass in ["first", "repeat"] {
			let clicked = Instant::now();
			app.action(Command::SettingsTab(tab));
			let _draws = app.chrome().overlay();
			let feedback_ms = clicked.elapsed().as_secs_f64() * 1000.;
			let kind = app.settings_kind();
			app.settings_resources.presented(kind);
			app.settings_loaded(completion(&events));
			let ready_ms = clicked.elapsed().as_secs_f64() * 1000.;
			let body = Instant::now();
			let _draws = app.chrome().overlay();
			println!(
				"{tab:?} {pass}: prior synchronous work {synchronous_ms:.2} ms; click→overlay {feedback_ms:.2} ms; data ready {ready_ms:.2} ms; body preparation {:.2} ms",
				body.elapsed().as_secs_f64() * 1000.
			);
		}
	}
	let clicked = Instant::now();
	app.action(Command::Fonts(super::super::font_panel::Command::Choosers));
	let _draws = app.chrome().overlay();
	let feedback_ms = clicked.elapsed().as_secs_f64() * 1000.;
	let kind = app.settings_kind();
	app.settings_resources.presented(kind);
	app.settings_loaded(completion(&events));
	println!(
		"Choices: click→overlay {feedback_ms:.2} ms; ready {:.2} ms",
		clicked.elapsed().as_secs_f64() * 1000.
	);
}

#[cfg(unix)]
#[test]
fn symlinked_fonts_and_styles_follow_target_changes() {
	use std::os::unix::fs::symlink;
	let resources = tempfile::tempdir().unwrap();
	let targets = tempfile::tempdir().unwrap();
	let dir = Some(resources.path());
	let font = targets.path().join("font.otf");
	std::fs::write(
		&font,
		include_bytes!(
			"../../../crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf"
		),
	)
	.unwrap();
	symlink(&font, resources.path().join("linked.otf")).unwrap();
	let mut faces = FaceCache::default();
	let before = faces.read(dir);
	assert_eq!(before.len(), 1, "a linked font is discoverable");
	assert_eq!(before[0].file, "linked.otf");
	std::fs::write(
		&font,
		include_bytes!(
			"../../../crates/markview-core/tests/fonts/NotoSans-Regular-subset.otf"
		),
	)
	.unwrap();
	let after = faces.read(dir);
	assert_eq!(after.len(), 1);
	assert_ne!(before[0].families, after[0].families);
	std::fs::remove_file(&font).unwrap();
	assert!(faces.read(dir).is_empty());

	let style = targets.path().join("style.mvss.toml");
	let mut styles = StyleCache::default();
	assert!(
		!styles
			.read(dir, None, StyleTarget::Ui)
			.iter()
			.any(|entry| entry.id == "linked")
	);
	std::fs::write(
		&style,
		"format_version=2\nversion=1\n[meta]\nname='First'\n",
	)
	.unwrap();
	symlink(&style, resources.path().join("linked.mvss.toml")).unwrap();
	let entries = styles.read(dir, None, StyleTarget::Ui);
	assert_eq!(
		entries
			.iter()
			.find(|entry| entry.id == "linked")
			.unwrap()
			.name,
		"First"
	);
	std::fs::write(
		&style,
		"format_version=2\nversion=1\n[meta]\nname='Changed stylesheet'\n",
	)
	.unwrap();
	let entries = styles.read(dir, None, StyleTarget::Ui);
	assert_eq!(
		entries
			.iter()
			.find(|entry| entry.id == "linked")
			.unwrap()
			.name,
		"Changed stylesheet"
	);
	std::fs::remove_file(&style).unwrap();
	assert!(
		!styles
			.read(dir, None, StyleTarget::Ui)
			.iter()
			.any(|entry| entry.id == "linked")
	);
}
