use super::*;
use markview_layout::background::{Executor, Task};
use std::sync::{Arc, mpsc};

fn begin() -> (App<Proxy>, Instant) {
	let mut app = super::golden::reader_app(
		"[^note]: A **short** note.",
		crate::render::Theme::Light,
		&Default::default(),
	);
	let point = super::interaction_tests::reference_point(&app);
	app.interaction.pointer.move_to((point.x, point.y));
	let now = Instant::now();
	app.hover_preview(now);
	(app, now)
}
fn ready(app: &mut App<Proxy>) -> work::Completion {
	app.preview
		.worker
		.wait(Duration::from_secs(10))
		.expect("the preview did not finish")
}

#[test]
fn layout_starts_at_thirty_ms_and_ready_results_wait_until_one_hundred() {
	let (mut app, now) = begin();
	assert_eq!(app.preview.deadline(), Some(now + PREPARE_DELAY));
	app.preview_tick(now + PREPARE_DELAY - Duration::from_nanos(1));
	assert!(app.preview.pending.is_none());
	app.preview_tick(now + PREPARE_DELAY);
	assert!(app.preview.pending.is_some());
	assert!(app.preview.open().is_none());
	assert_eq!(app.preview.deadline(), Some(now + SHOW_DELAY));
	let completion = ready(&mut app);
	app.preview.accept(completion);
	app.preview_tick(now + SHOW_DELAY - Duration::from_nanos(1));
	assert!(app.preview.open().is_none());
	app.preview_tick(now + SHOW_DELAY);
	assert!(app.preview.floating().is_some());
	assert_eq!(app.preview.deadline(), None);
}

#[test]
fn late_results_show_without_another_delay_or_a_busy_deadline() {
	let (mut app, now) = begin();
	app.preview_tick(now + PREPARE_DELAY);
	let completion = ready(&mut app);
	app.preview_tick(now + SHOW_DELAY);
	assert!(app.preview.open().is_none());
	assert_eq!(app.preview.deadline(), None);
	app.preview.accept(completion);
	app.preview_tick(now + SHOW_DELAY + Duration::from_millis(1));
	assert!(app.preview.floating().is_some());
}

#[test]
fn leaving_before_or_after_submission_cancels_without_opening() {
	for submitted in [false, true] {
		let (mut app, now) = begin();
		if submitted {
			app.preview_tick(now + PREPARE_DELAY);
		}
		let token = app.preview.worker.token();
		app.interaction.pointer.move_to((1100.0, 650.0));
		app.hover_preview(now + Duration::from_millis(40));
		assert!(token.is_none_or(|token| token.is_cancelled()));
		assert!(app.preview.pending.is_none());
		assert!(app.preview.worker.try_recv().is_none());
		app.preview_tick(now + SHOW_DELAY);
		assert!(app.preview.open().is_none());
		assert_eq!(app.preview.deadline(), None);
	}
}

#[test]
fn returning_to_the_same_reference_rejects_its_previous_completion() {
	let (mut app, now) = begin();
	app.preview_tick(now + PREPARE_DELAY);
	let old = ready(&mut app);
	app.preview
		.hover(Some(Target::Tab(77)), now + Duration::from_millis(31));
	app.hover_preview(now + Duration::from_millis(32));
	app.preview_tick(now + Duration::from_millis(32) + PREPARE_DELAY);
	assert_ne!(old.serial, app.preview.serial);
	app.preview.accept(old);
	assert!(app.preview.pending.as_ref().unwrap().ready.is_none());
	let latest = ready(&mut app);
	app.preview.accept(latest);
	app.preview_tick(now + Duration::from_millis(32) + SHOW_DELAY);
	assert!(app.preview.floating().is_some());
}

#[test]
fn changed_layout_document_or_images_invalidates_pending_work() {
	for mutation in 0..5 {
		let (mut app, now) = begin();
		app.preview_tick(now + PREPARE_DELAY);
		let token = app.preview.worker.token().unwrap();
		match mutation {
			0 => app.preferences.values.font_size += 1.0,
			1 => app.readers.session.version += 1,
			2 => {
				app.readers
					.session
					.display
					.snapshot
					.images
					.entries
					.insert("changed".into(), Default::default());
			}
			3 => {
				app.readers.session.document =
					Some(crate::loaded::LoadedDocument::markdown(Arc::new(
						markview_commonmark::parse(
							"Changed[^note].\n\n[^note]: Changed.",
						),
					)));
			}
			4 => app.readers.session.scrolling.offset += 20.0,
			_ => unreachable!(),
		}
		app.preview_tick(now + PREPARE_DELAY + Duration::from_millis(1));
		assert!(token.is_cancelled());
		if let Some(pending) = &app.preview.pending {
			assert!(pending.source.matches(&app));
		}
		assert!(app.preview.open().is_none());
	}
}

struct Held(mpsc::Sender<Task>);
impl Executor for Held {
	fn try_submit(&self, task: Task) -> Result<(), Task> {
		self.0.send(task).map_err(|error| error.0)
	}
}

#[test]
fn cancellation_discards_queued_computation_and_keeps_only_the_latest_waiter() {
	let (app, _) = begin();
	let source = Arc::new(
		app.footnote_source(app.preview.hover.as_ref().unwrap().target)
			.unwrap(),
	);
	let services = crate::services::Services::new(1);
	let mut handle = services.handle.clone();
	let (send, jobs) = mpsc::channel();
	handle.cpu = Arc::new(Held(send));
	let mut worker = work::Worker::default();
	worker.submit(&handle, 1, source.clone(), || {});
	let first = jobs.recv_timeout(Duration::from_secs(5)).unwrap();
	let cancelled = worker.token().unwrap();
	worker.submit(&handle, 2, source.clone(), || {});
	worker.submit(&handle, 3, source, || {});
	assert!(cancelled.is_cancelled());
	assert!(
		jobs.try_recv().is_err(),
		"only one computation may hold the CPU slot"
	);
	first.run();
	let latest = jobs.recv_timeout(Duration::from_secs(5)).unwrap();
	assert!(worker.try_recv().is_none());
	latest.run();
	let completion = worker.wait(Duration::from_secs(5)).unwrap();
	assert_eq!(completion.serial, 3);
	assert!(completion.snapshot.is_some());
	assert!(jobs.try_recv().is_err());
}

#[test]
fn shared_documents_larger_than_the_queue_budget_can_preview_small_notes() {
	let (app, _) = begin();
	let mut source = app
		.footnote_source(app.preview.hover.as_ref().unwrap().target)
		.unwrap();
	source.document = Arc::new(crate::loaded::LoadedDocument::Semantic {
		document: Arc::new(source.document.document().clone()),
		size: 128 * 1024 * 1024,
		input: 1,
		frontend: 2,
	});
	let services = crate::services::Services::new(1);
	let mut worker = work::Worker::default();
	worker.submit(&services.handle, 1, Arc::new(source), || {});
	assert!(
		worker
			.wait(Duration::from_secs(5))
			.expect("the shared document must not exhaust the queue budget")
			.snapshot
			.is_some()
	);
}

#[test]
fn cancellation_releases_an_already_completed_snapshot() {
	let (mut app, now) = begin();
	app.preview_tick(now + PREPARE_DELAY);
	let completion = ready(&mut app);
	app.preview.accept(completion);
	assert!(app.preview.pending.as_ref().unwrap().ready.is_some());
	app.dismiss_preview();
	assert!(app.preview.pending.is_none());
	assert!(app.preview.worker.try_recv().is_none());
}

proptest::proptest! {
	#[test]
	fn motion_inside_a_target_preserves_its_original_deadlines(trace in proptest::collection::vec((0u8..3, 0u16..100), 0..60)) {
		let mut preview = Preview::default();
		let mut now = Instant::now();
		let mut previous = None;
		let mut started = now;
		for (value, elapsed) in trace {
			now += Duration::from_millis(elapsed.into());
			let target = match value {
				0 => None,
				1 => Some(Target::Tab(1)),
				_ => Some(Target::Footnote { block: markview_document::BlockId::new(1).unwrap(), link: markview_layout::scene::LinkId::Inline(markview_document::LinkOccurrenceId::new(1).unwrap()) }),
			};
			if target != previous { started = now; }
			preview.hover(target, now);
			proptest::prop_assert_eq!(preview.deadline(), target.map(|target| started + if matches!(target, Target::Footnote { .. }) { PREPARE_DELAY } else { SHOW_DELAY }));
			previous = target;
		}
	}
}

#[test]
fn early_completions_use_a_preview_wake_instead_of_forcing_a_frame() {
	let (mut app, events) = app();
	app.readers.session.accept(
		picture_reader(
			"Reference[^n].\n\n[^n]: Note.",
			760.0,
			Default::default(),
		),
		app.viewport(),
		None,
	);
	let point = super::interaction_tests::reference_point(&app);
	app.interaction.pointer.move_to((point.x, point.y));
	let now = Instant::now();
	app.hover_preview(now);
	app.preview_tick(now + PREPARE_DELAY);
	assert!(matches!(
		events.recv_timeout(Duration::from_secs(10)).unwrap(),
		Event::PreviewReady
	));
	app.preview_tick(now + SHOW_DELAY - Duration::from_millis(1));
	assert!(app.preview.open().is_none());
	assert_eq!(app.preview.deadline(), Some(now + SHOW_DELAY));
}

#[test]
fn a_cached_page_can_restore_its_document_before_the_display_gate() {
	let (mut app, now) = begin();
	let document = app.readers.session.document.take();
	app.preview_tick(now + PREPARE_DELAY);
	assert!(app.preview.pending.is_none());
	assert_eq!(app.preview.deadline(), Some(now + SHOW_DELAY));
	app.readers.session.document = document;
	app.preview_tick(now + Duration::from_millis(60));
	let completion = ready(&mut app);
	app.preview.accept(completion);
	app.preview_tick(now + SHOW_DELAY);
	assert!(app.preview.floating().is_some());
}

#[test]
fn an_unavailable_document_does_not_spin_past_the_display_gate() {
	let (mut app, now) = begin();
	app.readers.session.document = None;
	app.preview_tick(now + PREPARE_DELAY);
	assert_eq!(app.preview.deadline(), Some(now + SHOW_DELAY));
	app.preview_tick(now + SHOW_DELAY);
	assert!(!app.preview.tracking());
	assert_eq!(app.preview.deadline(), None);
}
