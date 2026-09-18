//! Background highlighting owns its jobs, completion queue and bounded results.
use super::{LayoutOptions, expand_tabs_mapped};
use crate::{
	document::{Block, BlockKind},
	style::Condition,
};
use std::ops::Range;
use std::time::{Duration, Instant};
use std::{
	collections::{HashMap, HashSet},
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
		mpsc,
	},
	thread,
};
pub(super) type HighlightLines =
	Vec<Vec<(Range<usize>, Option<crate::style::Color>)>>;
pub(super) type HighlightResult = Arc<HighlightLines>;
type HighlightMessage = (u64, HighlightResult);

pub(super) struct Highlights {
	highlight_cache: HashMap<u64, HighlightResult>,
	highlight_tx: mpsc::Sender<HighlightMessage>,
	highlight_rx: mpsc::Receiver<HighlightMessage>,
	highlight_inflight: HashSet<u64>,
}
impl Highlights {
	pub(super) fn new() -> Self {
		let (highlight_tx, highlight_rx) = mpsc::channel();
		Self {
			highlight_cache: HashMap::new(),
			highlight_tx,
			highlight_rx,
			highlight_inflight: HashSet::new(),
		}
	}
	pub(super) fn results(&self) -> &HashMap<u64, HighlightResult> {
		&self.highlight_cache
	}
	/// Drops every colored result, used when no document is open.
	pub(super) fn clear(&mut self) {
		self.highlight_cache.clear();
	}
	pub(super) fn prepare(
		&mut self,
		blocks: &[Block],
		options: &LayoutOptions,
	) {
		let theme = resolved_theme(options);
		// Every code block of the current document, cached or not. Retaining on
		// this set bounds the cache to the document in hand, so the results of
		// a code block that left the document cannot pile up.
		let mut candidates: Vec<(u64, &str, &str)> = Vec::new();
		collect(blocks, theme.as_deref(), &mut candidates);
		let current: HashSet<u64> =
			candidates.iter().map(|(key, ..)| *key).collect();
		self.highlight_cache.retain(|key, _| current.contains(key));
		// Highlighting is cosmetic, so work past the byte budget is simply not
		// done: the code keeps its text and is laid out uncolored.
		let mut bytes = 0;
		let jobs: Vec<(u64, String, String, Option<Arc<str>>)> = candidates
			.into_iter()
			.filter(|(key, ..)| {
				!self.highlight_cache.contains_key(key)
					&& !self.highlight_inflight.contains(key)
			})
			.filter(|(_, _, text)| {
				bytes += text.len();
				bytes <= options.limits.highlight_bytes
			})
			.map(|(key, language, text)| {
				(key, language.to_owned(), text.to_owned(), theme.clone())
			})
			.collect();
		if jobs.is_empty() {
			return;
		}
		for (key, ..) in &jobs {
			self.highlight_inflight.insert(*key);
		}
		let worker_count = thread::available_parallelism()
			.map_or(1, std::num::NonZeroUsize::get)
			.min(jobs.len())
			.min(if jobs.len() < 4 { 1 } else { 4 });
		let tx = self.highlight_tx.clone();
		let max_line_bytes = options.limits.highlight_line_bytes;
		thread::spawn(move || {
			let next = AtomicUsize::new(0);
			thread::scope(|scope| {
				for _ in 0..worker_count {
					let next = &next;
					let jobs = &jobs;
					let tx = tx.clone();
					scope.spawn(move || {
						loop {
							let index = next.fetch_add(1, Ordering::Relaxed);
							let Some((key, language, text, theme)) =
								jobs.get(index)
							else {
								break;
							};
							let lines = text
								.trim_end_matches('\n')
								.split('\n')
								.map(|line| {
									expand_tabs_mapped(line, 4).0.to_owned()
								});
							let highlighted = crate::highlight::highlight_block(
								language,
								theme.as_deref(),
								lines,
								max_line_bytes,
							);
							let _ = tx.send((*key, Arc::new(highlighted)));
						}
					});
				}
			});
		});
	}

	pub(super) fn poll(&mut self) -> bool {
		let mut changed = false;
		while let Ok((key, highlighted)) = self.highlight_rx.try_recv() {
			self.store(key, highlighted);
			changed = true;
		}
		changed
	}

	/// Waits for every started job, so a caller without an event loop draws the
	/// colored layout on its first and only pass. A job that never reports is
	/// given up on after [`HIGHLIGHT_WAIT`], because an export must not hang.
	pub(super) fn settle(&mut self) -> bool {
		let mut changed = self.poll();
		let deadline = Instant::now() + HIGHLIGHT_WAIT;
		while !self.highlight_inflight.is_empty() {
			let Some(remaining) =
				deadline.checked_duration_since(Instant::now())
			else {
				log::warn!(
					"Highlights: {} job(s) did not report; the export stays uncolored",
					self.highlight_inflight.len()
				);
				self.highlight_inflight.clear();
				break;
			};
			let wait = remaining.min(Duration::from_millis(50));
			match self.highlight_rx.recv_timeout(wait) {
				Ok((key, highlighted)) => {
					self.store(key, highlighted);
					changed = true;
				}
				Err(mpsc::RecvTimeoutError::Timeout) => {}
				Err(mpsc::RecvTimeoutError::Disconnected) => {
					self.highlight_inflight.clear();
					break;
				}
			}
		}
		changed
	}

	fn store(&mut self, key: u64, highlighted: HighlightResult) {
		self.highlight_inflight.remove(&key);
		self.highlight_cache.insert(key, highlighted);
	}
}

/// The syntax theme in force, resolved from `options` exactly as the code
/// layout resolves it.
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

/// Identity of one code block's coloring. It is a pure function of the source
/// and theme, so it doubles as the block cache's highlight token.
pub(super) fn key(language: &str, text: &str, theme: Option<&str>) -> u64 {
	crate::document::fingerprint(&(language, text, theme))
}

fn collect<'a>(
	blocks: &'a [Block],
	theme: Option<&str>,
	out: &mut Vec<(u64, &'a str, &'a str)>,
) {
	for block in blocks {
		match &block.kind {
			BlockKind::Code { language, text } => {
				out.push((key(language, text, theme), language, text));
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. } => collect(blocks, theme, out),
			BlockKind::List { items, .. } => {
				for item in items {
					collect(&item.blocks, theme, out);
				}
			}
			_ => {}
		}
	}
}

/// How long an export waits for the cosmetic highlighting pass.
const HIGHLIGHT_WAIT: Duration = Duration::from_secs(30);
