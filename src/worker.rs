//! Latest-request-wins document processing, independent of file observation.
use crate::{
	file::read_markdown,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
};
use std::{
	path::PathBuf,
	sync::{Arc, Condvar, Mutex, atomic::Ordering},
	thread,
	time::{Duration, Instant},
};
/// How much of a fresh document the opening viewport is parsed from before the
/// complete parse runs.
const PREFIX_BYTES: usize = 64 * 1024;

/// Text and geometry are accepted together by the UI.
#[derive(Clone, Debug)]
pub struct ReaderSnapshot {
	pub blocked_images:
		std::collections::HashMap<String, crate::security::Resource>,
	pub document: Arc<crate::loaded::LoadedDocument>,
	pub layout: LayoutSnapshot,
	pub content_version: u64,
	pub complete: bool,
	pub parse_complete: bool,
	/// Remote image sources the per-revision cap left unrequested.
	pub remote_deferred: usize,
}
#[derive(Clone)]
pub struct Request {
	pub security: crate::security::Security,
	pub version: u64,
	pub content_version: u64,
	pub path: PathBuf,
	/// A document already parsed by its front end, when no file is read.
	pub loaded: Option<Arc<crate::loaded::LoadedDocument>>,
	/// The embed state already accepted by the requesting tab at this revision.
	pub embed_baseline: Option<EmbedBaseline>,
	/// The open folder root, used by Obsidian note and image embeds.
	pub workspace: Option<PathBuf>,
	pub options: LayoutOptions,
	pub requested: Instant,
	/// Initial viewport bottom plus prefetch, in document coordinates.
	pub coverage: f32,
	/// This tab's reader lifted the remote-image cap for this revision.
	pub load_all_images: bool,
	pub requested_image: Option<markview_layout::image::ImageSource>,
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
	pub counts: Option<markview_layout::text::TextCounts>,
}

type Inbox = crate::mailbox::Inbox<Request>;

#[cfg(test)]
mod proptests;

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

#[derive(Default)]
struct EmbedWatch(Option<EmbedObservation>);

struct EmbedObservation {
	path: PathBuf,
	content_version: u64,
	baseline: EmbedBaseline,
	next: Option<Instant>,
}

#[derive(Clone)]
pub(crate) struct EmbedBaseline {
	raw: Option<markview_document::ContentKey>,
	content: u64,
	dependencies: crate::embed::Dependencies,
}

impl EmbedBaseline {
	pub(crate) fn from_document(
		document: &crate::loaded::LoadedDocument,
	) -> Self {
		let dependencies = document
			.composition()
			.map(|composition| composition.dependencies.clone())
			.unwrap_or_default();
		Self {
			raw: document
				.markdown_parse()
				.map(|parse| parse.document.content_key()),
			content: document.content_key().get(),
			dependencies,
		}
	}

	pub(crate) fn after_release(content: u64) -> Self {
		Self {
			raw: None,
			content,
			dependencies: Default::default(),
		}
	}

	fn invalidated(&self, document: &crate::loaded::LoadedDocument) -> bool {
		self.dependencies.changed()
			|| (self.content != document.content_key().get()
				&& document.markdown_parse().is_some_and(|parse| {
					parse.document.has_note_embeds()
						&& self.raw.is_none_or(|raw| {
							parse.document.content_key() == raw
						})
				}))
	}
}

impl EmbedWatch {
	fn observe(
		&mut self,
		request: &Request,
		document: &crate::loaded::LoadedDocument,
	) {
		let baseline = EmbedBaseline::from_document(document);
		self.0 = Some(EmbedObservation {
			path: request.path.clone(),
			content_version: request.content_version,
			next: (!baseline.dependencies.is_empty())
				.then(|| Instant::now() + crate::embed::CHECK_INTERVAL),
			baseline,
		});
	}

	fn deadline(&self, fallback: Instant) -> Instant {
		self.0
			.as_ref()
			.and_then(|observed| observed.next)
			.map_or(fallback, |next| fallback.min(next))
	}

	fn pause(&mut self) {
		if let Some(observed) = &mut self.0 {
			observed.next = None;
		}
	}

	fn changed(&mut self, now: Instant) -> bool {
		let Some(observed) = &mut self.0 else {
			return false;
		};
		if observed.next.is_none_or(|next| now < next) {
			return false;
		}
		let changed = observed.baseline.dependencies.changed();
		observed.next =
			(!changed).then_some(now + crate::embed::CHECK_INTERVAL);
		changed
	}

	fn needs_revision(
		&self,
		request: &Request,
		document: &crate::loaded::LoadedDocument,
	) -> bool {
		let baseline = request.embed_baseline.as_ref().or_else(|| {
			let observed = self.0.as_ref()?;
			(observed.path == request.path
				&& observed.content_version == request.content_version)
				.then_some(&observed.baseline)
		});
		baseline.is_some_and(|baseline| baseline.invalidated(document))
	}
}

fn reuse_markdown(
	previous: Arc<markview_commonmark::MarkdownParse>,
	source: &Arc<str>,
	dialect: markview_commonmark::Dialect,
) -> Arc<markview_commonmark::MarkdownParse> {
	if previous.source == *source {
		previous
	} else {
		Arc::new(markview_commonmark::reparse_with(
			&previous,
			source.clone(),
			dialect,
		))
	}
}

type CachedCounts = Option<(u64, markview_layout::text::TextCounts)>;

pub struct Worker {
	cached_counts: Arc<Mutex<CachedCounts>>,
	_services: Arc<crate::services::Services>,
	inbox: Arc<(Mutex<Inbox>, Condvar)>,
	control: Arc<crate::mailbox::Control>,
	presented: Arc<std::sync::atomic::AtomicU64>,
	handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
	#[cfg(test)]
	pub fn new(done: impl Fn(Update) + Send + 'static) -> Self {
		Self::with_images(true, crate::test_support::fonts(), done)
	}
	#[cfg(any(test, feature = "diagnostics"))]
	pub fn with_images(
		offline: bool,
		fonts: markview_layout::fonts::FontConfig,
		done: impl Fn(Update) + Send + 'static,
	) -> Self {
		Self::with_images_and_parsed(offline, fonts, done, |_, _, _| {})
	}
	#[cfg(any(test, feature = "diagnostics"))]
	pub fn with_images_and_parsed(
		offline: bool,
		fonts: markview_layout::fonts::FontConfig,
		done: impl Fn(Update) + Send + 'static,
		parsed: impl Fn(PathBuf, u64, Arc<crate::loaded::LoadedDocument>)
		+ Send
		+ 'static,
	) -> Self {
		Self::with_services_and_parsed(
			Arc::new(crate::services::Services::new(4)),
			offline,
			fonts,
			done,
			parsed,
			|_, _| {},
		)
	}
	pub(crate) fn with_services_and_parsed(
		services: Arc<crate::services::Services>,
		offline: bool,
		fonts: markview_layout::fonts::FontConfig,
		done: impl Fn(Update) + Send + 'static,
		parsed: impl Fn(PathBuf, u64, Arc<crate::loaded::LoadedDocument>)
		+ Send
		+ 'static,
		embed_changed: impl Fn(PathBuf, u64) + Send + 'static,
	) -> Self {
		let inbox = Arc::new((Mutex::new(Inbox::default()), Condvar::new()));
		let thread_inbox = inbox.clone();
		let control = Arc::new(crate::mailbox::Control::default());
		control
			.coverage
			.store(f32::INFINITY.to_bits(), Ordering::Relaxed);
		let presented = Arc::new(std::sync::atomic::AtomicU64::new(0));
		let cached_counts = Arc::new(Mutex::new(None));
		let restored_counts = cached_counts.clone();
		let rendered = presented.clone();
		let current = control.clone();
		let target = control.clone();
		let io = services.handle.clone();
		let handle = thread::Builder::new()
			.name("markview-layout".into())
			.stack_size(8 * 1024 * 1024)
			.spawn(move || {
				// Discover the configured fonts here, while the window and
				// renderer initialize on the main thread; every later shaper
				// clones the resulting collection instead of scanning again.
				crate::layout::TextShaper::warm_fonts(&fonts);
				let wake: markview_layout::background::Wake = Arc::new({
					let inbox = thread_inbox.clone();
					move || {
						let (lock, signal) = &*inbox;
						Inbox::lock(lock).background_ready = true;
						signal.notify_one();
					}
				});
				let mut engine =
					LayoutEngine::with_executor(io.cpu.clone(), wake.clone());
				engine.set_auto_parallelism();
				let mut images = crate::images::Images::with_services(
					offline,
					crate::images::cache_directory(),
					fonts.clone(),
					io,
					None,
					wake,
				);
				let mut last: Option<Request> = None;
				let mut completed_version = 0;
				// Reads counts for the last content identity, reused by every
				// later update that carries the same content.
				let mut counted: Option<(
					u64,
					markview_layout::text::TextCounts,
				)> = None;
				let mut cached: Option<(
					PathBuf,
					u64,
					markview_commonmark::Dialect,
					Arc<markview_commonmark::MarkdownParse>,
				)> = None;
				let mut embeds = crate::embed::Loader::default();
				let mut embed_poll = EmbedWatch::default();
				loop {
					let deadline = embed_poll.deadline(images.poll_deadline());
					let Some((request, release)) = wait_for_request(&thread_inbox, deadline) else {
                        break;
                    };
					if release {
						cached = None;
						embeds = Default::default();
						embed_poll = EmbedWatch::default();
						last = None;
						counted = None;
						engine.release_document();
						images.release();
						continue;
					}
					let Some(request) = request else {
						if embed_poll.changed(Instant::now()) {
							let request = last.as_ref().unwrap();
							embed_changed(request.path.clone(), request.version);
							continue;
						}
						let changed = images.poll();
						let highlights =
							last.is_some() && engine.poll_highlights();
						if changed || highlights {
							requeue_background(&thread_inbox, last.as_ref());
						}
						continue;
					};
					let mut request = request;
					if last.as_ref().is_some_and(|last| last.options != request.options) {
						counted = None;
					}
					last = Some(request.clone());
					images.set_workspace(request.workspace.clone());
					images.set_standalone_image(
						crate::file::is_picture(&request.path)
							.then(|| request.path.clone()),
					);
					engine.set_parallel_start(request.coverage);
					let rendered = rendered.clone();
					let version = request.version;
					engine.set_parallel_ready(Arc::new(move || rendered.load(Ordering::Relaxed) == version));
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
					let mut refresh_embeds = false;
					update.result =
						Some((|| -> Result<ReaderSnapshot, String> {
							let dialect = markview_commonmark::Dialect {
								hard_breaks: request.options.hard_breaks,
								highlight_marks: request.options.highlight_marks,
								ofm: request.options.ofm,
								grid_tables: request.options.grid_tables,
								math_dollars_block: request.options.math_dollars_block,
							};
							let document = if let Some(loaded) =
								&request.loaded
							{
								loaded.with_dialect(dialect)
							} else if let Some((
								path,
								revision,
								cached_dialect,
								doc,
							)) = &cached
								&& (path, revision, cached_dialect) == (&request.path, &request.content_version, &dialect)
							{
								request.security =
									request.security.for_content(&doc.source);
								crate::loaded::LoadedDocument::markdown(
									doc.clone(),
								)
							} else {
								let start = Instant::now();
								let text = read_markdown(&request.path)
									.map_err(|e| format!("{e:#}"))?;
								request.security =
									request.security.for_content(&text);
								update.read_ms =
									start.elapsed().as_secs_f64() * 1000.0;
								let start = Instant::now();
								let source: Arc<str> = text.into();
								// A small edit to the document already held
								// re-parses only the block it changed.
								let previous = cached
									.as_ref()
									.filter(|(path, _, cached_dialect, _)| {
										path == &request.path
											&& *cached_dialect == dialect
									})
									.map(|(_, _, _, doc)| doc.clone());
								let doc = match previous {
									Some(previous) => reuse_markdown(previous, &source, dialect),
									None => {
										// A fresh document that is large
										// enough to pay for it shows its
										// opening viewport from a bounded
										// parse instead of waiting for the
										// whole file.
										let prefix_start = Instant::now();
										if current
											.sequence
											.load(Ordering::Relaxed) == request
											.version && let Some(prefix) =
											markview_commonmark::parse_prefix_with(
												&source,
												PREFIX_BYTES,
												dialect,
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
											images.set_security(
												request.security.clone(),
											);
											images.set_requested_image(request.requested_image.clone());
											images.set_embed_paths(None);
											images.set_initial_width(
												request.options.width,
											);
											images.prepare(
												&prefix,
												&request.path,
												request.content_version,
												request.load_all_images,
												&request.options.stylesheet,
												&request.options.fonts,
											);
											// Stop at the viewport rather
											// than laying out the whole
											// prefix, so the cost follows
											// the viewport, not the bound.
											let layout_start = Instant::now();
											let wanted = f32::from_bits(
												target
													.coverage
													.load(Ordering::Relaxed),
											);
											let mut shown = None;
											engine.layout_progressive_cancellable(
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
											|| {
													current.sequence.load(Ordering::Relaxed)
														!= request.version
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
														blocked_images: images
															.blocked
															.clone(),
														document: crate::loaded::LoadedDocument::markdown(
															Arc::new(prefix),
														),
														layout,
														content_version:
															request
																.content_version,
														complete: false,
														parse_complete: false,
														remote_deferred: images
															.deferred_remote(),
													}));
												done(partial);
											}
										}
										Arc::new(
											markview_commonmark::parse_with(
												source.clone(),
												dialect,
											),
										)
									}
								};
								update.parse_ms =
									start.elapsed().as_secs_f64() * 1000.0;
								if cached.as_ref().is_some_and(
									|(path, _, _, _)| path != &request.path,
								) {
									engine.release_document();
								}
								cached = Some((
									request.path.clone(),
									request.content_version,
									dialect,
									doc.clone(),
								));
								crate::loaded::LoadedDocument::markdown(doc)
							};
							let document = embeds.prepare(
								document,
								crate::embed::InputPath::File(&request.path),
								request.workspace.as_deref(),
								&request.security,
								|| current.sequence.load(Ordering::Relaxed) != request.version,
							).filter(|document| {
								refresh_embeds = embed_poll.needs_revision(&request, document);
								!refresh_embeds
							}).ok_or_else(|| "Superseded".to_string())?;
							embed_poll.observe(&request, &document);
							parsed(
								request.path.clone(),
								request.content_version,
								document.clone(),
							);
							let start = Instant::now();
							engine
								.validate_stylesheet(
									&request.options.stylesheet,
								)
								.map_err(|e| format!("Fonts: {e:#}"))?;
							images.set_security(request.security.clone());
							images.set_embed_paths(document.embed_paths());
							images.set_requested_image(request.requested_image.clone());
							images.set_initial_width(request.options.width);
							images.prepare(
								&document,
								&request.path,
								request.content_version,
								request.load_all_images,
								&request.options.stylesheet,
								&request.options.fonts,
							);
							// Small documents publish on completion; avoid starting workers
							// before that first readable snapshot. A web page has no source
							// text, so the estimate comes from its semantic text.
							let size = document.size();
							engine.set_parallel_start(if size < 32 * 1024 {
								f32::INFINITY
							} else {
								request.coverage
							});
							let mut publication =
								PrefixPublication::new(size);
							let layout = engine
								.layout_progressive_cancellable(
									&document,
									&request.options,
									&images.snapshot,
									|prefix| {
										if current
											.sequence
											.load(Ordering::Relaxed) != request
											.version
										{
											return false;
										}
										let wanted = f32::from_bits(
											target
												.coverage
												.load(Ordering::Relaxed),
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
													blocked_images: images
														.blocked
														.clone(),
													document: document.clone(),
													layout: prefix.clone(),
													content_version: request
														.content_version,
													complete: false,
													parse_complete: true,
													remote_deferred: images
														.deferred_remote(),
												}));
											done(partial);
										}
										true
									},
								|| {
										current.sequence.load(Ordering::Relaxed) != request.version
									},
								)
								.ok_or_else(|| "Superseded".to_string())?;
							update.layout_ms =
								start.elapsed().as_secs_f64() * 1000.0;
							Ok(ReaderSnapshot {
								blocked_images: images.blocked.clone(),
								document: document.clone(),
								layout,
								content_version: request.content_version,
								complete: true,
								parse_complete: true,
								remote_deferred: images.deferred_remote(),
							})
						})());
					if refresh_embeds {
						embed_poll.pause();
						embed_changed(request.path.clone(), request.version);
						continue;
					}
					if current.sequence.load(Ordering::Relaxed)
						== request.version
					{
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
								let restored = *restored_counts.lock().unwrap();
								let id = reader.document.document().content_key().get();
								match counted.filter(|(key, _)| *key == id).or(restored) {
									Some((id, counts))
										if id == reader.document.document().content_key().get() =>
									{
										Some(counts)
									}
									_ => {
										let pending = {
											let (lock, _) = &*thread_inbox;
											Inbox::lock(lock).pending.is_some()
										};
										(!pending).then(|| {
											let counts = markview_layout::text::TextCounts::of(&reader.layout.full_reading_text());
											counted = Some((
												reader.document.document().content_key().get(),
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
				engine.release_document();
				images.release();
			})
			.expect("start layout worker");
		Self {
			cached_counts,
			_services: services,
			inbox,
			control,
			presented,
			handle: Some(handle),
		}
	}
	pub(crate) fn reuse_counts(&self, counts: CachedCounts) {
		*self.cached_counts.lock().unwrap() = counts;
	}
	pub fn submit(&self, request: Request) {
		self.control
			.coverage
			.store(request.coverage.to_bits(), Ordering::Relaxed);
		self.control
			.sequence
			.store(request.version, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		let mut inbox = Inbox::lock(lock);
		if inbox.stopped {
			return;
		}
		inbox.pending = Some(request);
		wake.notify_one();
	}
	/// Allows background block workers after this revision's readable GPU frame.
	pub fn presented(&self, version: u64) {
		self.presented.store(version, Ordering::Relaxed);
	}
	pub fn prioritize(&self, coverage: f32) {
		self.control
			.coverage
			.store(coverage.to_bits(), Ordering::Relaxed);
	}
	pub fn cancel(&self) {
		self.control.sequence.store(0, Ordering::Relaxed);
		let (lock, _) = &*self.inbox;
		Inbox::lock(lock).pending = None;
	}
	/// Drops the document the worker keeps for the reader that just closed, so
	/// an empty reader holds no text, geometry or decoded images.
	pub fn release(&self) {
		self.cancel();
		let (lock, wake) = &*self.inbox;
		Inbox::lock(lock).release = true;
		wake.notify_one();
	}
}
impl Worker {
	pub(crate) fn shutdown(&mut self) {
		self.control.sequence.store(0, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		{
			let mut inbox = Inbox::lock(lock);
			inbox.stopped = true;
			inbox.pending = None;
		}
		wake.notify_one();
		if let Some(t) = self.handle.take() {
			// A read stuck inside the open of a special file outlives the
			// token, so this join needs a deadline as well as a stop signal.
			crate::shutdown::join_within(t, "Layout worker");
		}
	}
}

impl Drop for Worker {
	fn drop(&mut self) {
		self.shutdown();
	}
}

fn requeue_background(
	shared: &(Mutex<Inbox>, Condvar),
	last: Option<&Request>,
) {
	let (lock, _) = shared;
	let mut inbox = Inbox::lock(lock);
	if inbox.pending.is_none() && !inbox.stopped && !inbox.release {
		inbox.pending = last.cloned();
	}
}

fn wait_for_request(
	shared: &(Mutex<Inbox>, Condvar),
	deadline: Instant,
) -> Option<(Option<Request>, bool)> {
	let (lock, wake) = shared;
	let mut inbox = Inbox::lock(lock);
	if inbox.pending.is_none()
		&& !inbox.stopped
		&& !inbox.release
		&& !inbox.background_ready
	{
		let waited = wake.wait_timeout(
			inbox,
			deadline.saturating_duration_since(Instant::now()),
		);
		inbox = Inbox::recover(
			lock,
			waited.map(|(guard, _)| guard).map_err(|error| {
				std::sync::PoisonError::new(error.into_inner().0)
			}),
		);
	}
	if inbox.stopped {
		return None;
	}
	inbox.background_ready = false;
	let release = std::mem::take(&mut inbox.release);
	Some((if release { None } else { inbox.pending.take() }, release))
}

#[cfg(test)]
fn same_document(
	a: &Arc<crate::loaded::LoadedDocument>,
	b: &Arc<crate::loaded::LoadedDocument>,
) -> bool {
	use crate::loaded::LoadedDocument;
	// A reflow wraps the same parse in a new handle, so identity is the
	// document the handle names, not the handle itself.
	match (a.as_ref(), b.as_ref()) {
		(LoadedDocument::Markdown(a), LoadedDocument::Markdown(b)) => {
			Arc::ptr_eq(a, b)
		}
		(
			LoadedDocument::Semantic { document: a, .. },
			LoadedDocument::Semantic { document: b, .. },
		) => Arc::ptr_eq(a, b),
		_ => false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		fs,
		sync::{atomic::AtomicU64, mpsc},
		time::Duration,
	};
	#[test]
	fn poisoned_inbox_accepts_a_new_layout_and_shuts_down() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("read.md");
		fs::write(&path, "Readable after recovery.").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |update| {
			let _ = tx.send(update);
		});
		crate::test_support::poison(&worker.inbox.0);
		worker.submit(Request {
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version: 1,
			content_version: 1,
			path,
			loaded: None,
			embed_baseline: None,
			workspace: None,
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: 600.0,
			load_all_images: false,
			requested_image: None,
		});
		let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
		assert!(update.result.unwrap().is_ok());
		assert!(!worker.inbox.0.is_poisoned());
		crate::test_support::poison(&worker.inbox.0);
		drop(worker);
	}

	#[test]
	fn note_embed_dependency_changes_rebuild_the_readers_document() {
		use crate::security::{Security, Trust};
		let directory = crate::test_support::TestDir::new();
		let path = directory.path().join("A.md");
		let note = directory.path().join("B.md");
		let source = "![[B#^leaf]]";
		fs::write(&path, source).unwrap();
		fs::write(&note, "First marker ^leaf").unwrap();
		let (updates, rendered) = mpsc::channel();
		let (changes, changed) = mpsc::channel();
		let worker = Worker::with_services_and_parsed(
			Arc::new(crate::services::Services::new(2)),
			true,
			crate::test_support::fonts(),
			move |update| updates.send(update).unwrap(),
			|_, _, _| {},
			move |path, version| changes.send((path, version)).unwrap(),
		);
		let mut options = crate::test_support::options();
		options.ofm = true;
		let submit = |version| {
			worker.submit(Request {
				security: Security::local(Trust::Trusted),
				version,
				content_version: version,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options: options.clone(),
				requested: Instant::now(),
				coverage: 600.0,
				load_all_images: false,
				requested_image: None,
			})
		};
		let receive = |version| loop {
			let update =
				rendered.recv_timeout(Duration::from_secs(10)).unwrap();
			if update.version == version {
				let reader = update.result.unwrap().unwrap();
				if reader.complete {
					break reader;
				}
			}
		};
		submit(1);
		let first = receive(1);
		assert!(first.layout.full_reading_text().contains("First marker"));
		fs::write(&note, "A different second marker ^leaf").unwrap();
		assert_eq!(
			changed.recv_timeout(Duration::from_secs(10)).unwrap(),
			(path.clone(), 1)
		);
		submit(2);
		let second = receive(2);
		assert!(
			second
				.layout
				.full_reading_text()
				.contains("A different second marker")
		);
		assert_eq!(fs::read_to_string(&path).unwrap(), source);
		assert!(Arc::ptr_eq(
			first.document.markdown_parse().unwrap(),
			second.document.markdown_parse().unwrap()
		));
	}

	#[test]
	fn note_embed_edit_requests_a_revision_before_a_queued_reflow() {
		use crate::security::{Security, Trust};
		enum Observed {
			Published(u64, u64, String),
			Changed(PathBuf, u64),
		}

		let directory = crate::test_support::TestDir::new();
		let path = directory.path().join("A.md");
		let note = directory.path().join("B.md");
		fs::write(&path, "![[B#Section]]").unwrap();
		fs::write(&note, "# Section\n\nOld embedded content").unwrap();
		let (events, received) = mpsc::channel();
		let changes = events.clone();
		let (resume, paused) = mpsc::channel();
		let worker = Worker::with_services_and_parsed(
			Arc::new(crate::services::Services::new(2)),
			true,
			crate::test_support::fonts(),
			move |update| {
				let Some(Ok(reader)) = update.result else {
					return;
				};
				if reader.complete {
					let _ = events.send(Observed::Published(
						update.version,
						reader.content_version,
						reader.layout.full_reading_text(),
					));
					if update.version == 1 {
						let _ = paused.recv_timeout(Duration::from_secs(5));
					}
				}
			},
			|_, _, _| {},
			move |path, version| {
				let _ = changes.send(Observed::Changed(path, version));
			},
		);
		let submit = |version, content_version, width| {
			let mut options = crate::test_support::options();
			options.ofm = true;
			options.width = width;
			worker.submit(Request {
				security: Security::local(Trust::Trusted),
				version,
				content_version,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options,
				requested: Instant::now(),
				coverage: 600.0,
				load_all_images: false,
				requested_image: None,
			});
		};
		submit(1, 1, 600.0);
		assert!(
			matches!(received.recv_timeout(Duration::from_secs(10)).unwrap(),
			Observed::Published(1, 1, text) if text.contains("Old embedded content"))
		);
		fs::write(&note, "# Section\n\nNew embedded content after edit")
			.unwrap();
		submit(2, 1, 550.0);
		resume.send(()).unwrap();

		loop {
			match received.recv_timeout(Duration::from_secs(10)).unwrap() {
				Observed::Changed(changed, version) => {
					assert_eq!((changed, version), (path.clone(), 2));
					break;
				}
				Observed::Published(_, 1, text) => {
					assert!(
						!text.contains("New embedded content"),
						"new text used the old content version"
					);
				}
				Observed::Published(..) => {
					panic!("unexpected reader publication")
				}
			}
		}
		submit(3, 2, 550.0);
		assert!(
			matches!(received.recv_timeout(Duration::from_secs(10)).unwrap(),
			Observed::Published(3, 2, text) if text.contains("New embedded content"))
		);
	}

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
	fn a_semantic_document_measures_its_own_text_for_the_large_path() {
		// A web page keeps no source text, so a size taken from `source()` would
		// leave every article on the small-document path.
		let body = "Article paragraph. ".repeat(2_000);
		let text = vec![markview_document::Inline {
			link: None,
			kind: markview_document::InlineKind::Text(body.clone()),
			style: Default::default(),
		}];
		let mut builder = markview_document::DocumentBuilder::new(
			markview_document::DocumentRevision::new(1),
		);
		let block = builder
			.block(Vec::new(), markview_document::BlockKind::Paragraph(text));
		let document = builder.finish(vec![block]);
		let loaded =
			crate::loaded::LoadedDocument::semantic(Arc::new(document));
		assert_eq!(loaded.source(), None);
		assert_eq!(loaded.size(), body.len());
		let mut publication = PrefixPublication::new(loaded.size());
		assert!(publication.publish(2, 900., 600., Duration::from_millis(1)));
	}
	#[test]
	fn progressive_worker_chases_target_and_cancels_changed_file() {
		let dir = crate::test_support::TestDir::new();
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
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version: 1,
			content_version: 1,
			path: path.clone(),
			loaded: None,
			embed_baseline: None,
			workspace: None,
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: 600.,
			load_all_images: false,
			requested_image: None,
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
		assert!(first.layout.blocks.len() < first.document.blocks().len());
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
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version: 2,
			content_version: 2,
			..request
		});
		resume.send(()).unwrap();
		let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
		assert_eq!(update.version, 2);
		let final_reader = update.result.unwrap().unwrap();
		assert!(final_reader.complete);
		assert_eq!(final_reader.document.source().unwrap(), "Replacement");
		assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
	}

	#[test]
	fn progressive_worker_finishes_with_shared_prefix() {
		let dir = crate::test_support::TestDir::new();
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
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version: 1,
			content_version: 1,
			path,
			loaded: None,
			embed_baseline: None,
			workspace: None,
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: 600.,
			load_all_images: false,
			requested_image: None,
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
	fn a_changed_hard_break_dialect_reparses_the_same_content() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("read.md");
		fs::write(&path, "one\ntwo\n\nthree\n").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let request = |version: u64, hard_breaks: bool| Request {
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version,
			content_version: 1,
			path: path.clone(),
			loaded: None,
			embed_baseline: None,
			workspace: None,
			options: LayoutOptions {
				hard_breaks,
				..crate::test_support::options()
			},
			requested: Instant::now(),
			coverage: f32::INFINITY,
			load_all_images: false,
			requested_image: None,
		};
		let complete = |version: u64| loop {
			let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
			if update.version == version
				&& update.result.as_ref().unwrap().as_ref().unwrap().complete
			{
				break update.result.unwrap().unwrap();
			}
		};
		let line_broken = |reader: &ReaderSnapshot| {
			reader.document.blocks().iter().any(|b| {
				matches!(
					b.kind(),
					markview_document::BlockKind::Paragraph(text)
						if text.iter().any(|i| matches!(
							i.kind,
							markview_document::InlineKind::LineBreak { .. }
						))
				)
			})
		};
		worker.submit(request(1, false));
		let soft = complete(1);
		assert!(!line_broken(&soft));
		// The same content in the other dialect is a new parse, not a reflow
		// of the document the worker kept.
		worker.submit(request(2, true));
		let hard = complete(2);
		assert!(!same_document(&soft.document, &hard.document));
		assert!(line_broken(&hard));
		// The same dialect again reuses the retained parse, like any reflow.
		worker.submit(request(3, true));
		let reused = complete(3);
		assert!(same_document(&hard.document, &reused.document));
	}

	#[test]
	fn a_changed_highlight_dialect_reparses_the_same_content() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("read.md");
		fs::write(&path, "a ==marked== b\n").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let request = |version: u64, highlight_marks: bool| Request {
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version,
			content_version: 1,
			path: path.clone(),
			loaded: None,
			embed_baseline: None,
			workspace: None,
			options: LayoutOptions {
				highlight_marks,
				..crate::test_support::options()
			},
			requested: Instant::now(),
			coverage: f32::INFINITY,
			load_all_images: false,
			requested_image: None,
		};
		let complete = |version: u64| loop {
			let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
			if update.version == version
				&& update.result.as_ref().unwrap().as_ref().unwrap().complete
			{
				break update.result.unwrap().unwrap();
			}
		};
		let marked = |reader: &ReaderSnapshot| {
			reader.document.blocks().iter().any(|b| {
				matches!(
					b.kind(),
					markview_document::BlockKind::Paragraph(text)
						if text.iter().any(|i| i.style.mark)
				)
			})
		};
		worker.submit(request(1, false));
		let literal = complete(1);
		assert!(!marked(&literal));
		// The same content in the other dialect is a new parse, not a reflow
		// of the document the worker kept.
		worker.submit(request(2, true));
		let highlighted = complete(2);
		assert!(!same_document(&literal.document, &highlighted.document));
		assert!(marked(&highlighted));
		// The same dialect again reuses the retained parse, like any reflow.
		worker.submit(request(3, true));
		let reused = complete(3);
		assert!(same_document(&highlighted.document, &reused.document));
	}

	#[test]
	fn grid_table_toggle_reparses_retained_documents() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("grid.md");
		fs::write(&path, "+---+\n| a |\n+---+\n").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let mut previous = None;
		for (version, grid_tables) in
			[(1, true), (2, false), (3, true), (4, true)]
		{
			worker.submit(Request {
				security: crate::security::Security::local(
					crate::security::Trust::Trusted,
				),
				version,
				content_version: 1,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options: LayoutOptions {
					grid_tables,
					..crate::test_support::options()
				},
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
				requested_image: None,
			});
			let reader = loop {
				let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
				if update.version == version
					&& update
						.result
						.as_ref()
						.unwrap()
						.as_ref()
						.unwrap()
						.complete
				{
					break update.result.unwrap().unwrap();
				}
			};
			assert_eq!(
				matches!(
					reader.document.blocks()[0].kind(),
					markview_document::BlockKind::Table(_)
				),
				grid_tables
			);
			if let Some(previous) = &previous {
				assert_eq!(
					same_document(previous, &reader.document),
					version == 4
				);
			}
			previous = Some(reader.document.clone());
		}
	}

	#[test]
	fn block_math_toggle_reparses_retained_documents() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("math.md");
		fs::write(&path, "$$\nx\n$$\n").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let mut previous = None;
		for (version, math_dollars_block) in
			[(1, true), (2, false), (3, true), (4, true)]
		{
			worker.submit(Request {
				security: crate::security::Security::local(
					crate::security::Trust::Trusted,
				),
				version,
				content_version: 1,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options: LayoutOptions {
					math_dollars_block,
					..crate::test_support::options()
				},
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
				requested_image: None,
			});
			let reader = loop {
				let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
				if update.version == version
					&& update
						.result
						.as_ref()
						.unwrap()
						.as_ref()
						.unwrap()
						.complete
				{
					break update.result.unwrap().unwrap();
				}
			};
			assert_eq!(
				matches!(
					reader.document.blocks()[0].kind(),
					markview_document::BlockKind::Paragraph(text)
						if matches!(text.as_slice(), [markview_document::Inline {
							kind: markview_document::InlineKind::Math { latex, display: true }, ..
						}] if latex == "x\n")
				),
				math_dollars_block
			);
			if let Some(previous) = &previous {
				assert_eq!(
					same_document(previous, &reader.document),
					version == 4
				);
			}
			previous = Some(reader.document.clone());
		}
	}

	#[test]
	fn worker_publishes_latest_request() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("read.md");
		fs::write(&path, "A paragraph.").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		for version in 1..=20 {
			worker.submit(Request {
				security: crate::security::Security::local(
					crate::security::Trust::Trusted,
				),
				version,
				content_version: 1,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options: LayoutOptions {
					width: 250.0 + version as f32,
					fonts: crate::test_support::fonts(),
					..Default::default()
				},
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
				requested_image: None,
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
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("read.md");
		fs::write(&path, "First").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			tx.send(u).unwrap();
		});
		let submit = |version, content_version| {
			worker.submit(Request {
				security: crate::security::Security::local(
					crate::security::Trust::Trusted,
				),
				version,
				content_version,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options: crate::test_support::options(),
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
				requested_image: None,
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
		assert!(same_document(
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
					update.result.unwrap().unwrap().document.source().unwrap(),
					"Second"
				);
				break;
			}
		}
		assert_eq!(first.document.source().unwrap(), "First");
	}

	#[test]
	fn release_drops_the_retained_document() {
		let dir = crate::test_support::TestDir::new();
		let path = dir.path().join("read.md");
		fs::write(&path, "First").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let request = |version| Request {
			security: crate::security::Security::local(
				crate::security::Trust::Trusted,
			),
			version,
			content_version: 1,
			path: path.clone(),
			loaded: None,
			embed_baseline: None,
			workspace: None,
			options: crate::test_support::options(),
			requested: Instant::now(),
			coverage: f32::INFINITY,
			load_all_images: false,
			requested_image: None,
		};
		worker.submit(request(1));
		let first = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert_eq!(first.document.source().unwrap(), "First");
		// The same content version reuses the retained parse.
		fs::write(&path, "Second").unwrap();
		worker.submit(request(2));
		let reused = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert_eq!(reused.document.source().unwrap(), "First");
		// Once nothing holds the document, the worker reads it again.
		worker.release();
		worker.submit(request(3));
		let reread = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert_eq!(reread.document.source().unwrap(), "Second");
	}

	#[test]
	fn complete_updates_carry_counts_to_each_document() {
		let dir = crate::test_support::TestDir::new();
		let first = dir.path().join("a.md");
		let second = dir.path().join("b.md");
		// Identical content in two files: the second open shares the cached
		// content identity, but its own session still needs the counts.
		let source = "Before.\n\n<details>\n<summary>Summary</summary>\n\nHidden words.\n\n</details>\n\nAfter.\n";
		fs::write(&first, source).unwrap();
		fs::write(&second, source).unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		let submit = |version, path: &PathBuf| {
			worker.submit(Request {
				security: crate::security::Security::local(
					crate::security::Trust::Trusted,
				),
				version,
				content_version: 1,
				path: path.clone(),
				loaded: None,
				embed_baseline: None,
				workspace: None,
				options: crate::test_support::options(),
				requested: Instant::now(),
				coverage: f32::INFINITY,
				load_all_images: false,
				requested_image: None,
			})
		};
		let complete = |version| loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == version
				&& update.result.as_ref().unwrap().as_ref().unwrap().complete
			{
				break update;
			}
		};
		submit(1, &first);
		let counts = complete(1).counts.expect("counts for the first document");
		assert_eq!(counts.words, 5);
		submit(2, &second);
		let update = complete(2);
		assert!(update.result.unwrap().is_ok());
		assert_eq!(update.counts, Some(counts));

		let key = markview_commonmark::parse(source)
			.document
			.content_key()
			.get();
		let cached = markview_layout::text::TextCounts {
			words: 123,
			..counts
		};
		worker.release();
		worker.reuse_counts(Some((key, cached)));
		submit(3, &first);
		assert_eq!(complete(3).counts, Some(cached));

		worker.release();
		fs::write(&second, "Changed input.").unwrap();
		submit(4, &second);
		assert_eq!(complete(4).counts.unwrap().words, 2);
	}
}
