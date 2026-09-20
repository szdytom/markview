//! Latest-request-wins document processing, independent of file observation.
use crate::{
	document,
	file::read_document,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
};
use std::{
	path::PathBuf,
	sync::{
		Arc, Condvar, Mutex,
		atomic::{AtomicU32, AtomicU64, Ordering},
	},
	thread,
	time::{Duration, Instant},
};
/// How much of a fresh document the opening viewport is parsed from before the
/// complete parse runs.
const PREFIX_BYTES: usize = 64 * 1024;

/// Text and geometry are accepted together by the UI.
#[derive(Clone, Debug)]
pub struct ReaderSnapshot {
	pub document: Arc<document::Document>,
	pub layout: LayoutSnapshot,
	pub content_version: u64,
	pub complete: bool,
	/// Remote image sources the per-revision cap left unrequested.
	pub remote_deferred: usize,
}
#[derive(Clone)]
pub struct Request {
	pub version: u64,
	pub content_version: u64,
	pub path: PathBuf,
	pub options: LayoutOptions,
	pub requested: Instant,
	/// Initial viewport bottom plus prefetch, in document coordinates.
	pub coverage: f32,
	/// This tab's reader lifted the remote-image cap for this revision.
	pub load_all_images: bool,
}
#[derive(Clone)]
pub struct Update {
	pub version: u64,
	pub path: PathBuf,
	pub result: Option<Result<ReaderSnapshot, String>>,
	pub requested: Instant,
	pub read_ms: f64,
	pub parse_ms: f64,
	pub layout_ms: f64,
	/// Reading counts for a complete snapshot whose content changed, so the
	/// event loop does not have to segment the whole document to draw the
	/// footer.
	pub counts: Option<markview_core::text::TextCounts>,
}

struct Inbox {
	pending: Option<Request>,
	stopped: bool,
	/// Set when the last tab closes: drop the parsed document and its caches.
	release: bool,
}

/// Publication decisions use elapsed layout time, so small but expensive
/// documents can show completed blocks without waiting for a full viewport.
struct PrefixPublication {
	large: bool,
	blocks: usize,
	target: f32,
	at: Duration,
}
impl PrefixPublication {
	fn new(bytes: usize) -> Self {
		Self {
			large: bytes >= 32 * 1024,
			blocks: 0,
			target: -1.,
			at: Duration::ZERO,
		}
	}
	fn publish(
		&mut self,
		blocks: usize,
		height: f32,
		target: f32,
		elapsed: Duration,
	) -> bool {
		let overdue = elapsed >= Duration::from_millis(32);
		if blocks == 0 || (!self.large && !overdue) {
			return false;
		}
		let ready =
			height >= target || (!self.large && overdue && self.blocks == 0);
		let demand = target != self.target;
		let batch = blocks >= (self.blocks * 2).max(1)
			&& elapsed.saturating_sub(self.at) >= Duration::from_millis(32);
		if !ready || !(demand || batch) {
			return false;
		}
		self.blocks = blocks;
		self.target = target;
		self.at = elapsed;
		true
	}
}

pub struct Worker {
	inbox: Arc<(Mutex<Inbox>, Condvar)>,
	version: Arc<AtomicU64>,
	coverage: Arc<AtomicU32>,
	handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
	#[cfg(test)]
	pub fn new(done: impl Fn(Update) + Send + 'static) -> Self {
		Self::with_images(true, crate::test_support::fonts(), done)
	}
	pub fn with_images(
		offline: bool,
		fonts: markview_core::fonts::FontConfig,
		done: impl Fn(Update) + Send + 'static,
	) -> Self {
		let inbox = Arc::new((
			Mutex::new(Inbox {
				pending: None,
				stopped: false,
				release: false,
			}),
			Condvar::new(),
		));
		let thread_inbox = inbox.clone();
		let version = Arc::new(AtomicU64::new(0));
		let current = version.clone();
		let coverage = Arc::new(AtomicU32::new(f32::INFINITY.to_bits()));
		let target = coverage.clone();
		let handle = thread::Builder::new()
			.name("markview-layout".into())
			.stack_size(8 * 1024 * 1024)
			.spawn(move || {
				// Discover the configured fonts here, while the window and
				// renderer initialize on the main thread; every later shaper
				// clones the resulting collection instead of scanning again.
				crate::layout::TextShaper::warm_fonts(&fonts);
				let mut engine = LayoutEngine::new();
				let mut images = crate::images::Images::new(offline);
				let mut last: Option<Request> = None;
				let mut completed_version = 0;
				// Reads counts for the last content identity, reused by every
				// later update that carries the same content.
				let mut counted: Option<(
					u64,
					markview_core::text::TextCounts,
				)> = None;
				let mut cached: Option<(
					PathBuf,
					u64,
					Arc<document::Document>,
				)> = None;
				loop {
					let request = {
						let (lock, wake) = &*thread_inbox;
						let mut inbox = lock.lock().unwrap();
						while inbox.pending.is_none()
							&& !inbox.stopped && !inbox.release
						{
							inbox = wake
								.wait_timeout(
									inbox,
									std::time::Duration::from_millis(50),
								)
								.unwrap()
								.0;
							if inbox.pending.is_none() && images.poll() {
								inbox.pending = last.clone();
							}
							if inbox.pending.is_none()
								&& last.is_some() && engine.poll_highlights()
							{
								inbox.pending = last.clone();
							}
						}
						if inbox.stopped {
							break;
						}
						if std::mem::take(&mut inbox.release) {
							cached = None;
							last = None;
							counted = None;
							engine.release_document();
							images.release();
							continue;
						}
						inbox.pending.take().unwrap()
					};
					last = Some(request.clone());
					let mut update = Update {
						version: request.version,
						path: request.path.clone(),
						requested: request.requested,
						result: None,
						read_ms: 0.0,
						parse_ms: 0.0,
						layout_ms: 0.0,
						counts: None,
					};
					update.result =
						Some((|| -> Result<ReaderSnapshot, String> {
							let document = if let Some((path, revision, doc)) =
								&cached && path == &request.path
								&& *revision == request.content_version
							{
								doc.clone()
							} else {
								let start = Instant::now();
								let text = read_document(&request.path)
									.map_err(|e| format!("{e:#}"))?;
								update.read_ms =
									start.elapsed().as_secs_f64() * 1000.0;
								let start = Instant::now();
								let source: Arc<str> = text.into();
								// A small edit to the document already held
								// re-parses only the block it changed.
								let previous = cached
									.as_ref()
									.filter(|(path, ..)| path == &request.path)
									.map(|(_, _, doc)| doc.clone());
								let doc = match previous {
									Some(previous) => {
										Arc::new(document::reparse(
											&previous,
											source.clone(),
										))
									}
									None => {
										// A fresh document that is large
										// enough to pay for it shows its
										// opening viewport from a bounded
										// parse instead of waiting for the
										// whole file.
										let prefix_start = Instant::now();
										if current.load(Ordering::Relaxed)
											== request.version && let Some(
											prefix,
										) =
											document::parse_prefix(
												&source,
												PREFIX_BYTES,
											) {
											let prefix_parse = prefix_start
												.elapsed()
												.as_secs_f64()
												* 1000.0;
											engine
												.validate_stylesheet(
													&request.options.stylesheet,
												)
												.map_err(|e| {
													format!("Fonts: {e:#}")
												})?;
											images.prepare(
												&prefix,
												&request.path,
												request.content_version,
												request.load_all_images,
												&request.options.stylesheet,
											);
											// Stop at the viewport rather
											// than laying out the whole
											// prefix, so the cost follows
											// the viewport, not the bound.
											let layout_start = Instant::now();
											let wanted = f32::from_bits(
												target.load(Ordering::Relaxed),
											);
											let mut shown = None;
											engine.layout_progressive(
												&prefix,
												&request.options,
												&images.snapshot,
												|snapshot| {
													if !snapshot
														.blocks
														.is_empty() && snapshot
														.height
														>= wanted
													{
														shown = Some(
															snapshot.clone(),
														);
														return false;
													}
													true
												},
											);
											if let Some(layout) = shown {
												let mut partial =
													update.clone();
												partial.parse_ms = prefix_parse;
												partial.layout_ms = layout_start
													.elapsed()
													.as_secs_f64()
													* 1000.0;
												partial.result =
													Some(Ok(ReaderSnapshot {
														document: Arc::new(
															prefix,
														),
														layout,
														content_version:
															request
																.content_version,
														complete: false,
														remote_deferred: images
															.deferred_remote(),
													}));
												done(partial);
											}
										}
										Arc::new(document::parse(
											source.clone(),
										))
									}
								};
								update.parse_ms =
									start.elapsed().as_secs_f64() * 1000.0;
								if cached.as_ref().is_some_and(
									|(path, _, _)| path != &request.path,
								) {
									engine.clear_document_cache();
								}
								cached = Some((
									request.path.clone(),
									request.content_version,
									doc.clone(),
								));
								doc
							};
							if current.load(Ordering::Relaxed)
								!= request.version
							{
								return Err("Superseded".into());
							}
							let start = Instant::now();
							engine
								.validate_stylesheet(
									&request.options.stylesheet,
								)
								.map_err(|e| format!("Fonts: {e:#}"))?;
							images.prepare(
								&document,
								&request.path,
								request.content_version,
								request.load_all_images,
								&request.options.stylesheet,
							);
							let mut publication =
								PrefixPublication::new(document.source.len());
							let layout = engine
								.layout_progressive(
									&document,
									&request.options,
									&images.snapshot,
									|prefix| {
										if current.load(Ordering::Relaxed)
											!= request.version
										{
											return false;
										}
										let wanted = f32::from_bits(
											target.load(Ordering::Relaxed),
										);
										if request.version != completed_version
											&& publication.publish(
												prefix.blocks.len(),
												prefix.height,
												wanted,
												start.elapsed(),
											) {
											let mut partial = update.clone();
											partial.layout_ms =
												start.elapsed().as_secs_f64()
													* 1000.;
											partial.result =
												Some(Ok(ReaderSnapshot {
													document: document.clone(),
													layout: prefix.clone(),
													content_version: request
														.content_version,
													complete: false,
													remote_deferred: images
														.deferred_remote(),
												}));
											done(partial);
										}
										true
									},
								)
								.ok_or_else(|| "Superseded".to_string())?;
							update.layout_ms =
								start.elapsed().as_secs_f64() * 1000.0;
							Ok(ReaderSnapshot {
								document,
								layout,
								content_version: request.content_version,
								complete: true,
								remote_deferred: images.deferred_remote(),
							})
						})());
					if current.load(Ordering::Relaxed) == request.version {
						if update.result.as_ref().is_some_and(|r| r.is_ok()) {
							completed_version = request.version;
						}
						// Counting the reading text is expensive, so it happens
						// here, after any prefix has been published, and never
						// while a newer edit is already waiting for the worker.
						// The result is cached by content identity and attached
						// to every later complete update for it, so whichever
						// session receives that update gets the counts.
						let next = match &update.result {
							Some(Ok(reader)) if reader.complete => {
								match counted {
									Some((id, counts))
										if id == reader.document.content_id =>
									{
										Some(counts)
									}
									_ => {
										let pending = {
											let (lock, _) = &*thread_inbox;
											lock.lock()
												.unwrap()
												.pending
												.is_some()
										};
										(!pending).then(|| {
											let counts = reader
												.layout
												.select_all(
													reader.content_version,
												)
												.map(|selection| {
													markview_core::text::TextCounts::of(
														&reader.layout.extract_text(
															selection,
															reader.content_version,
														),
													)
												})
												.unwrap_or_default();
											counted = Some((
												reader.document.content_id,
												counts,
											));
											counts
										})
									}
								}
							}
							_ => None,
						};
						update.counts = next;
						done(update);
					}
				}
			})
			.expect("start layout worker");
		Self {
			inbox,
			version,
			coverage,
			handle: Some(handle),
		}
	}
	pub fn submit(&self, request: Request) {
		self.coverage
			.store(request.coverage.to_bits(), Ordering::Relaxed);
		self.version.store(request.version, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		lock.lock().unwrap().pending = Some(request);
		wake.notify_one();
	}
	pub fn prioritize(&self, coverage: f32) {
		self.coverage.store(coverage.to_bits(), Ordering::Relaxed);
	}
	pub fn cancel(&self) {
		self.version.store(0, Ordering::Relaxed);
		let (lock, _) = &*self.inbox;
		lock.lock().unwrap().pending = None;
	}
	/// Drops the document the worker keeps for the reader that just closed, so
	/// an empty reader holds no text, geometry or decoded images.
	pub fn release(&self) {
		self.cancel();
		let (lock, wake) = &*self.inbox;
		lock.lock().unwrap().release = true;
		wake.notify_one();
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
		self.version.store(0, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		lock.lock().unwrap().stopped = true;
		wake.notify_one();
		if let Some(t) = self.handle.take() {
			let _ = t.join();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{fs, sync::mpsc, time::Duration};
	#[test]
	fn small_document_publication_uses_time_and_preserves_batching() {
		let mut p = PrefixPublication::new(3691);
		assert!(!p.publish(1, 900., 600., Duration::from_millis(31)));
		assert!(!p.publish(0, 0., 600., Duration::from_millis(32)));
		// An expensive first block can publish even below viewport coverage.
		assert!(p.publish(1, 80., 600., Duration::from_millis(32)));
		assert!(!p.publish(2, 900., 600., Duration::from_millis(33)));
		assert!(p.publish(2, 900., 600., Duration::from_millis(64)));
		assert!(!p.publish(3, 1000., 600., Duration::from_millis(96)));
		// A changed viewport target bypasses the batch gate once covered.
		assert!(p.publish(3, 1000., 950., Duration::from_millis(97)));
		assert!(!p.publish(4, 1100., 1800., Duration::from_millis(130)));
		assert!(p.publish(5, 1900., 1800., Duration::from_millis(131)));
	}
	#[test]
	fn large_document_publication_keeps_viewport_coverage() {
		let mut p = PrefixPublication::new(32 * 1024);
		assert!(!p.publish(1, 80., 600., Duration::from_millis(40)));
		assert!(p.publish(2, 900., 600., Duration::from_millis(41)));
		let mut fast = PrefixPublication::new(32 * 1024);
		assert!(fast.publish(2, 900., 600., Duration::from_millis(1)));
	}
	#[test]
	fn progressive_worker_chases_target_and_cancels_changed_file() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "A paragraph 中文 with **text**.\n\n".repeat(1000))
			.unwrap();
		let (tx, rx) = mpsc::channel();
		let (resume, gate) = mpsc::channel();
		let gate = Mutex::new(gate);
		let paused = AtomicU64::new(0);
		let worker = Worker::new(move |u| {
			let pause =
				u.version == 1 && paused.fetch_add(1, Ordering::Relaxed) < 2;
			tx.send(u).unwrap();
			if pause {
				gate.lock()
					.unwrap()
					.recv_timeout(Duration::from_secs(10))
					.unwrap();
			}
		});
		let request = Request {
			version: 1,
			content_version: 1,
			path: path.clone(),
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: 600.,
			load_all_images: false,
		};
		worker.submit(request.clone());
		let first = rx
			.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert!(!first.complete);
		assert!(first.layout.height >= 600.);
		assert!(first.layout.blocks.len() < first.document.blocks.len());
		worker.prioritize(6000.);
		resume.send(()).unwrap();
		let next = rx
			.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert!(!next.complete);
		assert!(next.layout.height >= 6000.);
		assert!(Arc::ptr_eq(
			&first.layout.blocks[0].layout,
			&next.layout.blocks[0].layout
		));
		fs::write(&path, "Replacement").unwrap();
		worker.submit(Request {
			version: 2,
			content_version: 2,
			..request
		});
		resume.send(()).unwrap();
		let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
		assert_eq!(update.version, 2);
		let final_reader = update.result.unwrap().unwrap();
		assert!(final_reader.complete);
		assert_eq!(&*final_reader.document.source, "Replacement");
		assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
	}

	#[test]
	fn progressive_worker_finishes_with_shared_prefix() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(
			&path,
			format!("{}\n\n", "Paragraph. ".repeat(40)).repeat(100),
		)
		.unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			tx.send(u).unwrap();
		});
		worker.submit(Request {
			version: 1,
			content_version: 1,
			path,
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: 600.,
			load_all_images: false,
		});
		let first = rx
			.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert!(!first.complete);
		loop {
			let reader = rx
				.recv_timeout(Duration::from_secs(10))
				.unwrap()
				.result
				.unwrap()
				.unwrap();
			if reader.complete {
				assert_eq!(reader.layout.blocks.len(), 100);
				assert!(Arc::ptr_eq(
					&first.layout.blocks[0].layout,
					&reader.layout.blocks[0].layout
				));
				break;
			}
		}
	}
	#[test]
	fn worker_publishes_latest_request() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "A paragraph.").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		for version in 1..=20 {
			worker.submit(Request {
				version,
				content_version: 1,
				path: path.clone(),
				options: LayoutOptions {
					width: 250.0 + version as f32,
					fonts: crate::test_support::fonts(),
					..Default::default()
				},
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
			});
		}
		loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == 20 {
				assert_eq!(update.result.unwrap().unwrap().layout.width, 270.0);
				break;
			}
		}
		assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
	}
}

#[cfg(test)]
mod reflow_tests {
	use super::*;
	use std::{fs, sync::mpsc, time::Duration};
	#[test]
	fn reflow_reuses_document_without_reading_and_reload_is_not_lost() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "First").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			tx.send(u).unwrap();
		});
		let submit = |version, content_version| {
			worker.submit(Request {
				version,
				content_version,
				path: path.clone(),
				options: crate::test_support::options(),
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
			})
		};
		submit(1, 1);
		let first = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		fs::remove_file(&path).unwrap();
		submit(2, 1);
		let reflow = rx.recv_timeout(Duration::from_secs(5)).unwrap();
		assert_eq!(reflow.read_ms, 0.0);
		assert_eq!(reflow.parse_ms, 0.0);
		assert!(Arc::ptr_eq(
			&first.document,
			&reflow.result.unwrap().unwrap().document
		));
		submit(3, 2);
		assert!(
			rx.recv_timeout(Duration::from_secs(5))
				.unwrap()
				.result
				.unwrap()
				.is_err()
		);
		fs::write(&path, "Second").unwrap();
		submit(4, 2);
		submit(5, 2);
		loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == 5 {
				assert_eq!(
					&*update.result.unwrap().unwrap().document.source,
					"Second"
				);
				break;
			}
		}
		assert_eq!(&*first.document.source, "First");
	}

	#[test]
	fn release_drops_the_retained_document() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "First").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let request = |version| Request {
			version,
			content_version: 1,
			path: path.clone(),
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: f32::INFINITY,
			load_all_images: false,
		};
		worker.submit(request(1));
		let first = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert_eq!(&*first.document.source, "First");
		// The same content version reuses the retained parse.
		fs::write(&path, "Second").unwrap();
		worker.submit(request(2));
		let reused = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert_eq!(&*reused.document.source, "First");
		// Once nothing holds the document, the worker reads it again.
		worker.release();
		worker.submit(request(3));
		let reread = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert_eq!(&*reread.document.source, "Second");
	}

	#[test]
	fn complete_updates_carry_counts_to_each_document() {
		let dir = tempfile::tempdir().unwrap();
		let first = dir.path().join("a.md");
		let second = dir.path().join("b.md");
		// Identical content in two files: the second open shares the cached
		// content identity, but its own session still needs the counts.
		fs::write(&first, "Some words to count.\n").unwrap();
		fs::write(&second, "Some words to count.\n").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let submit = |version, path: &PathBuf| {
			worker.submit(Request {
				version,
				content_version: 1,
				path: path.clone(),
				options: crate::test_support::options(),
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
			})
		};
		submit(1, &first);
		let counts = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.counts
			.expect("counts for the first document");
		assert!(counts.chars > 0 && counts.words > 0);
		submit(2, &second);
		let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
		assert!(update.result.unwrap().is_ok());
		assert_eq!(update.counts, Some(counts));
	}
}
