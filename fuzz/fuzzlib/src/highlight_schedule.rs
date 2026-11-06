//! Deterministic scheduling of background syntax-highlight tasks.
//!
//! The executor is a controlled queue: the input decides whether a task is
//! accepted, when one is run, and which document or theme is current. Every
//! input drains its own queue before the budget window closes, so the target
//! never blocks `wait_highlights` on a task it withheld.
use crate::{coverage, oracle, pipeline};
use markview_layout::{
	background::{Executor, Task},
	layout::{LayoutEngine, LayoutOptions},
};
use std::{
	collections::VecDeque,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
};

#[derive(Default)]
struct Controlled {
	queue: Mutex<VecDeque<Task>>,
	accept: AtomicBool,
}

impl Controlled {
	fn new(accept: bool) -> Self {
		Self {
			accept: AtomicBool::new(accept),
			..Default::default()
		}
	}
	fn set_accept(&self, accept: bool) {
		self.accept.store(accept, Ordering::Relaxed);
	}
	fn pending(&self) -> usize {
		self.queue.lock().unwrap().len()
	}
	fn run_one(&self) -> bool {
		let Some(task) = self.queue.lock().unwrap().pop_front() else {
			return false;
		};
		task.run();
		true
	}
	fn run_all(&self) -> usize {
		let mut ran = 0;
		while self.run_one() {
			ran += 1;
		}
		ran
	}
}

impl Executor for Controlled {
	fn try_submit(&self, task: Task) -> Result<(), Task> {
		if !self.accept.load(Ordering::Relaxed) {
			return Err(task);
		}
		self.queue.lock().unwrap().push_back(task);
		Ok(())
	}
}

/// A fence longer than any run in `body`, so the code cannot close it early.
fn fence(body: &[u8]) -> String {
	let mut longest = 0;
	let mut run = 0;
	for byte in body {
		if *byte == b'~' {
			run += 1;
			longest = longest.max(run);
		} else {
			run = 0;
		}
	}
	"~".repeat((longest + 2).max(3))
}

fn documents(body: &[u8]) -> (String, String) {
	let code = String::from_utf8_lossy(body);
	let fence = fence(body);
	(
		format!("{fence}rust\n{code}\n{fence}\n\nfirst document\n"),
		format!("{fence}python\n{code}\n{fence}\n\nsecond document\n"),
	)
}

/// `HLP1`, eight control bytes, then Markdown used as a code body.
pub fn check(data: &[u8]) {
	if !data.starts_with(b"HLP1") || data.len() < 12 {
		return;
	}
	let controls = &data[4..12];
	let (md_a, md_b) = documents(&data[12..]);
	let doc_a = markview_commonmark::parse(md_a.clone());
	let doc_b = markview_commonmark::parse(md_b.clone());
	let base = LayoutOptions {
		width: 320. + f32::from(controls[1]) * 4.,
		fonts: pipeline::pinned_fonts(),
		..Default::default()
	};
	let mut options = base.clone();
	let executor = Arc::new(Controlled::new(controls[0] % 2 == 0));
	let mut engine =
		LayoutEngine::with_executor(executor.clone(), Arc::new(|| {}));
	let mut current = &doc_a;
	let mut current_md = &md_a;
	for (step, byte) in controls.iter().enumerate() {
		match byte % 8 {
			0 => {
				engine.layout(&doc_a, &options);
				current = &doc_a;
				current_md = &md_a;
			}
			1 => {
				engine.layout(&doc_b, &options);
				current = &doc_b;
				current_md = &md_b;
			}
			2 => {
				engine.poll_highlights();
			}
			3 => {
				executor.run_one();
			}
			4 => executor.set_accept(byte % 2 == 0),
			5 => {
				options.codeblock_theme_override =
					(byte % 2 == 0).then(|| "base16-ocean.dark".into());
			}
			6 => engine.clear_document_cache(),
			_ => engine.release_document(),
		}
		coverage::hit(coverage::Hit::HighlightSchedule);
		if step == 3 {
			// A→B→A: B's layout must not accept A's late results. The
			// controlled executor has to accept here, or `wait_highlights`
			// would block on tasks this input withheld.
			executor.set_accept(true);
			engine.layout(&doc_b, &options);
			executor.run_all();
			engine.poll_highlights();
			engine.layout(&doc_a, &options);
			executor.run_all();
			engine.poll_highlights();
			let warm = engine.layout(&doc_a, &options);
			let cold = pipeline::differential_engine().layout(&doc_a, &options);
			assert_eq!(
				oracle::layout(&warm),
				oracle::layout(&cold),
				"A→B→A did not converge"
			);
			current = &doc_a;
			current_md = &md_a;
			coverage::hit(coverage::Hit::HighlightConverged);
		}
	}
	// Drain this input's work, then require convergence on its last document.
	executor.set_accept(true);
	for _ in 0..4 {
		engine.layout(current, &options);
		executor.run_all();
		engine.poll_highlights();
	}
	let warm = engine.layout(current, &options);
	executor.run_all();
	engine.poll_highlights();
	let cold = pipeline::differential_engine().layout(current, &options);
	assert_eq!(
		oracle::layout(&warm),
		oracle::layout(&cold),
		"latest document diverged after controlled scheduling: {current_md:?}"
	);
	assert_eq!(executor.pending(), 0, "input left tasks in the queue");
	coverage::hit(coverage::Hit::HighlightConverged);
}
