//! The layout owner retains pending inputs and accepts only its current epoch.
use super::{LayoutOptions, expand_tabs_mapped};
use crate::{
	background::{Executor, Task, Wake},
	document::Block,
	style::Condition,
};
use std::{
	collections::{HashMap, HashSet, VecDeque},
	ops::Range,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc,
	},
	time::Duration,
};
use web_time::Instant;
pub(super) type HighlightLines =
	Vec<Vec<(Range<usize>, Option<crate::style::Color>)>>;
pub(super) type HighlightResult = Arc<HighlightLines>;
type Message = (u64, u64, Option<HighlightResult>);

pub(super) struct Highlights {
	cache: HashMap<u64, HighlightResult>,
	tx: mpsc::Sender<Message>,
	rx: mpsc::Receiver<Message>,
	current: HashSet<u64>,
	inflight: HashSet<u64>,
	finished: HashSet<u64>,
	pending: VecDeque<(u64, Task)>,
	pending_keys: HashSet<u64>,
	epoch: u64,
	identity: Option<(u64, Option<Arc<str>>, usize, usize)>,
	cancel: Arc<AtomicBool>,
	executor: Arc<dyn Executor>,
	wake: Wake,
}
impl Highlights {
	pub(super) fn new(executor: Arc<dyn Executor>, wake: Wake) -> Self {
		let (tx, rx) = mpsc::channel();
		executor.on_available(wake.clone());
		Self {
			cache: HashMap::new(),
			tx,
			rx,
			current: HashSet::new(),
			inflight: HashSet::new(),
			finished: HashSet::new(),
			pending: VecDeque::new(),
			pending_keys: HashSet::new(),
			epoch: 0,
			identity: None,
			cancel: Arc::new(AtomicBool::new(false)),
			executor,
			wake,
		}
	}
	pub(super) fn results(&self) -> &HashMap<u64, HighlightResult> {
		&self.cache
	}
	pub(super) fn clear(&mut self) {
		self.cancel.store(true, Ordering::Relaxed);
		self.cancel = Arc::new(AtomicBool::new(false));
		self.epoch += 1;
		self.identity = None;
		self.pending.clear();
		self.pending_keys.clear();
		self.inflight.clear();
		self.finished.clear();
		self.current.clear();
		self.cache.clear();
		while self.rx.try_recv().is_ok() {}
	}
	pub(super) fn prepare(
		&mut self,
		document: u64,
		blocks: &[Block],
		options: &LayoutOptions,
	) {
		let theme = resolved_theme(options);
		let line_bytes = options.limits.highlight_line_bytes;
		let identity = (
			document,
			theme.clone(),
			line_bytes,
			options.limits.highlight_bytes,
		);
		if self.identity.as_ref() != Some(&identity) {
			let cache = std::mem::take(&mut self.cache);
			self.clear();
			self.cache = cache;
			self.identity = Some(identity);
		}
		let mut candidates = Vec::new();
		collect(blocks, theme.as_deref(), line_bytes, &mut candidates);
		let mut seen = HashSet::new();
		candidates.retain(|(key, ..)| seen.insert(*key));
		let mut bytes = 0usize;
		self.current = candidates
			.iter()
			.filter_map(|(key, _, text)| {
				bytes = bytes.saturating_add(text.len());
				(bytes <= options.limits.highlight_bytes).then_some(*key)
			})
			.collect();
		self.cache.retain(|key, _| self.current.contains(key));
		for (key, language, text) in candidates {
			if !self.current.contains(&key)
				|| self.cache.contains_key(&key)
				|| self.finished.contains(&key)
				|| self.inflight.contains(&key)
				|| self.pending_keys.contains(&key)
			{
				continue;
			}
			let language = language.to_owned();
			let text = text.to_owned();
			let theme = theme.clone();
			let cancel = self.cancel.clone();
			let epoch = self.epoch;
			let tx = self.tx.clone();
			let wake = self.wake.clone();
			let task = Task::new(text.len(), move || {
				let result = std::panic::catch_unwind(
					std::panic::AssertUnwindSafe(|| {
						if cancel.load(Ordering::Relaxed) {
							return None;
						}
						let mut highlighter =
							crate::highlight::Highlighter::new(
								&language,
								theme.as_deref(),
							);
						let mut colored = Vec::new();
						for line in super::code::code_lines(&text) {
							if cancel.load(Ordering::Relaxed) {
								return None;
							}
							colored.push(highlighter.highlight(
								&expand_tabs_mapped(line, 4).0,
								line_bytes,
							));
						}
						(!cancel.load(Ordering::Relaxed))
							.then(|| Arc::new(colored))
					}),
				)
				.ok()
				.flatten();
				let _ = tx.send((epoch, key, result));
				wake();
			});
			self.pending_keys.insert(key);
			self.pending.push_back((key, task));
		}
		self.submit();
	}
	fn submit(&mut self) {
		while let Some((key, task)) = self.pending.pop_front() {
			match self.executor.try_submit(task) {
				Ok(()) => {
					self.pending_keys.remove(&key);
					self.inflight.insert(key);
				}
				Err(task) => {
					self.pending.push_front((key, task));
					break;
				}
			}
		}
	}
	pub(super) fn poll(&mut self) -> bool {
		let mut changed = false;
		while let Ok(message) = self.rx.try_recv() {
			changed |= self.store(message);
		}
		self.submit();
		changed
	}
	pub(super) fn settle(&mut self) -> bool {
		let mut changed = self.poll();
		let deadline = Instant::now() + HIGHLIGHT_WAIT;
		while !self.inflight.is_empty() || !self.pending.is_empty() {
			let Some(remaining) =
				deadline.checked_duration_since(Instant::now())
			else {
				log::warn!(
					"Highlights: current work exceeded the export deadline"
				);
				self.cancel.store(true, Ordering::Relaxed);
				self.pending.clear();
				self.pending_keys.clear();
				self.inflight.clear();
				break;
			};
			match self
				.rx
				.recv_timeout(remaining.min(Duration::from_millis(50)))
			{
				Ok(message) => {
					changed |= self.store(message);
				}
				Err(mpsc::RecvTimeoutError::Timeout) => {}
				Err(mpsc::RecvTimeoutError::Disconnected) => break,
			}
			self.submit();
		}
		changed
	}
	fn store(&mut self, (epoch, key, result): Message) -> bool {
		if epoch != self.epoch || !self.current.contains(&key) {
			return false;
		}
		self.inflight.remove(&key);
		self.finished.insert(key);
		if let Some(result) = result {
			self.cache.insert(key, result);
			true
		} else {
			false
		}
	}
}
impl Drop for Highlights {
	fn drop(&mut self) {
		self.cancel.store(true, Ordering::Relaxed);
	}
}

fn resolved_theme(options: &LayoutOptions) -> Option<Arc<str>> {
	options
		.codeblock_theme_override
		.as_deref()
		.or(options
			.stylesheet
			.rule(Condition::CodeBlock)
			.theme
			.as_deref())
		.map(Arc::from)
}
pub(super) fn key(
	language: &str,
	text: &str,
	theme: Option<&str>,
	line_bytes: usize,
) -> u64 {
	crate::document::fingerprint(&(language, text, theme, line_bytes))
}
fn collect<'a>(
	blocks: &'a [Block],
	theme: Option<&str>,
	line_bytes: usize,
	out: &mut Vec<(u64, &'a str, &'a str)>,
) {
	for block in blocks {
		block.for_each_code_block(&mut |language, text| {
			out.push((key(language, text, theme, line_bytes), language, text))
		});
	}
}
const HIGHLIGHT_WAIT: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Mutex;
	#[derive(Default)]
	struct Controlled {
		tasks: Mutex<Vec<Task>>,
		full: AtomicBool,
	}
	impl Executor for Controlled {
		fn try_submit(&self, task: Task) -> Result<(), Task> {
			if self.full.load(Ordering::Relaxed) {
				return Err(task);
			}
			self.tasks.lock().unwrap().push(task);
			Ok(())
		}
	}
	#[test]
	fn many_backpressured_blocks_are_queued_once_across_reflows() {
		for count in [2_000, 16_000] {
			let text: String = (0..count)
				.map(|index| format!("```rust\nlet n = {index};\n```\n\n"))
				.collect();
			let document = markview_commonmark::parse(text);
			let executor = Arc::new(Controlled::default());
			executor.full.store(true, Ordering::Relaxed);
			let mut highlights =
				Highlights::new(executor.clone(), Arc::new(|| {}));
			let start = Instant::now();
			highlights.prepare(
				document.content_key().get(),
				document.blocks(),
				&LayoutOptions::default(),
			);
			eprintln!(
				"highlight preparation: {count} blocks in {:?}",
				start.elapsed()
			);
			assert_eq!(highlights.pending.len(), count);
			assert_eq!(highlights.pending_keys.len(), count);
			highlights.prepare(
				document.content_key().get(),
				document.blocks(),
				&LayoutOptions {
					width: 100.,
					..LayoutOptions::default()
				},
			);
			assert_eq!(highlights.pending.len(), count);
			assert_eq!(highlights.pending_keys.len(), count);
			executor.full.store(false, Ordering::Relaxed);
			highlights.poll();
			assert!(highlights.pending.is_empty());
			assert!(highlights.pending_keys.is_empty());
			assert_eq!(executor.tasks.lock().unwrap().len(), count);
			assert_eq!(highlights.inflight.len(), count);
			highlights.clear();
			assert!(highlights.inflight.is_empty());
		}
	}

	#[test]
	fn epochs_reject_late_results_and_reflow_does_not_duplicate_jobs() {
		let executor = Arc::new(Controlled::default());
		let mut highlights = Highlights::new(executor.clone(), Arc::new(|| {}));
		let doc = markview_commonmark::parse(
			"---\ntitle: N\n---\n\n```rust\nlet n = 1;\n```\n",
		);
		let options = LayoutOptions::default();
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&options,
		);
		assert_eq!(executor.tasks.lock().unwrap().len(), 2);
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&LayoutOptions {
				width: 100.,
				..options.clone()
			},
		);
		assert_eq!(executor.tasks.lock().unwrap().len(), 2);
		let old_epoch = highlights.epoch;
		let old_key = *highlights.current.iter().next().unwrap();
		highlights.clear();
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&options,
		);
		highlights
			.tx
			.send((old_epoch, old_key, Some(Arc::new(Vec::new()))))
			.unwrap();
		assert!(!highlights.poll());
		assert!(highlights.cache.is_empty());
		let tasks = std::mem::take(&mut *executor.tasks.lock().unwrap());
		for task in tasks {
			task.run();
		}
		assert!(highlights.poll());
		assert!(highlights.inflight.is_empty());
		assert_eq!(highlights.cache.len(), 2);
		let theme = LayoutOptions {
			codeblock_theme_override: Some("none".into()),
			..options.clone()
		};
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&theme,
		);
		assert!(highlights.cache.is_empty());
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&LayoutOptions {
				limits: crate::limits::Limits {
					highlight_line_bytes: 1,
					..theme.limits
				},
				..theme
			},
		);
		assert!(highlights.cache.is_empty());
	}
	#[test]
	fn backpressure_retains_inputs_and_failed_work_settles() {
		let executor = Arc::new(Controlled::default());
		executor.full.store(true, Ordering::Relaxed);
		let mut highlights = Highlights::new(executor.clone(), Arc::new(|| {}));
		let doc = markview_commonmark::parse("```rust\nfn main() {}\n```\n");
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&LayoutOptions::default(),
		);
		assert_eq!(highlights.pending.len(), 1);
		assert!(highlights.inflight.is_empty());
		executor.full.store(false, Ordering::Relaxed);
		highlights.poll();
		let key = *highlights.current.iter().next().unwrap();
		highlights.tx.send((highlights.epoch, key, None)).unwrap();
		assert!(!highlights.poll());
		assert!(highlights.inflight.is_empty());
		highlights.prepare(
			doc.document.content_key().get(),
			doc.blocks(),
			&LayoutOptions::default(),
		);
		assert_eq!(executor.tasks.lock().unwrap().len(), 1);
		highlights.clear();
		assert!(highlights.pending.is_empty());
	}
}
