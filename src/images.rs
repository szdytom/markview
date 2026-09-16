//! Bounded image scheduling and versioned snapshot publication.
mod cache;
mod decode;
mod source;
#[cfg(test)]
mod tests;
use anyhow::Result;
use cache::cache_pixels;
use decode::{Decoded, decode};
use markview_core::{
	document::Document,
	image::{ImageInfo, ImageSnapshot},
};
use source::{Source, fetch, source, stamp};
use std::{
	collections::{HashMap, HashSet},
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
		mpsc,
	},
	thread,
	time::{Duration, Instant, SystemTime},
};
const CPU_BUDGET: usize = 256 * 1024 * 1024;
/// Distinct remote sources one document may fetch per revision. Past this the
/// remainder wait as placeholders until the reader chooses to load them.
const MAX_REMOTE_SOURCES: usize = 128;
const REMOTE_LIMIT: &str = "Remote image limit reached (Load all)";
static VERSION: AtomicU64 = AtomicU64::new(1);

struct Job {
	ticket: u64,
	source: Source,
	generation: u64,
	target: Option<(u32, u32)>,
}
struct Finished {
	ticket: u64,
	source: Source,
	generation: u64,
	result: Result<Decoded>,
}
struct Entry {
	ticket: u64,
	aliases: Vec<String>,
	info: ImageInfo,
	stamp: Option<(u64, Option<SystemTime>)>,
	busy: bool,
	svg: bool,
	raster: Option<(u32, u32)>,
}

pub struct Images {
	pub snapshot: ImageSnapshot,
	entries: HashMap<Source, Entry>,
	send: Option<mpsc::Sender<Job>>,
	recv: mpsc::Receiver<Finished>,
	generation: u64,
	document: PathBuf,
	revision: u64,
	offline: bool,
	poll_at: Instant,
}

impl Images {
	pub fn new(offline: bool) -> Self {
		let (tx, rx) = mpsc::channel::<Job>();
		let rx = Arc::new(Mutex::new(rx));
		let (done, recv) = mpsc::channel();
		for i in 0..4 {
			let rx = rx.clone();
			let done = done.clone();
			thread::Builder::new()
				.name(format!("markview-image-{i}"))
				.spawn(move || {
					loop {
						let Ok(job) = rx.lock().unwrap().recv() else {
							break;
						};
						// A malformed file must not take the reader down with it.
						let result = std::panic::catch_unwind(
							std::panic::AssertUnwindSafe(|| {
								fetch(&job.source)
									.and_then(|b| decode(&b, job.target))
							}),
						)
						.unwrap_or_else(|_| {
							Err(anyhow::anyhow!("Image decoder failed"))
						});
						if done
							.send(Finished {
								ticket: job.ticket,
								source: job.source,
								generation: job.generation,
								result,
							})
							.is_err()
						{
							break;
						}
					}
				})
				.expect("start image loader");
		}
		Self {
			snapshot: Default::default(),
			entries: HashMap::new(),
			send: Some(tx),
			recv,
			generation: 0,
			document: PathBuf::new(),
			revision: 0,
			offline,
			poll_at: Instant::now(),
		}
	}

	/// `load_all` comes from the tab that asked for this layout, so lifting the
	/// remote cap never leaks into another document or another revision.
	pub fn prepare(
		&mut self,
		doc: &Document,
		path: &Path,
		revision: u64,
		load_all: bool,
	) {
		if self.document != path {
			self.entries.clear();
			self.snapshot = Default::default();
			self.generation += 1;
			self.document = path.into();
		}
		let reload = self.revision != revision;
		self.revision = revision;
		if load_all {
			// Clear a cap error recorded by an earlier pass of this revision,
			// so the deferred images are scheduled now.
			for e in self.entries.values_mut() {
				if e.info.error.as_deref() == Some(REMOTE_LIMIT) {
					e.info.error = None;
					e.info.size = None;
				}
			}
		}
		let mut specs = Vec::new();
		for b in &doc.blocks {
			b.images(&mut specs);
		}
		let mut wanted = HashSet::new();
		// Distinct remote sources past the cap stay placeholders; in document
		// order, so the same document always defers the same images.
		let mut remote_seen = 0usize;
		let retained_pixels: HashMap<_, _> = {
			let pixels = self.snapshot.pixels.decoded.lock().unwrap();
			self.entries
				.iter()
				.filter_map(|(source, e)| {
					e.aliases
						.iter()
						.find_map(|a| pixels.get(a).cloned())
						.map(|p| (source.clone(), p))
				})
				.collect()
		};
		for e in self.entries.values_mut() {
			e.aliases.clear();
		}
		self.snapshot.entries.clear();
		for spec in specs {
			match source(&spec.src, path, self.offline) {
				Ok(source) => {
					let first = wanted.insert(source.clone());
					let remote = matches!(source, Source::Http(_));
					if remote && first {
						remote_seen += 1;
					}
					let capped =
						remote && !load_all && remote_seen > MAX_REMOTE_SOURCES;
					let e = self.entries.entry(source.clone()).or_insert_with(
						|| Entry {
							ticket: 0,
							aliases: Vec::new(),
							info: Default::default(),
							stamp: stamp(&source),
							busy: false,
							svg: false,
							raster: None,
						},
					);
					if !e.aliases.contains(&spec.src) {
						e.aliases.push(spec.src.clone());
					}
					if reload && e.info.error.is_some() {
						e.info.error = None;
						e.info.size = None;
					}
					if capped {
						e.info.error = Some(REMOTE_LIMIT.into());
						e.info.size = None;
					}
					self.snapshot
						.entries
						.insert(spec.src.clone(), e.info.clone());
				}
				Err(e) => {
					self.snapshot.entries.insert(
						spec.src.clone(),
						ImageInfo {
							error: Some(e.to_string()),
							..Default::default()
						},
					);
				}
			}
		}
		self.entries.retain(|s, _| wanted.contains(s));
		// A new spelling of a retained source shares its pixels immediately.
		// Remove aliases no longer present so old snapshots cannot pin them.
		{
			let mut pixels = self.snapshot.pixels.decoded.lock().unwrap();
			for (source, e) in &self.entries {
				if let Some(p) = retained_pixels.get(source) {
					for alias in &e.aliases {
						pixels.insert(alias.clone(), p.clone());
					}
				}
			}
			pixels.retain(|alias, _| self.snapshot.entries.contains_key(alias));
		}
		self.schedule();
	}

	fn schedule(&mut self) {
		let demand = self.snapshot.pixels.demand.lock().unwrap().clone();
		let pixels = self.snapshot.pixels.decoded.lock().unwrap();
		let mut running = self.entries.values().filter(|e| e.busy).count();
		let mut keys: Vec<_> = self.entries.keys().cloned().collect();
		keys.sort_by_key(|s| {
			!self.entries[s]
				.aliases
				.iter()
				.any(|a| demand.contains_key(a))
		});
		for s in keys {
			if running >= 4 {
				break;
			}
			let e = self.entries.get_mut(&s).unwrap();
			let requested = e
				.aliases
				.iter()
				.filter_map(|a| demand.get(a).copied())
				.reduce(|mut a, b| {
					a.merge(b);
					a
				});
			let target = requested.map(|d| d.size);
			let resident = e.aliases.iter().any(|a| pixels.contains_key(a));
			let resize = e.svg && target.is_some() && target != e.raster;
			if !e.busy
				&& e.info.error.is_none()
				&& (e.info.size.is_none()
					|| resize || (requested.is_some_and(|d| d.needs_pixels)
					&& !resident))
			{
				e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
				e.busy = true;
				running += 1;
				let _ = self.send.as_ref().unwrap().send(Job {
					ticket: e.ticket,
					source: s,
					generation: self.generation,
					target: if e.svg { target } else { None },
				});
			}
		}
	}

	pub fn poll(&mut self) -> bool {
		let mut changed = false;
		while let Ok(done) = self.recv.try_recv() {
			if done.generation != self.generation {
				continue;
			}
			let Some(e) = self.entries.get_mut(&done.source) else {
				continue;
			};
			if e.ticket != done.ticket {
				continue;
			}
			e.busy = false;
			e.info.version = VERSION.fetch_add(1, Ordering::Relaxed);
			match done.result {
				Ok(decoded) => {
					e.info.size = Some(decoded.intrinsic);
					e.info.error = None;
					e.svg = decoded.svg;
					e.raster =
						Some((decoded.pixels.width, decoded.pixels.height));
					let mut pixels =
						self.snapshot.pixels.decoded.lock().unwrap();
					let demand = self.snapshot.pixels.demand.lock().unwrap();
					cache_pixels(
						&mut pixels,
						&e.aliases,
						decoded.pixels,
						&demand,
						CPU_BUDGET,
					);
				}
				Err(error) => {
					e.info.error = Some(error.to_string());
					self.snapshot
						.pixels
						.decoded
						.lock()
						.unwrap()
						.retain(|s, _| !e.aliases.contains(s));
				}
			}
			for alias in &e.aliases {
				self.snapshot.entries.insert(alias.clone(), e.info.clone());
			}
			changed = true;
		}
		if Instant::now() >= self.poll_at {
			self.poll_at = Instant::now() + Duration::from_millis(500);
			for (s, e) in &mut self.entries {
				let next = stamp(s);
				if next != e.stamp && !e.busy {
					e.stamp = next;
					e.info.error = None;
					e.info.size = None;
					changed = true;
				}
			}
		}
		self.schedule();
		changed
	}

	pub fn wait(&mut self) {
		// A headless frame can have posted new SVG sizes since the last load.
		self.poll();
		while self.entries.values().any(|e| {
			e.busy || (e.info.size.is_none() && e.info.error.is_none())
		}) {
			self.poll();
			thread::sleep(Duration::from_millis(5));
		}
	}

	/// Remote sources the per-revision cap left unrequested.
	pub fn deferred_remote(&self) -> usize {
		self.entries
			.values()
			.filter(|e| e.info.error.as_deref() == Some(REMOTE_LIMIT))
			.count()
	}
}
