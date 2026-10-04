//! Bounded image scheduling and versioned snapshot publication.
mod cache;
mod decode;
mod diagram;
mod fonts;
mod pixels;
mod source;
#[cfg(test)]
mod tests;
use anyhow::Result;
use decode::{Decoded, decode};
use fonts::DiagramFonts;
use markview_core::{
	document::Document,
	fonts::FontConfig,
	image::{ImageInfo, ImageSnapshot},
	style::Stylesheet,
};
use pixels::cache_pixels;
use source::{Source, fetch, source, stamp};
use std::{
	collections::{HashMap, HashSet},
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
		mpsc,
	},
	thread,
	time::{Duration, Instant, SystemTime},
};
use tokio_util::sync::CancellationToken;
const CPU_BUDGET: usize = 256 * 1024 * 1024;
/// Distinct remote sources one document may fetch per revision. Past this the
/// remainder wait as placeholders until the reader chooses to load them.
const MAX_REMOTE_SOURCES: usize = 128;
const REMOTE_LIMIT: &str = "Remote image limit reached (Load all)";
static VERSION: AtomicU64 = AtomicU64::new(1);

struct Job {
	cancel: CancellationToken,
	pdf: bool,
	ticket: u64,
	source: Source,
	generation: u64,
	target: Option<(u32, u32)>,
	/// The diagram theme this job renders with; other sources ignore it.
	theme: Arc<diagram::DiagramTheme>,
	/// The faces the rasterizer draws a diagram with, absent while the
	/// document holds no diagram to load.
	diagram: Option<DiagramRequest>,
}

/// Inputs for preparing the reader's diagram faces. The expensive collection
/// scan stays on the image worker rather than the layout thread.
#[derive(Clone)]
struct DiagramRequest {
	config: FontConfig,
	han: Vec<String>,
	families: Vec<String>,
	generic_families: Vec<(String, Vec<String>)>,
}
impl DiagramRequest {
	fn key(&self) -> u64 {
		crate::document::fingerprint(&(
			self.config.clone(),
			self.han.clone(),
			self.families.clone(),
			self.generic_families.clone(),
		))
	}
}
struct Finished {
	ticket: u64,
	source: Source,
	generation: u64,
	result: Result<Loaded>,
}
struct Loaded {
	intrinsic: (u32, u32),
	raster: (u32, u32),
	svg: bool,
	pixels: Option<Arc<markview_core::image::Pixels>>,
	pdf: Option<markview_pdf::PreparedImage>,
}

impl Loaded {
	fn new(decoded: Decoded, pdf: bool) -> Result<Self> {
		let prepared = if pdf {
			Some(markview_pdf::PreparedImage::new(&decoded.pixels)?)
		} else {
			None
		};
		Ok(Self {
			intrinsic: decoded.intrinsic,
			raster: (decoded.pixels.width, decoded.pixels.height),
			svg: decoded.svg,
			pixels: (!pdf).then_some(decoded.pixels),
			pdf: prepared,
		})
	}
}

struct Entry {
	cancel: CancellationToken,
	target: Option<(u32, u32)>,
	pdf: Option<markview_pdf::PreparedImage>,
	ticket: u64,
	aliases: Vec<String>,
	info: ImageInfo,
	stamp: Option<(u64, Option<SystemTime>)>,
	busy: bool,
	svg: bool,
	raster: Option<(u32, u32)>,
	/// Fingerprint of the diagram theme these pixels were rendered with, so a
	/// stylesheet change redraws a diagram instead of keeping its old colors.
	theme: u64,
}

impl Drop for Entry {
	fn drop(&mut self) {
		self.cancel.cancel();
	}
}

/// The faces a rasterizer resolves `source` with. Only a diagram is measured
/// and drawn with the reader's own faces; a standalone SVG keeps the system
/// resolver but uses the stylesheet's `[svg.generic_font_family]` mappings.
fn rasterizer_fonts<'a>(
	source: &Source,
	diagram: Option<&'a Arc<DiagramFonts>>,
	theme: &'a diagram::DiagramTheme,
) -> Option<(&'a Arc<DiagramFonts>, &'a str)> {
	match (source, diagram) {
		(Source::Diagram(_), Some(diagram)) => {
			Some((diagram, theme.font_family()))
		}
		_ => None,
	}
}

pub(crate) fn cache_directory() -> Option<PathBuf> {
	cache::directory()
}

type Intrinsic = (Source, Option<(u64, Option<SystemTime>)>, (u32, u32));

pub struct Images {
	pub snapshot: ImageSnapshot,
	/// Compress PDF resources on the workers instead of caching raw pixels.
	pdf: bool,
	entries: HashMap<Source, Entry>,
	/// Keep bounded intrinsic sizes across tabs, without retaining decoded pixels.
	intrinsic: Vec<Intrinsic>,
	resident: HashMap<String, Arc<markview_core::image::Pixels>>,
	demand: HashMap<String, markview_core::image::ImageDemand>,
	done: mpsc::Sender<Finished>,
	services: crate::services::Handle,
	_owner: Option<Arc<crate::services::Services>>,
	cache: Option<cache::Cache>,
	offline: bool,
	wake: markview_core::background::Wake,
	pipelines: Arc<tokio::sync::Semaphore>,
	recv: mpsc::Receiver<Finished>,
	generation: u64,
	document: PathBuf,
	revision: u64,
	poll_at: Instant,
	theme: Arc<diagram::DiagramTheme>,
	/// Identity of the theme together with the faces it draws with, so a new
	/// Han list redraws a diagram whose table did not change.
	theme_key: u64,
	/// Identity of the generic SVG mappings used by standalone SVGs.
	svg_theme_key: u64,
	fonts: FontConfig,
	/// The faces a diagram is measured and drawn with. Built only when the
	/// document holds a diagram, so a document without one never scans the
	/// reader's font collection.
	diagram: Option<DiagramRequest>,
}

impl Images {
	pub(crate) fn shared(
		offline: bool,
		fonts: FontConfig,
		services: &Arc<crate::services::Services>,
	) -> Self {
		Self::with_services(
			offline,
			cache::directory(),
			fonts,
			services.handle.clone(),
			Some(services.clone()),
			Arc::new(|| {}),
		)
	}
	pub(crate) fn shared_pdf(
		offline: bool,
		fonts: FontConfig,
		services: &Arc<crate::services::Services>,
	) -> Self {
		Self {
			pdf: true,
			..Self::shared(offline, fonts, services)
		}
	}

	/// A scheduler with an explicit cache directory, for tests. `None` keeps
	/// every fetch off the user's disk.
	#[cfg(test)]
	pub(super) fn with_cache(offline: bool, root: Option<PathBuf>) -> Self {
		Self::with_cache_and_fonts(offline, root, crate::test_support::fonts())
	}

	/// The same, with the faces a diagram is measured and drawn with.
	#[cfg(test)]
	pub(super) fn with_cache_and_fonts(
		offline: bool,
		root: Option<PathBuf>,
		fonts: FontConfig,
	) -> Self {
		Self::build(offline, root, fonts)
	}

	#[cfg(test)]
	fn build(
		offline: bool,
		cache_root: Option<PathBuf>,
		fonts: FontConfig,
	) -> Self {
		let owner = Arc::new(crate::services::Services::new(4));
		Self::with_services(
			offline,
			cache_root,
			fonts,
			owner.handle.clone(),
			Some(owner),
			Arc::new(|| {}),
		)
	}

	pub(crate) fn with_services(
		offline: bool,
		cache_root: Option<PathBuf>,
		fonts: FontConfig,
		services: crate::services::Handle,
		owner: Option<Arc<crate::services::Services>>,
		wake: markview_core::background::Wake,
	) -> Self {
		let (done, recv) = mpsc::channel();
		Self {
			snapshot: ImageSnapshot::default(),
			pdf: false,
			entries: HashMap::new(),
			intrinsic: Vec::new(),
			resident: HashMap::new(),
			demand: HashMap::new(),
			done,
			services,
			_owner: owner,
			cache: cache_root.map(cache::Cache::new),
			offline,
			wake,
			pipelines: Arc::new(tokio::sync::Semaphore::new(4)),
			recv,
			generation: 0,
			document: PathBuf::new(),
			revision: 0,
			poll_at: Instant::now(),
			theme_key: 0,
			svg_theme_key: 0,
			theme: Arc::new(diagram::resolve(&Stylesheet::default(), None)),
			diagram: None,
			fonts,
		}
	}

	/// `load_all` comes from the tab that asked for this layout, so lifting the
	/// remote cap never leaks into another document or another revision.
	///
	/// `sheet` is the stylesheet whose `[mermaid]` table draws the diagrams,
	/// and `fonts` the faces the request's reader has: a download adds a
	/// directory and bumps the revision, and the diagrams must follow the same
	/// faces the body text now uses. A resolved theme change redraws every
	/// diagram from the source it already parsed; resolving per request also
	/// follows the sheet's font definitions and the CJK variant it selected.
	pub fn prepare(
		&mut self,
		doc: &Document,
		path: &Path,
		revision: u64,
		load_all: bool,
		sheet: &Stylesheet,
		fonts: &FontConfig,
	) {
		if self.fonts != *fonts {
			self.fonts = fonts.clone();
		}
		let mut specs = Vec::new();
		for b in &doc.blocks {
			b.images(&mut specs);
		}
		// A document without a diagram never pays for the reader's font
		// collection, which a diagram's measurement and drawing need.
		let has_diagram = specs.iter().any(|spec| {
			spec.src.starts_with(markview_core::image::MERMAID_SCHEME)
		});
		let han: Vec<String> = sheet
			.cjk_families()
			.into_iter()
			.map(str::to_owned)
			.collect();
		let mermaid_families = diagram::candidate_families(sheet);
		let svg_generic_families = sheet.svg_generic_font_families();
		self.diagram = has_diagram.then(|| DiagramRequest {
			config: self.fonts.clone(),
			han,
			families: mermaid_families,
			generic_families: svg_generic_families,
		});
		let resolved = diagram::resolve(sheet, None);
		let faces = self.diagram.as_ref().map_or(0, DiagramRequest::key);
		let key =
			crate::document::fingerprint(&(faces, resolved.fingerprint()));
		self.svg_theme_key =
			crate::document::fingerprint(&sheet.svg_generic_font_families());
		if key != self.theme_key {
			self.theme = Arc::new(resolved);
			self.theme_key = key;
		}
		let theme = self.theme_key;
		if self.document != path {
			for (source, entry) in self.entries.drain() {
				if let Some(size) = entry.info.size
					&& matches!(source, Source::File(_) | Source::Http(_))
				{
					self.intrinsic.retain(|(key, _, _)| *key != source);
					self.intrinsic.push((source, entry.stamp, size));
				}
			}
			let excess = self.intrinsic.len().saturating_sub(256);
			self.intrinsic.drain(..excess);
			self.snapshot.pixels.replace(HashMap::new());
			self.snapshot = Default::default();
			self.resident.clear();
			self.demand.clear();
			self.generation += 1;
			self.snapshot.generation = self.generation;
			self.document = path.into();
		}
		self.snapshot
			.pixels
			.set_wake((!specs.is_empty()).then(|| self.wake.clone()));
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
		let mut wanted = HashSet::new();
		// Distinct remote sources past the cap stay placeholders; in document
		// order, so the same document always defers the same images.
		let mut remote_seen = 0usize;
		let retained_pixels: HashMap<_, _> = {
			let pixels = &self.resident;
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
			match source(&spec.src, path) {
				Ok(source) => {
					let first = wanted.insert(source.clone());
					let remote = matches!(source, Source::Http(_));
					if remote && first {
						remote_seen += 1;
					}
					let capped =
						remote && !load_all && remote_seen > MAX_REMOTE_SOURCES;
					let stamp = stamp(&source);
					let size = self
						.intrinsic
						.iter()
						.rev()
						.find(|(key, previous, _)| {
							*key == source && *previous == stamp
						})
						.map(|(_, _, size)| *size);
					let e = self.entries.entry(source.clone()).or_insert_with(
						|| Entry {
							cancel: self.services.cancel.child_token(),
							target: None,
							pdf: None,
							ticket: 0,
							aliases: Vec::new(),
							info: ImageInfo {
								size,
								..Default::default()
							},
							stamp,
							busy: false,
							svg: false,
							raster: None,
							theme,
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
						if e.busy {
							e.cancel.cancel();
							e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
							e.busy = false;
						}
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
			let pixels = &mut self.resident;
			for (source, e) in &self.entries {
				if let Some(p) = retained_pixels.get(source) {
					for alias in &e.aliases {
						pixels.insert(alias.clone(), p.clone());
					}
				}
			}
			pixels.retain(|alias, _| self.snapshot.entries.contains_key(alias));
		}
		self.publish_pixels();
		self.schedule();
	}

	fn publish_pixels(&self) {
		self.snapshot.pixels.replace(
			self.resident
				.iter()
				.filter_map(|(alias, pixels)| {
					self.snapshot.entries.get(alias).map(|info| {
						((alias.clone(), info.version), pixels.clone())
					})
				})
				.collect(),
		);
	}

	fn schedule(&mut self) {
		if let Some(demand) = self.snapshot.pixels.take_demand(self.generation)
		{
			self.demand = demand;
		}
		let demand = &self.demand;
		let pixels = &self.resident;
		let theme = self.theme_key;
		let svg_theme = self.svg_theme_key;
		let mut running = self.entries.values().filter(|e| e.busy).count();
		let mut keys: Vec<_> = self.entries.keys().cloned().collect();
		keys.sort_by_key(|s| {
			!self.entries[s]
				.aliases
				.iter()
				.any(|a| demand.contains_key(a))
		});
		for s in keys {
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
			// A diagram drawn under another theme is stale even though its
			// size and pixels are already here. Keeping them lets the old
			// drawing stand until the new one is ready, so nothing reflows
			// through a placeholder.
			let source_theme = if matches!(s, Source::Diagram(_)) {
				theme
			} else {
				svg_theme
			};
			let stale = source_theme != e.theme
				&& (matches!(s, Source::Diagram(_)) || e.svg);
			// A failure the old theme caused — a drawing past the pixel
			// limit, say — does not survive it, or the working theme that
			// follows could never bring the diagram back. A job still in
			// flight is cancelled before the next theme is scheduled.
			if e.busy && (stale || (e.svg && e.target != target)) {
				e.cancel.cancel();
				e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
				e.busy = false;
				running -= 1;
			}
			if stale && e.info.error.is_some() {
				e.info.error = None;
			}
			if running < 4
				&& !e.busy && e.info.error.is_none()
				&& (stale
					|| e.info.size.is_none()
					|| e.raster.is_none()
					|| resize || (requested.is_some_and(|d| d.needs_pixels)
					&& !resident))
			{
				e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
				e.busy = true;
				e.theme = source_theme;
				e.target = if e.svg { target } else { None };
				running += 1;
				e.cancel.cancel();
				e.cancel = self.services.cancel.child_token();
				let job = Job {
					cancel: e.cancel.clone(),
					pdf: self.pdf,
					ticket: e.ticket,
					source: s.clone(),
					generation: self.generation,
					target: e.target,
					theme: self.theme.clone(),
					diagram: matches!(s, Source::Diagram(_))
						.then(|| self.diagram.clone())
						.flatten(),
				};
				self.start(job);
			}
		}
	}

	fn start(&self, job: Job) {
		let services = self.services.clone();
		let done = self.done.clone();
		let wake = self.wake.clone();
		let cache = self.cache.clone();
		let offline = self.offline;
		let pipelines = self.pipelines.clone();
		let ticket = job.ticket;
		let generation = job.generation;
		let source = job.source.clone();
		let fallback_done = done.clone();
		let fallback_source = source.clone();
		let accepted = self.services.submit(async move {
			use futures_util::FutureExt;
			let operation = async {
				let _pipeline = tokio::select! {
					biased;
					_ = job.cancel.cancelled() => anyhow::bail!("Cancelled"),
					permit = pipelines.acquire_owned() => permit?,
				};
				let bytes = match &job.source {
					Source::Diagram(_) => None,
					Source::Http(url) => Some(
						cache::fetch_http(
							url,
							offline,
							cache.as_ref(),
							&services,
							&job.cancel,
						)
						.await?,
					),
					_ => {
						let source = job.source.clone();
						let theme = job.theme.clone();
						Some(
							tokio::task::spawn_blocking(move || {
								fetch(&source, offline, None, &theme)
							})
							.await??,
						)
					}
				};
				let input_bytes = bytes.as_ref().map_or_else(
					|| match &job.source {
						Source::Diagram(code) => code.len(),
						_ => 0,
					},
					Vec::len,
				);
				let cancel = job.cancel.clone();
				services
					.compute(input_bytes, &cancel, move || {
						if job.cancel.is_cancelled() {
							anyhow::bail!("Cancelled");
						}
						let diagram = job.diagram.as_ref().map(|request| {
							DiagramFonts::get_for(
								&request.config,
								&request.han,
								&request.families,
								&request.generic_families,
							)
						});
						let theme = diagram.as_ref().map_or_else(
							|| job.theme.clone(),
							|diagram| {
								let metrics: Arc<
									dyn mermaid_rs_renderer::TextMetrics,
								> = diagram.clone();
								Arc::new(job.theme.with_metrics(metrics))
							},
						);
						if job.cancel.is_cancelled() {
							anyhow::bail!("Cancelled");
						}
						let bytes = match bytes {
							Some(bytes) => bytes,
							None => fetch(&job.source, offline, None, &theme)?,
						};
						if job.cancel.is_cancelled() {
							anyhow::bail!("Cancelled");
						}
						let decoded = decode(
							&bytes,
							job.target,
							rasterizer_fonts(
								&job.source,
								diagram.as_ref(),
								&theme,
							),
							theme.generic_font_families(),
						)?;
						if job.cancel.is_cancelled() {
							anyhow::bail!("Cancelled");
						}
						Loaded::new(decoded, job.pdf)
					})
					.await
			};
			let result = std::panic::AssertUnwindSafe(operation)
				.catch_unwind()
				.await
				.unwrap_or_else(|_| {
					Err(anyhow::anyhow!("Image pipeline failed"))
				});
			let _ = done.send(Finished {
				ticket,
				source,
				generation,
				result,
			});
			wake();
		});
		if !accepted {
			let _ = fallback_done.send(Finished {
				ticket,
				source: fallback_source,
				generation,
				result: Err(anyhow::anyhow!("Image service closed")),
			});
			(self.wake)();
		}
	}

	pub(crate) fn poll_deadline(&self) -> Instant {
		self.poll_at
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
					e.raster = Some(decoded.raster);
					e.pdf = decoded.pdf;
					if let Some(incoming) = decoded.pixels {
						let pixels = &mut self.resident;
						let demand = &self.demand;
						cache_pixels(
							pixels, &e.aliases, incoming, demand, CPU_BUDGET,
						);
					}
				}
				Err(error) => {
					e.pdf = None;
					e.info.error = Some(error.to_string());
					self.resident.retain(|s, _| !e.aliases.contains(s));
				}
			}
			for alias in &e.aliases {
				self.snapshot.entries.insert(alias.clone(), e.info.clone());
			}
			changed = true;
		}
		if changed {
			self.publish_pixels();
		}
		if Instant::now() >= self.poll_at {
			self.poll_at = Instant::now() + Duration::from_millis(500);
			for (s, e) in &mut self.entries {
				let next = stamp(s);
				if next != e.stamp {
					if e.busy {
						e.cancel.cancel();
						e.ticket = VERSION.fetch_add(1, Ordering::Relaxed);
						e.busy = false;
					}
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

	/// Only encoded resources survive a PDF load, shared by all source aliases.
	pub fn pdf_images(
		&self,
	) -> HashMap<(String, u64), markview_pdf::PreparedImage> {
		self.entries
			.values()
			.filter_map(|entry| entry.pdf.as_ref().map(|image| (entry, image)))
			.flat_map(|(entry, image)| {
				entry.aliases.iter().map(move |alias| {
					((alias.clone(), entry.info.version), image.clone())
				})
			})
			.collect()
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

	/// Drops every decoded result of the closed document. `generation` moves so
	/// a load already in flight for it cannot reappear as a current entry.
	pub fn release(&mut self) {
		self.entries.clear();
		self.intrinsic.clear();
		self.snapshot.pixels.replace(HashMap::new());
		self.snapshot = Default::default();
		self.resident.clear();
		self.demand.clear();
		self.generation += 1;
		self.snapshot.generation = self.generation;
	}
}
