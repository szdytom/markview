//! Downloadable font families and the user font directory they land in.
//!
//! Nothing here runs on its own: a stylesheet only names families, and the
//! reader or the `fonts` command turns that list into an explicit download.
//! The directory is nothing more than another `--fonts` directory, so what a
//! family is called is decided by the font files themselves; the only state
//! kept here is what is on disk.
use anyhow::{Context, Result, bail};
use markview_core::{
	fonts::{FontConfig, is_font_file},
	style::{FontArchive, FontFamily, FontFile, FontSource},
};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::{
	collections::{BTreeMap, HashMap, HashSet},
	fs,
	io::{self, Read},
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
	time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// The largest single downloaded font accepted.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// The largest archive a source may download before it is unpacked.
pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The most one archive may unpack into matched members.
pub const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// The most members one archive may contribute.
pub const MAX_MEMBERS: usize = 4096;
/// Above this the reader warns about the download directory, but never
/// refuses: the number only exists to explain where the disk went.
pub const SOFT_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
/// Limit redraws while still reporting slow transfers regularly.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// The user font directory beside `settings.toml`.
pub fn directory() -> Option<PathBuf> {
	crate::settings::config_path()
		.and_then(|path| path.parent().map(|parent| parent.join("fonts")))
}

/// Makes the download directory part of a drawing run's font set, like
/// another `--fonts` directory.
///
/// A run that pins its own faces keeps exactly the command-line set, a
/// directory that is not there yet adds nothing, and a directory that is
/// already listed is not listed twice.
pub fn join_download_directory(
	fonts: &mut FontConfig,
	personal: Option<PathBuf>,
) {
	if fonts.ignore_system_fonts {
		return;
	}
	let Some(dir) = personal.filter(|dir| dir.is_dir()) else {
		return;
	};
	if !fonts.directories.contains(&dir) {
		fonts.directories.push(dir);
	}
}

/// What one font file on disk says about itself.
///
/// A downloaded family is only ever known through its own files: the reader
/// reads their names and attributes instead of trusting a manifest, so a file
/// copied in by hand is described exactly like one this application wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceInfo {
	/// The file name inside its directory, which is what a reader shows.
	pub file: String,
	/// Family names the file declares, the typographic family first. Every
	/// localized spelling is kept, because one family may be named differently
	/// per language.
	pub families: Vec<String>,
	/// The attributes of the first face. A collection's faces differ from one
	/// another, and only the family names are needed to recognize a family.
	pub weight: u16,
	pub style: FaceStyle,
	/// How many faces the file holds; a collection holds several.
	pub faces: usize,
	pub bytes: u64,
}

/// The slant a face declares for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceStyle {
	Normal,
	Italic,
	Oblique,
}

/// Reads every font file directly inside `directory` and reports what each one
/// says about itself, in file-name order. A file that cannot be read is
/// skipped, exactly as the shaper's own scan does.
pub fn describe(directory: &Path) -> Vec<FaceInfo> {
	let Ok(entries) = std::fs::read_dir(directory) else {
		return Vec::new();
	};
	let mut paths: Vec<PathBuf> =
		entries.flatten().map(|entry| entry.path()).collect();
	// Sorting keeps the report stable on every filesystem.
	paths.sort();
	paths.iter().filter_map(|path| describe_one(path)).collect()
}

pub(crate) fn describe_one(path: &Path) -> Option<FaceInfo> {
	if !is_font_file(path) {
		return None;
	}
	let file = path.file_name()?.to_str()?.to_owned();
	let bytes = std::fs::read(path).ok()?;
	let bytes_len = bytes.len() as u64;
	let data = swash::FontDataRef::new(&bytes)?;
	// The typographic family groups every weight of one family; the plain
	// family is what an older face declares instead. Every face contributes,
	// because a collection may hold several families.
	let mut typographic: Vec<String> = Vec::new();
	let mut plain: Vec<String> = Vec::new();
	let mut first: Option<(u16, FaceStyle)> = None;
	for font in data.fonts() {
		if first.is_none() {
			let attributes = font.attributes();
			let style = match attributes.style() {
				swash::Style::Italic => FaceStyle::Italic,
				swash::Style::Oblique(_) => FaceStyle::Oblique,
				swash::Style::Normal => FaceStyle::Normal,
			};
			first = Some((attributes.weight().0, style));
		}
		for string in font.localized_strings() {
			if !string.is_decodable() {
				continue;
			}
			let name = string.chars().collect::<String>();
			let name = name.trim();
			if name.is_empty() {
				continue;
			}
			let name = name.to_owned();
			match string.id() {
				swash::StringId::TypographicFamily => {
					push_name(&mut typographic, name)
				}
				swash::StringId::Family => push_name(&mut plain, name),
				_ => {}
			}
		}
	}
	typographic.extend(plain);
	let (weight, style) = first?;
	Some(FaceInfo {
		file,
		families: typographic,
		// A collection's faces differ; these describe its first face and are
		// shown for it alone.
		weight,
		style,
		faces: data.len(),
		bytes: bytes_len,
	})
}

fn push_name(names: &mut Vec<String>, name: String) {
	if !names.contains(&name) {
		names.push(name);
	}
}

/// What the reader knows about one catalogued family.
#[derive(Clone, Debug)]
pub struct Family {
	pub family: FontFamily,
	/// The catalogued sheets that declare this id, lowest layer first.
	pub owners: Vec<String>,
	pub state: State,
	/// The local files that serve the family, in file-name order.
	pub files: Vec<String>,
	/// What those files occupy.
	pub bytes: u64,
}

/// How a family is already satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
	/// Its own files are in the download directory.
	Downloaded,
	/// A name it lists is already installable, so there is nothing to fetch.
	Provided,
	/// Nothing provides it yet.
	Missing,
}

/// Builds the catalogue from the sheets that declare families, lowest layer
/// first, together with what the download directory holds.
///
/// A higher layer replaces a whole entry of the same id, exactly as font
/// definitions cascade. Ownership is kept per declaring sheet, because several
/// sheets may offer the same family.
pub fn catalog<'a>(
	sheets: impl IntoIterator<Item = (&'a str, &'a [FontFamily])>,
	dir: Option<&Path>,
	config: &FontConfig,
) -> Vec<Family> {
	let faces = dir.map(describe).unwrap_or_default();
	catalog_faces(sheets, &faces, config)
}

pub(crate) fn catalog_faces<'a>(
	sheets: impl IntoIterator<Item = (&'a str, &'a [FontFamily])>,
	faces: &[FaceInfo],
	config: &FontConfig,
) -> Vec<Family> {
	let mut out: Vec<Family> = Vec::new();
	for (owner, families) in sheets {
		for family in families {
			let served = |family: &FontFamily| -> Vec<String> {
				faces
					.iter()
					.filter(|face| {
						face.families
							.iter()
							.any(|name| family.lookfor.contains(name))
					})
					.map(|face| face.file.clone())
					.collect()
			};
			if let Some(existing) =
				out.iter_mut().find(|entry| entry.family.id == family.id)
			{
				// A higher layer replaces the whole entry, so the files it is
				// served by are read again from its new name list.
				existing.family = family.clone();
				existing.files = served(family);
				existing.bytes = faces
					.iter()
					.filter(|face| existing.files.contains(&face.file))
					.map(|face| face.bytes)
					.sum();
				existing.state = state_of(family, &existing.files, config);
				if !existing.owners.iter().any(|name| name == owner) {
					existing.owners.push(owner.to_owned());
				}
				continue;
			}
			let files = served(family);
			let bytes = faces
				.iter()
				.filter(|face| files.contains(&face.file))
				.map(|face| face.bytes)
				.sum();
			out.push(Family {
				state: state_of(family, &files, config),
				family: family.clone(),
				owners: vec![owner.to_owned()],
				files,
				bytes,
			});
		}
	}
	out
}

fn state_of(
	family: &FontFamily,
	files: &[String],
	config: &FontConfig,
) -> State {
	if !files.is_empty() {
		return State::Downloaded;
	}
	if markview_core::fonts::provided_family(config, &family.lookfor).is_some()
	{
		return State::Provided;
	}
	State::Missing
}

/// What the reader draws about one family's download.
#[derive(Clone, Debug)]
pub struct Progress {
	pub id: String,
	pub phase: Phase,
	/// Files finished so far, and how many the source holds.
	pub files_done: usize,
	pub files_total: usize,
	/// Bytes written so far across all transfers.
	pub bytes_done: u64,
	/// Sum of per-transfer completion fractions, including active downloads.
	pub files_progress: f64,
	pub current: Option<String>,
	/// A failure reason, or the name of the source being tried.
	pub note: Option<String>,
}
impl Progress {
	pub fn queued(id: &str) -> Self {
		Self {
			id: id.to_owned(),
			phase: Phase::Queued,
			files_done: 0,
			files_total: 0,
			bytes_done: 0,
			files_progress: 0.0,
			current: None,
			note: None,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
	Queued,
	Downloading,
	Extracting,
	Done,
	Failed,
	Cancelled,
}

/// What a run did, for a command's exit status and its summary line.
#[derive(Clone, Debug, Default)]
pub struct Summary {
	/// The families this run set out to download. A reader retires exactly
	/// these from its progress table, so two runs cannot clear each other.
	pub requested: Vec<String>,
	/// Families that ended up on disk.
	pub stored: usize,
	pub bytes: u64,
	pub failed: Vec<(String, String)>,
	pub cancelled: Vec<String>,
}

/// An injectable async transport. Futures stay on the shared I/O service.
pub trait Transport: Sync {
	fn fetch(
		&self,
		url: &str,
		path: &Path,
		cap: u64,
		progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
		cancel: &CancellationToken,
	) -> impl std::future::Future<Output = Result<()>> + Send;
	fn probe(
		&self,
		url: &str,
	) -> impl std::future::Future<Output = Result<Duration>> + Send;
}
impl Transport for crate::net::Downloader {
	async fn fetch(
		&self,
		url: &str,
		path: &Path,
		cap: u64,
		progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
		cancel: &CancellationToken,
	) -> Result<()> {
		crate::net::Downloader::fetch(self, url, path, cap, progress, cancel)
			.await
	}
	async fn probe(&self, url: &str) -> Result<Duration> {
		crate::net::Downloader::probe(self, url).await
	}
}

#[cfg(test)]
type CancelPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync>;

struct Download<'a, T> {
	transport: &'a T,
	services: &'a crate::services::Handle,
	cancel: &'a Cancellation,
}

#[derive(Clone)]
struct Cancellation {
	token: CancellationToken,
	#[cfg(test)]
	predicate: Option<(String, CancelPredicate)>,
}
impl Cancellation {
	fn is_cancelled(&self) -> bool {
		#[cfg(test)]
		if let Some((id, predicate)) = &self.predicate
			&& predicate(id)
		{
			self.token.cancel();
		}
		self.token.is_cancelled()
	}
}

pub async fn run_async<T: Transport>(
	families: &[FontFamily],
	dir: &Path,
	transport: &T,
	services: &crate::services::Handle,
	cancels: HashMap<String, CancellationToken>,
	report: impl FnMut(Progress) + Send,
) -> Summary {
	let cancels = cancels
		.into_iter()
		.map(|(id, token)| {
			(
				id,
				Cancellation {
					token,
					#[cfg(test)]
					predicate: None,
				},
			)
		})
		.collect();
	run_operation(families, dir, transport, services, cancels, report).await
}

#[cfg(test)]
fn run<T: Transport>(
	families: &[FontFamily],
	dir: &Path,
	transport: &T,
	jobs: usize,
	predicate: Arc<dyn Fn(&str) -> bool + Send + Sync>,
	report: &mut (dyn FnMut(Progress) + Send),
) -> Summary {
	let services = crate::services::Services::new(jobs);
	let cancels = families
		.iter()
		.map(|family| {
			(
				family.id.clone(),
				Cancellation {
					token: services.handle.cancel.child_token(),
					predicate: Some((family.id.clone(), predicate.clone())),
				},
			)
		})
		.collect();
	tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.unwrap()
		.block_on(run_operation(
			families,
			dir,
			transport,
			&services.handle,
			cancels,
			report,
		))
}

#[derive(Clone)]
struct Reporter {
	id: String,
	run: usize,
	attempt: usize,
	tx: tokio::sync::mpsc::Sender<Event>,
}
struct Event {
	id: String,
	run: usize,
	attempt: usize,
	change: Change,
}
enum Change {
	Attempt {
		total: usize,
		note: Option<String>,
	},
	File {
		file: usize,
		bytes: u64,
		total: Option<u64>,
		done: bool,
		phase: Phase,
		name: String,
	},
	Terminal {
		phase: Phase,
		note: Option<String>,
	},
}
impl Reporter {
	fn event(&self, change: Change) -> Event {
		Event {
			id: self.id.clone(),
			run: self.run,
			attempt: self.attempt,
			change,
		}
	}
	fn send(
		&self,
		change: Change,
	) -> impl std::future::Future<Output = ()> + Send + use<> {
		let tx = self.tx.clone();
		let event = self.event(change);
		async move {
			let _ = tx.send(event).await;
		}
	}
	async fn fetch<T: Transport>(
		&self,
		file: usize,
		name: &str,
		url: &str,
		path: &Path,
		cap: u64,
		download: &Download<'_, T>,
	) -> Result<u64> {
		let Download {
			transport,
			services,
			cancel,
		} = *download;
		if cancel.is_cancelled() {
			bail!("Cancelled");
		}
		let _permit = services.permit(&cancel.token).await?;
		let mut last_report: Option<Instant> = None;
		transport
			.fetch(
				url,
				path,
				cap,
				&mut |bytes, total| {
					if last_report
						.is_some_and(|at| at.elapsed() < PROGRESS_INTERVAL)
						&& total != Some(bytes)
					{
						return;
					}
					let _ = self.tx.try_send(self.event(Change::File {
						file,
						bytes,
						total,
						done: false,
						phase: Phase::Downloading,
						name: name.to_owned(),
					}));
					last_report = Some(Instant::now());
				},
				&cancel.token,
			)
			.await?;
		drop(_permit);
		let bytes = tokio::fs::metadata(path).await?.len();
		self.send(Change::File {
			file,
			bytes,
			total: Some(bytes),
			done: false,
			phase: Phase::Downloading,
			name: name.to_owned(),
		})
		.await;
		Ok(bytes)
	}
}
#[derive(Default)]
struct FileProgress {
	bytes: u64,
	fraction: f64,
	done: bool,
}
struct FamilyProgress {
	value: Progress,
	attempt: usize,
	files: HashMap<usize, FileProgress>,
}

async fn run_operation<T: Transport>(
	families: &[FontFamily],
	dir: &Path,
	transport: &T,
	services: &crate::services::Handle,
	cancels: HashMap<String, Cancellation>,
	mut report: impl FnMut(Progress) + Send,
) -> Summary {
	use futures_util::{FutureExt, StreamExt, stream::FuturesUnordered};
	if families.is_empty() {
		return Summary::default();
	}
	let mut summary = Summary {
		requested: families.iter().map(|f| f.id.clone()).collect(),
		..Default::default()
	};
	if let Err(error) = tokio::fs::create_dir_all(dir).await {
		summary.failed = families
			.iter()
			.map(|f| {
				(
					f.id.clone(),
					format!("Cannot create {}: {error}", dir.display()),
				)
			})
			.collect();
		for (id, reason) in &summary.failed {
			report(Progress {
				phase: Phase::Failed,
				note: Some(reason.clone()),
				..Progress::queued(id)
			});
		}
		return summary;
	}
	static RUN: AtomicUsize = AtomicUsize::new(1);
	let run = RUN.fetch_add(1, Ordering::Relaxed);
	let mut probes = HashMap::new();
	for family in families {
		for source in &family.source {
			if let Some(url) = first_url(source) {
				probes.entry(host_of(url)).or_insert_with(|| {
					let url = url.to_owned();
					async move {
						let Ok(_permit) =
							services.permit(&services.cancel).await
						else {
							return Duration::MAX;
						};
						tokio::select! {
							_ = services.cancel.cancelled() => Duration::MAX,
							latency = transport.probe(&url) => latency.unwrap_or(Duration::MAX),
						}
					}
					.boxed()
					.shared()
				});
			}
		}
	}
	let (tx, mut rx) = tokio::sync::mpsc::channel(64);
	let mut states: HashMap<_, _> = families
		.iter()
		.map(|f| {
			let value = Progress::queued(&f.id);
			report(value.clone());
			(
				f.id.clone(),
				FamilyProgress {
					value,
					attempt: 0,
					files: HashMap::new(),
				},
			)
		})
		.collect();
	let mut running = FuturesUnordered::new();
	for family in families {
		let reporter = Reporter {
			id: family.id.clone(),
			run,
			attempt: 0,
			tx: tx.clone(),
		};
		let cancel =
			cancels
				.get(&family.id)
				.cloned()
				.unwrap_or_else(|| Cancellation {
					token: services.cancel.child_token(),
					#[cfg(test)]
					predicate: None,
				});
		let probes = probes.clone();
		running.push(
			async move {
				let operation = async {
					if cancel.is_cancelled() {
						return Outcome::Cancelled;
					}
					let mut latencies = BTreeMap::new();
					let mut needed = FuturesUnordered::new();
					for source in &family.source {
						if let Some(url) = first_url(source) {
							let host = host_of(url);
							let probe = probes[&host].clone();
							needed.push(async move { (host, probe.await) });
						}
					}
					loop {
						tokio::select! {
							biased;
							_ = cancel.token.cancelled() => return Outcome::Cancelled,
							next = needed.next() => match next {
								Some((host, latency)) => { latencies.insert(host, latency); }
								None => break,
							}
						}
					}
					download_family(
						family, dir, transport, services, &latencies,
						&reporter, &cancel,
					)
					.await
				};
				let outcome = std::panic::AssertUnwindSafe(operation)
					.catch_unwind()
					.await
					.unwrap_or_else(|_| {
						Outcome::Failed("Download operation failed".into())
					});
				let (phase, note) = match &outcome {
					Outcome::Stored { .. } => (Phase::Done, None),
					Outcome::Failed(reason) => {
						(Phase::Failed, Some(reason.clone()))
					}
					Outcome::Cancelled => (Phase::Cancelled, None),
				};
				// Terminal events apply to the latest attempt, independently of file events.
				let mut terminal = reporter.clone();
				terminal.attempt = usize::MAX;
				terminal.send(Change::Terminal { phase, note }).await;
				(family.id.clone(), outcome)
			}
			.boxed(),
		);
	}
	drop(tx);
	let mut dirty = HashSet::new();
	let mut tick = tokio::time::interval(PROGRESS_INTERVAL);
	loop {
		tokio::select! {
			event = rx.recv() => {
				let Some(event) = event else { if running.is_empty() { break; } else { continue; } };
				if event.run != run { continue; }
				let Some(state) = states.get_mut(&event.id) else { continue; };
				match event.change {
					Change::Attempt { total, note } => {
						if event.attempt < state.attempt { continue; }
						state.attempt = event.attempt; state.files.clear();
						state.value = Progress { phase: Phase::Downloading, files_total: total, note, ..Progress::queued(&event.id) };
						dirty.insert(event.id);
					}
					Change::File { file, bytes, total, done, phase, name } => {
						if event.attempt != state.attempt { continue; }
						state.files.insert(file, FileProgress { bytes, fraction: if done { 1.0 } else { total.filter(|n| *n > 0).map_or(0.0, |n| (bytes as f64 / n as f64).min(1.0)) }, done });
						state.value.bytes_done = state.files.values().map(|f| f.bytes).sum();
						state.value.files_progress = state.files.values().map(|f| f.fraction).sum();
						state.value.files_done = state.files.values().filter(|f| f.done).count();
						state.value.phase = phase; state.value.current = Some(name);
						// Finished files are useful immediately, even beside a stalled transfer.
						if done { report(state.value.clone()); dirty.remove(&event.id); } else { dirty.insert(event.id); }
					}
					Change::Terminal { phase, note } => {
						state.value.phase = phase; state.value.note = note; state.value.current = None;
						report(state.value.clone()); dirty.remove(&event.id);
					}
				}
			}
			Some((id, outcome)) = running.next(), if !running.is_empty() => {
				match outcome {
					Outcome::Stored { bytes } => { summary.stored += 1; summary.bytes += bytes; }
					Outcome::Failed(reason) => summary.failed.push((id, reason)),
					Outcome::Cancelled => summary.cancelled.push(id),
				}
			}
			_ = tick.tick() => { for id in dirty.drain() { report(states[&id].value.clone()); } }
		}
	}
	summary.failed.sort();
	summary.cancelled.sort();
	summary
}

fn first_url(source: &FontSource) -> Option<&str> {
	source
		.files
		.first()
		.map(FontFile::url)
		.or_else(|| source.archives.first().map(|a| a.url.as_str()))
}
fn host_of(url: &str) -> String {
	url::Url::parse(url)
		.ok()
		.and_then(|url| url.host_str().map(str::to_ascii_lowercase))
		.unwrap_or_default()
}
async fn download_family<T: Transport>(
	family: &FontFamily,
	dir: &Path,
	transport: &T,
	services: &crate::services::Handle,
	latencies: &BTreeMap<String, Duration>,
	reporter: &Reporter,
	cancel: &Cancellation,
) -> Outcome {
	let mut sources: Vec<_> = family.source.iter().collect();
	sources.sort_by_key(|source| {
		first_url(source).map(|url| {
			latencies
				.get(&host_of(url))
				.copied()
				.unwrap_or(Duration::MAX)
		})
	});
	let mut last = "no source".to_owned();
	for (index, source) in sources.into_iter().enumerate() {
		if cancel.is_cancelled() {
			return Outcome::Cancelled;
		}
		let mut reporter = reporter.clone();
		reporter.attempt = index + 1;
		reporter
			.send(Change::Attempt {
				total: source.files.len() + source.archives.len(),
				note: source.label().map(str::to_owned),
			})
			.await;
		match attempt(
			family, source, dir, transport, services, &reporter, cancel,
		)
		.await
		{
			Ok(bytes) => return Outcome::Stored { bytes },
			Err(error) => last = format!("{error:#}"),
		}
	}
	if cancel.is_cancelled() {
		Outcome::Cancelled
	} else {
		Outcome::Failed(last)
	}
}

async fn attempt<T: Transport>(
	family: &FontFamily,
	source: &FontSource,
	dir: &Path,
	transport: &T,
	services: &crate::services::Handle,
	reporter: &Reporter,
	cancel: &Cancellation,
) -> Result<u64> {
	use futures_util::{FutureExt, StreamExt};
	let download = Download {
		transport,
		services,
		cancel,
	};
	// Only the budget's worth of file futures are constructed at a time.
	let mut pending = source.files.iter().enumerate();
	let mut files = futures_util::stream::FuturesUnordered::new();
	let mut staged = Vec::new();
	let mut first_error = None;
	loop {
		while files.len() < services.jobs {
			let Some((index, file)) = pending.next() else {
				break;
			};
			files.push(
				fetch_file(family, file, dir, &download, reporter, index)
					.boxed(),
			);
		}
		let Some(result) = files.next().await else {
			break;
		};
		match result {
			Ok(entry) => staged.push(entry),
			Err(error) => {
				first_error.get_or_insert(error);
			}
		}
	}
	if let Some(error) = first_error {
		return Err(error);
	}
	for (index, archive) in source.archives.iter().enumerate() {
		staged.extend(
			fetch_archive(
				family,
				archive,
				dir,
				&download,
				reporter,
				source.files.len() + index,
			)
			.await?,
		);
	}
	if cancel.is_cancelled() {
		bail!("Cancelled");
	}
	let bytes = staged.iter().map(|s| s.bytes).sum();
	let dir = dir.to_owned();
	let family = family.clone();
	let cancelled = cancel.clone();
	services
		.compute(0, &cancel.token, move || {
			if cancelled.is_cancelled() {
				bail!("Cancelled");
			}
			install(&dir, &mut staged)?;
			let installed: Vec<_> =
				staged.iter().map(|s| s.name.clone()).collect();
			if let Err(error) = retire_replaced(&dir, &family, &installed) {
				log::warn!("Fonts: cannot retire an old file: {error:#}");
			}
			Ok(bytes)
		})
		.await
}

struct DownloadTemp(PathBuf, bool);
impl Drop for DownloadTemp {
	fn drop(&mut self) {
		if self.1 {
			let _ = fs::remove_file(&self.0);
		}
	}
}

async fn fetch_file<T: Transport>(
	family: &FontFamily,
	file: &FontFile,
	dir: &Path,
	download: &Download<'_, T>,
	reporter: &Reporter,
	index: usize,
) -> Result<Staged> {
	let Download {
		services, cancel, ..
	} = *download;
	let basename = url_basename(file.url());
	let temp = DownloadTemp(temp_path(dir), true);
	let bytes = reporter
		.fetch(
			index,
			&basename,
			file.url(),
			&temp.0,
			MAX_FILE_BYTES,
			download,
		)
		.await?;
	let family = family.clone();
	let file = file.clone();
	let cancelled = cancel.clone();
	let name = basename.clone();
	let staged = services
		.compute(0, &cancel.token, move || {
			if cancelled.is_cancelled() {
				bail!("Cancelled");
			}
			let mut temp = temp;
			let staged = stage(&family, &temp.0, &basename, file.sha256())?;
			temp.1 = false;
			Ok(staged)
		})
		.await?;
	reporter
		.send(Change::File {
			file: index,
			bytes,
			total: Some(bytes),
			done: true,
			phase: Phase::Downloading,
			name,
		})
		.await;
	Ok(staged)
}
async fn fetch_archive<T: Transport>(
	family: &FontFamily,
	archive: &FontArchive,
	dir: &Path,
	download: &Download<'_, T>,
	reporter: &Reporter,
	index: usize,
) -> Result<Vec<Staged>> {
	let Download {
		services, cancel, ..
	} = *download;
	let basename = url_basename(&archive.url);
	let temp = DownloadTemp(temp_path(dir), true);
	let bytes = reporter
		.fetch(
			index,
			&basename,
			&archive.url,
			&temp.0,
			MAX_ARCHIVE_BYTES,
			download,
		)
		.await?;
	reporter
		.send(Change::File {
			file: index,
			bytes,
			total: Some(bytes),
			done: false,
			phase: Phase::Extracting,
			name: basename.clone(),
		})
		.await;
	let family = family.clone();
	let archive = archive.clone();
	let dir = dir.to_owned();
	let cancelled = cancel.clone();
	let name = basename.clone();
	let staged = services
		.compute(0, &cancel.token, move || {
			if cancelled.is_cancelled() {
				bail!("Cancelled");
			}
			if let Some(expected) = &archive.sha256
				&& !sha256_file(&temp.0)?.eq_ignore_ascii_case(expected)
			{
				bail!("sha256 mismatch for {basename}");
			}
			let container = sniff_file(&temp.0)?;
			let mut extracted =
				unpack(container, &temp.0, &archive.members, &dir)?;
			let mut staged = Vec::new();
			while !extracted.is_empty() {
				if cancelled.is_cancelled() {
					bail!("Cancelled");
				}
				let Some((member_temp, member_name)) = extracted.next_file()
				else {
					break;
				};
				let mut member = DownloadTemp(member_temp, true);
				staged.push(stage(&family, &member.0, &member_name, None)?);
				member.1 = false;
			}
			Ok(staged)
		})
		.await?;
	reporter
		.send(Change::File {
			file: index,
			bytes,
			total: Some(bytes),
			done: true,
			phase: Phase::Extracting,
			name,
		})
		.await;
	Ok(staged)
}

/// How much of a selection to take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
	/// Only families nothing provides yet. This is what the bulk action means:
	/// a family an installed face already covers is not "missing".
	Missing,
	/// Also families an installed face covers: a named family downloads a
	/// copy, and the bulk **Download All** takes every family not on disk.
	Named,
	/// Everything named, even what is already on disk.
	All,
}

/// The families a selection picked out of the catalogue, in catalogue order.
pub fn select<'a>(
	families: &'a [Family],
	ids: &[String],
	scope: Scope,
) -> Vec<&'a FontFamily> {
	families
		.iter()
		.filter(|entry| ids.iter().any(|id| id == &entry.family.id))
		.filter(|entry| match scope {
			Scope::Missing => entry.state == State::Missing,
			Scope::Named => entry.state != State::Downloaded,
			Scope::All => true,
		})
		.map(|entry| &entry.family)
		.collect()
}

enum Outcome {
	Stored { bytes: u64 },
	Failed(String),
	Cancelled,
}

/// One verified file of a source, waiting for the rest of it to succeed.
///
/// It is still a temporary file with a `.tmp` name, so the shaper's directory
/// scan cannot see it and an installed file is never replaced before the whole
/// source is known to be sound.
struct Staged {
	temp: PathBuf,
	/// The name this file takes once it is installed.
	name: String,
	bytes: u64,
	/// Set once the file has been moved into place, because a temp path that
	/// no longer exists must not be removed again on the way out.
	installed: bool,
}

impl Drop for Staged {
	fn drop(&mut self) {
		if !self.installed {
			let _ = fs::remove_file(&self.temp);
		}
	}
}

/// Moves staged files into place, keeping what they replace until all of them
/// have landed.
///
/// An existing file is moved aside rather than removed, so a failure part way
/// through puts every replaced copy back and removes everything already
/// installed. Two staged files may not claim one destination: that would
/// silently drop one of them.
fn install(dir: &Path, staged: &mut [Staged]) -> Result<()> {
	for (index, entry) in staged.iter().enumerate() {
		if staged[..index].iter().any(|other| other.name == entry.name) {
			bail!(
				"Two files of one source map to {:?}; give them distinct names",
				entry.name
			);
		}
	}
	let mut placed: Vec<PathBuf> = Vec::new();
	let mut replaced: Vec<(PathBuf, PathBuf)> = Vec::new();
	for entry in staged.iter_mut() {
		let target = dir.join(&entry.name);
		let saved = if target.exists() {
			let keep = temp_path(dir);
			if let Err(error) = fs::rename(&target, &keep) {
				remove_placed(&placed);
				restore(&replaced);
				return Err(error).with_context(|| {
					format!("Cannot replace {}", target.display())
				});
			}
			Some(keep)
		} else {
			None
		};
		if let Err(error) = fs::rename(&entry.temp, &target) {
			if let Some(keep) = &saved {
				let _ = fs::rename(keep, &target);
			}
			remove_placed(&placed);
			restore(&replaced);
			return Err(error)
				.with_context(|| format!("Cannot store {}", target.display()));
		}
		entry.installed = true;
		placed.push(target.clone());
		if let Some(keep) = saved {
			replaced.push((target, keep));
		}
	}
	// Every file landed, so the copies they replaced are no longer wanted.
	for (_, keep) in replaced {
		let _ = fs::remove_file(keep);
	}
	if let Ok(parent) = fs::File::open(dir) {
		let _ = parent.sync_all();
	}
	Ok(())
}

/// Removes the family's own earlier copies of the faces this run installed.
///
/// A mirror may name the same face differently — a release archive uses
/// `NotoSerif/hinted/ttf/NotoSerif-Regular.ttf` while a CDN serves
/// `NotoSerif-Regular.ttf` — so switching mirrors would otherwise leave both.
/// Only a file that this application named (a readable stem, a dash and a
/// sixteen-digit digest), that shares a stem with a file just installed, and
/// whose every declared name belongs to this family is removed. A face that
/// also declares another family, or a file a reader placed by hand, is left
/// alone.
fn retire_replaced(
	dir: &Path,
	family: &FontFamily,
	installed: &[String],
) -> Result<()> {
	let stems: Vec<&str> = installed
		.iter()
		.filter_map(|name| generated_stem(name))
		.collect();
	if stems.is_empty() {
		return Ok(());
	}
	for face in describe(dir) {
		if installed.contains(&face.file) {
			continue;
		}
		let Some(stem) = generated_stem(&face.file) else {
			continue;
		};
		if !stems.contains(&stem) {
			continue;
		}
		if face.families.is_empty()
			|| !face
				.families
				.iter()
				.all(|name| family.lookfor.contains(name))
		{
			continue;
		}
		fs::remove_file(dir.join(&face.file)).with_context(|| {
			format!("Cannot remove the replaced {}", face.file)
		})?;
	}
	Ok(())
}

/// The readable stem of a name this application generated, if it is one.
fn generated_stem(name: &str) -> Option<&str> {
	let (stem, extension) = name.rsplit_once('.')?;
	if !matches!(
		extension.to_ascii_lowercase().as_str(),
		"ttf" | "otf" | "ttc" | "otc"
	) {
		return None;
	}
	let (stem, digest) = stem.rsplit_once('-')?;
	if stem.is_empty()
		|| digest.len() != 16
		|| !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
	{
		return None;
	}
	Some(stem)
}

fn remove_placed(placed: &[PathBuf]) {
	for path in placed {
		let _ = fs::remove_file(path);
	}
}

fn restore(replaced: &[(PathBuf, PathBuf)]) {
	for (target, keep) in replaced {
		let _ = fs::rename(keep, target);
	}
}

/// Verifies a downloaded body and gives it the name it will be installed under.
///
/// The name is built from the family and the file's own source name, so the
/// same logical file lands on one name whichever mirror produced it. Nothing
/// in the download directory is touched here.
fn stage(
	family: &FontFamily,
	temp: &Path,
	source_name: &str,
	sha256: Option<&str>,
) -> Result<Staged> {
	let bytes = fs::read(temp)
		.with_context(|| format!("Cannot read {}", temp.display()))?;
	if bytes.len() as u64 > MAX_FILE_BYTES {
		bail!("Font file exceeds {} MiB", MAX_FILE_BYTES / (1024 * 1024));
	}
	if let Some(expected) = sha256 {
		let actual = sha256_hex(&bytes);
		if !actual.eq_ignore_ascii_case(expected) {
			bail!("sha256 mismatch for {source_name}");
		}
	}
	if !markview_core::fonts::is_font(&bytes) {
		bail!("{source_name} is not a font file");
	}
	Ok(Staged {
		temp: temp.to_owned(),
		name: local_name(&family.id, source_name, &bytes),
		bytes: bytes.len() as u64,
		installed: false,
	})
}

/// The local name one file is stored under.
///
/// `source_name` is the name the file has at its origin — a URL's last path
/// segment, or an archive member's whole path. Its last segment becomes the
/// readable prefix, while the whole name is hashed together with the family
/// id: two mirrors of one family agree on where their copy of a file lands,
/// and two members of one archive that share a basename but not a path stay
/// apart.
pub fn local_name(family: &str, source_name: &str, bytes: &[u8]) -> String {
	let basename = source_name.rsplit('/').next().unwrap_or(source_name);
	let decoded = percent_encoding::percent_decode_str(basename)
		.decode_utf8_lossy()
		.into_owned();
	let cleaned: String = decoded
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
				c
			} else {
				'_'
			}
		})
		.collect();
	let (mut stem, declared) = match cleaned.rsplit_once('.') {
		Some((stem, extension)) if !stem.is_empty() => {
			(stem.to_owned(), Some(extension.to_ascii_lowercase()))
		}
		_ => (cleaned, None),
	};
	let extension = match declared.as_deref() {
		Some(extension @ ("ttf" | "otf" | "ttc" | "otc")) => {
			extension.to_owned()
		}
		// A member with no usable extension takes the one its outlines imply.
		_ => outline_extension(bytes).to_owned(),
	};
	if stem.is_empty() || stem == "." || stem == ".." {
		stem = "font".into();
	}
	// The hash is fixed length, so bounding the stem keeps the whole name well
	// inside a file name's limit while the readable prefix stays.
	stem.truncate(96);
	format!("{stem}-{:016x}.{extension}", key_hash(family, source_name))
}

fn outline_extension(bytes: &[u8]) -> &'static str {
	if markview_core::fonts::is_postscript_outline(bytes) {
		"otf"
	} else {
		"ttf"
	}
}

/// A stable hash of one logical file's identity, used only to keep names
/// apart. FNV-1a never changes between builds, unlike `DefaultHasher`.
fn key_hash(family: &str, name: &str) -> u64 {
	const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
	const PRIME: u64 = 0x0000_0100_0000_01b3;
	family
		.bytes()
		.chain([0])
		.chain(name.bytes())
		.fold(OFFSET, |hash, byte| {
			(hash ^ u64::from(byte)).wrapping_mul(PRIME)
		})
}

/// The last path segment of a URL, percent-decoded.
fn url_basename(url: &str) -> String {
	url::Url::parse(url)
		.ok()
		.and_then(|url| {
			url.path_segments()
				.and_then(|mut path| path.next_back())
				.map(str::to_owned)
		})
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| "font".into())
}

/// A fresh temporary name inside the download directory.
///
/// The `.tmp` suffix is what keeps the shaper's directory scan from ever
/// registering a half-written file, and staying in the same directory is what
/// makes the final rename atomic.
fn temp_path(dir: &Path) -> PathBuf {
	static NEXT: AtomicUsize = AtomicUsize::new(0);
	let n = NEXT.fetch_add(1, Ordering::Relaxed);
	dir.join(format!(".tmp-{}-{n}.tmp", std::process::id()))
}

fn sha256_hex(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(64);
	for byte in Sha256::digest(bytes) {
		use std::fmt::Write as _;
		let _ = write!(out, "{byte:02x}");
	}
	out
}

fn sha256_file(path: &Path) -> Result<String> {
	let mut file = fs::File::open(path)?;
	let mut hasher = Sha256::new();
	let mut buffer = vec![0u8; 64 * 1024];
	loop {
		let read = file.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		hasher.update(&buffer[..read]);
	}
	let mut out = String::with_capacity(64);
	for byte in hasher.finalize() {
		use std::fmt::Write as _;
		let _ = write!(out, "{byte:02x}");
	}
	Ok(out)
}

/// The container an archive body is stored in, read from its leading bytes.
///
/// Nothing is declared in a stylesheet: the same family may be published as a
/// zip on one mirror and a tarball on another, and both are recognized here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
	Zip,
	Gzip,
	Zstd,
	Tar,
}

/// Recognizes a container from its first bytes.
pub fn sniff(head: &[u8]) -> Option<Container> {
	if head.starts_with(b"PK\x03\x04")
		|| head.starts_with(b"PK\x05\x06")
		|| head.starts_with(b"PK\x07\x08")
	{
		return Some(Container::Zip);
	}
	if head.starts_with(&[0x1f, 0x8b]) {
		return Some(Container::Gzip);
	}
	if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
		return Some(Container::Zstd);
	}
	// A plain tar names itself at offset 257.
	if head.len() >= 262 && &head[257..262] == b"ustar" {
		return Some(Container::Tar);
	}
	None
}

fn sniff_file(path: &Path) -> Result<Container> {
	let mut file = fs::File::open(path)?;
	let mut head = [0u8; 512];
	let read = file.read(&mut head)?;
	sniff(&head[..read])
		.with_context(|| format!("{} is not an archive", path.display()))
}

/// Unpacks the members matching `patterns` into temporary files.
///
/// `*` stays inside one path segment, `**` crosses segments, and `?` matches
/// one character. Directories, symlinks and hard links are never taken, so an
/// archive can only ever contribute regular files, and nothing is written
/// outside `dir`.
fn unpack(
	container: Container,
	path: &Path,
	patterns: &[String],
	dir: &Path,
) -> Result<Extracted> {
	let mut out = Extracted::default();
	match container {
		Container::Zip => unpack_zip(path, patterns, dir, &mut out)?,
		Container::Tar => {
			let file = fs::File::open(path)?;
			unpack_tar(file, patterns, dir, &mut out)?;
		}
		Container::Gzip => {
			let file = fs::File::open(path)?;
			unpack_tar(
				flate2::read::GzDecoder::new(file),
				patterns,
				dir,
				&mut out,
			)?;
		}
		Container::Zstd => {
			let file = fs::File::open(path)?;
			let decoder = zstd::stream::read::Decoder::new(file)
				.context("Cannot read the zstd stream")?;
			unpack_tar(decoder, patterns, dir, &mut out)?;
		}
	}
	if out.files.is_empty() {
		bail!("No archive member matched");
	}
	Ok(out)
}

/// The members one archive has unpacked so far.
///
/// It owns every temporary file it holds, so a failure anywhere after the
/// first member was written still cleans up after itself: whatever was not
/// taken into place is removed when this is dropped.
#[derive(Default)]
struct Extracted {
	files: Vec<(PathBuf, String)>,
	bytes: u64,
}

impl Drop for Extracted {
	fn drop(&mut self) {
		for (path, _) in &self.files {
			let _ = fs::remove_file(path);
		}
	}
}

impl Extracted {
	/// Takes one extracted member, leaving the rest to [`Drop`].
	fn next_file(&mut self) -> Option<(PathBuf, String)> {
		self.files.pop()
	}

	fn is_empty(&self) -> bool {
		self.files.is_empty()
	}

	fn take(
		&mut self,
		reader: &mut dyn Read,
		dir: &Path,
		name: String,
	) -> Result<()> {
		if self.files.len() >= MAX_MEMBERS {
			bail!("Archive has more than {MAX_MEMBERS} matching members");
		}
		let temp = temp_path(dir);
		// Registered before the first write, so a failed copy or sync is
		// cleaned up by [`Drop`] exactly like an abandoned extraction.
		self.files.push((temp.clone(), name));
		let mut file = fs::File::create(&temp)?;
		let mut limited = reader.take(MAX_FILE_BYTES + 1);
		let written = io::copy(&mut limited, &mut file)?;
		file.sync_all()?;
		if written > MAX_FILE_BYTES {
			bail!("Font file exceeds {} MiB", MAX_FILE_BYTES / (1024 * 1024));
		}
		self.bytes = self.bytes.saturating_add(written);
		if self.bytes > MAX_UNPACKED_BYTES {
			bail!(
				"Archive unpacks to more than {} GiB",
				MAX_UNPACKED_BYTES >> 30
			);
		}
		Ok(())
	}
}

fn unpack_zip(
	path: &Path,
	patterns: &[String],
	dir: &Path,
	out: &mut Extracted,
) -> Result<()> {
	let file = fs::File::open(path)?;
	let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
		.context("Not a zip archive")?;
	for index in 0..archive.len() {
		let mut entry = archive.by_index(index)?;
		if !entry.is_file() {
			continue;
		}
		// A symlink is an entry too; only a regular file may be taken.
		if let Some(mode) = entry.unix_mode()
			&& mode & 0o170000 != 0o100000
		{
			continue;
		}
		let name = entry.name().replace('\\', "/");
		if !patterns.iter().any(|pattern| glob_match(pattern, &name)) {
			continue;
		}
		out.take(&mut entry, dir, name)?;
	}
	Ok(())
}

fn unpack_tar(
	reader: impl Read,
	patterns: &[String],
	dir: &Path,
	out: &mut Extracted,
) -> Result<()> {
	let mut archive = tar::Archive::new(reader);
	for entry in archive.entries().context("Not a tar archive")? {
		let mut entry = entry?;
		if !entry.header().entry_type().is_file() {
			continue;
		}
		let name = entry.path()?.to_string_lossy().replace('\\', "/");
		if !patterns.iter().any(|pattern| glob_match(pattern, &name)) {
			continue;
		}
		out.take(&mut entry, dir, name)?;
	}
	Ok(())
}

/// Whether `pattern` matches the `/`-separated member `name`.
///
/// `*` matches within one segment, `**` crosses segments, `?` matches exactly
/// one character, and every other character is literal.
pub fn glob_match(pattern: &str, name: &str) -> bool {
	let pattern: Vec<char> = pattern.chars().collect();
	let name: Vec<char> = name.chars().collect();
	glob_at(&pattern, 0, &name, 0)
}

fn glob_at(p: &[char], pi: usize, s: &[char], si: usize) -> bool {
	if pi == p.len() {
		return si == s.len();
	}
	match p[pi] {
		'*' => {
			let double = p.get(pi + 1) == Some(&'*');
			let next = if double { pi + 2 } else { pi + 1 };
			// `**/` also matches no segment at all.
			if double
				&& p.get(next) == Some(&'/')
				&& glob_at(p, next + 1, s, si)
			{
				return true;
			}
			let mut at = si;
			loop {
				if glob_at(p, next, s, at) {
					return true;
				}
				if at >= s.len() || (!double && s[at] == '/') {
					return false;
				}
				at += 1;
			}
		}
		'?' => si < s.len() && s[si] != '/' && glob_at(p, pi + 1, s, si + 1),
		c => si < s.len() && s[si] == c && glob_at(p, pi + 1, s, si + 1),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write as _;

	/// Packs single-face fonts into one collection, so a test can describe a
	/// file that holds more than one family.
	fn collection_of(faces: &[Vec<u8>]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(b"ttcf");
		out.extend(1u32.to_be_bytes());
		out.extend((faces.len() as u32).to_be_bytes());
		out.extend(std::iter::repeat_n(0u8, 4 * faces.len()));
		for (index, face) in faces.iter().enumerate() {
			let start = out.len();
			let slot = 12 + index * 4;
			out[slot..slot + 4].copy_from_slice(&(start as u32).to_be_bytes());
			let count = u16::from_be_bytes([face[4], face[5]]) as usize;
			out.extend(&face[..12]);
			let mut tables = Vec::new();
			for table in 0..count {
				let record = 12 + table * 16;
				let at = u32::from_be_bytes(
					face[record + 8..record + 12].try_into().unwrap(),
				) as usize;
				let length = u32::from_be_bytes(
					face[record + 12..record + 16].try_into().unwrap(),
				) as usize;
				tables.push(face[at..at + length].to_vec());
				out.extend(&face[record..record + 8]);
				out.extend(0u32.to_be_bytes());
				out.extend((length as u32).to_be_bytes());
			}
			for (table, data) in tables.iter().enumerate() {
				while out.len() % 4 != 0 {
					out.push(0);
				}
				let at = out.len();
				let slot = start + 12 + table * 16 + 8;
				out[slot..slot + 4].copy_from_slice(&(at as u32).to_be_bytes());
				out.extend(data);
			}
		}
		out
	}

	/// A collection holds several families; describing only its first face
	/// would hide the rest from the download catalogue.
	#[test]
	fn a_collection_reports_every_family() {
		let dir = tempfile::tempdir().unwrap();
		let read = |name: &str| {
			std::fs::read(
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("crates/markview-core/tests/fonts")
					.join(name),
			)
			.unwrap()
		};
		let faces = vec![
			read("NotoSerif-Regular-subset.otf"),
			read("NotoSans-Regular-subset.otf"),
		];
		let bytes = collection_of(&faces);
		std::fs::write(dir.path().join("both.ttc"), &bytes).unwrap();
		let described = describe(dir.path());
		assert_eq!(described.len(), 1);
		assert_eq!(described[0].file, "both.ttc");
		assert_eq!(described[0].faces, 2);
		assert_eq!(described[0].bytes, bytes.len() as u64);
		let names = &described[0].families;
		assert!(
			names.iter().any(|name| name.contains("Noto Serif")),
			"{names:?}"
		);
		assert!(
			names.iter().any(|name| name.contains("Noto Sans")),
			"{names:?}"
		);
	}

	#[test]
	fn a_face_is_described_from_its_own_tables() {
		let dir = tempfile::tempdir().unwrap();
		let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf",
		))
		.unwrap();
		std::fs::write(dir.path().join("face.ttf"), &bytes).unwrap();
		// A file that is not a font is skipped rather than reported.
		std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
		let faces = describe(dir.path());
		assert_eq!(faces.len(), 1);
		assert_eq!(faces[0].file, "face.ttf");
		assert_eq!(faces[0].bytes, bytes.len() as u64);
		assert_eq!(faces[0].style, FaceStyle::Normal);
		assert!((1..=1000).contains(&faces[0].weight), "{}", faces[0].weight);
		assert!(
			faces[0]
				.families
				.iter()
				.any(|name| name.contains("Noto Serif")),
			"{:?}",
			faces[0].families
		);
		// An absent directory describes nothing instead of failing.
		assert!(describe(&dir.path().join("missing")).is_empty());
	}

	/// A real face, so the "is a font" check is exercised rather than stubbed.
	fn font_bytes() -> Vec<u8> {
		fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf",
		))
		.unwrap()
	}

	fn family(id: &str, files: &[&str]) -> FontFamily {
		FontFamily {
			id: id.into(),
			lookfor: vec![format!("{id} family")],
			description: None,
			license: None,
			license_url: None,
			homepage: None,
			source: vec![FontSource {
				name: None,
				files: files
					.iter()
					.map(|url| FontFile::Url((*url).into()))
					.collect(),
				archives: Vec::new(),
			}],
		}
	}

	#[tokio::test]
	async fn simultaneous_runs_share_the_transfer_budget() {
		use std::sync::atomic::{AtomicUsize, Ordering};
		struct Controlled {
			active: AtomicUsize,
			peak: AtomicUsize,
			entered: tokio::sync::mpsc::UnboundedSender<()>,
			release: tokio::sync::Semaphore,
			bytes: Vec<u8>,
		}
		impl Transport for Controlled {
			async fn fetch(
				&self,
				_: &str,
				path: &Path,
				_: u64,
				progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
				cancel: &CancellationToken,
			) -> Result<()> {
				let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
				self.peak.fetch_max(active, Ordering::SeqCst);
				self.entered.send(()).unwrap();
				let permit = tokio::select! { permit = self.release.acquire() => permit?, _ = cancel.cancelled() => bail!("Cancelled") };
				permit.forget();
				tokio::fs::write(path, &self.bytes).await?;
				progress(
					self.bytes.len() as u64,
					Some(self.bytes.len() as u64),
				);
				self.active.fetch_sub(1, Ordering::SeqCst);
				Ok(())
			}
			async fn probe(&self, _: &str) -> Result<Duration> {
				Ok(Duration::ZERO)
			}
		}
		let services = crate::services::Services::new(2);
		let (entered, mut events) = tokio::sync::mpsc::unbounded_channel();
		let transport = Controlled {
			active: AtomicUsize::new(0),
			peak: AtomicUsize::new(0),
			entered,
			release: tokio::sync::Semaphore::new(0),
			bytes: font_bytes(),
		};
		let a = tempfile::tempdir().unwrap();
		let b = tempfile::tempdir().unwrap();
		let first = [family(
			"first",
			&["https://example.com/a.otf", "https://example.com/b.otf"],
		)];
		let second = [family(
			"second",
			&["https://example.com/c.otf", "https://example.com/d.otf"],
		)];
		let release = async {
			events.recv().await.unwrap();
			events.recv().await.unwrap();
			assert_eq!(transport.active.load(Ordering::SeqCst), 2);
			transport.release.add_permits(4);
		};
		let (first, second, ()) = tokio::join!(
			run_async(
				&first,
				a.path(),
				&transport,
				&services.handle,
				HashMap::new(),
				|_| {}
			),
			run_async(
				&second,
				b.path(),
				&transport,
				&services.handle,
				HashMap::new(),
				|_| {}
			),
			release
		);
		assert!(first.failed.is_empty(), "{:?}", first.failed);
		assert!(second.failed.is_empty(), "{:?}", second.failed);
		assert_eq!(first.stored + second.stored, 2);
		let count = |dir: &Path| {
			fs::read_dir(dir)
				.unwrap()
				.filter_map(Result::ok)
				.filter(|entry| {
					entry.path().extension().is_some_and(|ext| ext == "otf")
				})
				.count()
		};
		assert_eq!(count(a.path()) + count(b.path()), 4);
		assert_eq!(transport.peak.load(Ordering::SeqCst), 2);
		assert_eq!(transport.active.load(Ordering::SeqCst), 0);
	}

	#[test]
	fn a_download_directory_joins_the_font_set_once() {
		let dir = tempfile::tempdir().unwrap();
		// A drawing run gains the directory, like another `--fonts` one.
		let mut fonts = FontConfig::default();
		join_download_directory(&mut fonts, Some(dir.path().to_path_buf()));
		assert_eq!(fonts.directories, vec![dir.path().to_path_buf()]);
		// A second listing of the same directory is dropped.
		join_download_directory(&mut fonts, Some(dir.path().to_path_buf()));
		assert_eq!(fonts.directories, vec![dir.path().to_path_buf()]);
		// A directory already named on the command line is not repeated.
		let mut named = FontConfig {
			directories: vec![dir.path().to_path_buf()],
			..Default::default()
		};
		join_download_directory(&mut named, Some(dir.path().to_path_buf()));
		assert_eq!(named.directories, vec![dir.path().to_path_buf()]);
		// A pinned run keeps exactly its command-line set.
		let mut pinned = FontConfig {
			ignore_system_fonts: true,
			..Default::default()
		};
		join_download_directory(&mut pinned, Some(dir.path().to_path_buf()));
		assert!(pinned.directories.is_empty());
	}

	#[test]
	fn a_file_name_carries_its_family_and_basename() {
		let font = font_bytes();
		let a = local_name("noto", "NotoSerif-Regular.otf", &font);
		assert!(a.starts_with("NotoSerif-Regular-"), "{a}");
		assert!(a.ends_with(".otf"), "{a}");
		// The same logical file lands on one name whichever mirror named it.
		assert_eq!(a, local_name("noto", "NotoSerif-Regular.otf", &font));
		// Two families never share a name, even for the same basename.
		assert_ne!(a, local_name("other", "NotoSerif-Regular.otf", &font));
		// A query string is not part of the basename, and a percent escape is
		// decoded before the name is built.
		let b = local_name(
			"noto",
			&url_basename("https://example.com/a%20b.otf?token=1"),
			&font,
		);
		assert!(b.starts_with("a_b-"), "{b}");
		assert!(b.ends_with(".otf"), "{b}");
		// An unknown extension gives way to the one the outlines imply, which
		// for this face is TrueType whatever its file name says.
		assert!(local_name("noto", "Serif", &font).ends_with(".ttf"));
		assert!(local_name("noto", "Serif.otf", &font).ends_with(".otf"));
		assert!(
			url_basename("https://example.com/a/b.otf").starts_with("b.otf")
		);
		assert_eq!(url_basename("https://example.com/"), "font");
	}

	#[test]
	fn a_member_pattern_stops_at_a_separator_unless_it_is_doubled() {
		assert!(glob_match("*.otf", "a.otf"));
		assert!(!glob_match("*.otf", "sub/a.otf"));
		assert!(glob_match("**/*.otf", "sub/a.otf"));
		assert!(glob_match("**/*.otf", "a.otf"));
		assert!(glob_match("Sans/**/SC/*.otf", "Sans/x/y/SC/a.otf"));
		assert!(glob_match("a?c", "abc"));
		assert!(!glob_match("a?c", "a/c"));
		assert!(!glob_match("*.otf", "a.ttf"));
	}

	#[test]
	fn a_container_is_recognized_from_its_leading_bytes() {
		assert_eq!(sniff(b"PK\x03\x04rest"), Some(Container::Zip));
		assert_eq!(sniff(&[0x1f, 0x8b, 0x08]), Some(Container::Gzip));
		assert_eq!(
			sniff(&[0x28, 0xb5, 0x2f, 0xfd, 0x00]),
			Some(Container::Zstd)
		);
		let mut tar = vec![0u8; 512];
		tar[257..262].copy_from_slice(b"ustar");
		assert_eq!(sniff(&tar), Some(Container::Tar));
		assert_eq!(sniff(b"<!doctype html>"), None);
	}

	/// Builds a zip holding the given members, so extraction is tested against
	/// a writer rather than a checked-in binary.
	fn zip_of(members: &[(&str, &[u8])]) -> PathBuf {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.keep().join("a.zip");
		let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
		let options: zip::write::FileOptions<'_, ()> =
			zip::write::FileOptions::default();
		for (name, bytes) in members {
			writer.start_file(*name, options).unwrap();
			writer.write_all(bytes).unwrap();
		}
		writer.finish().unwrap();
		path
	}

	#[test]
	fn an_archive_contributes_only_its_matching_regular_files() {
		let font = font_bytes();
		let archive = zip_of(&[
			("Sans/SubsetOTF/SC/NotoSansSC-Regular.otf", &font),
			("Sans/SubsetOTF/SC/NotoSansSC-Bold.otf", &font),
			("Sans/OTF/Japanese/NotoSansJP-Regular.otf", &font),
			("Sans/README.md", b"not a font"),
		]);
		let dir = tempfile::tempdir().unwrap();
		let patterns = vec!["**/SubsetOTF/SC/*.otf".to_string()];
		let mut members =
			unpack(Container::Zip, &archive, &patterns, dir.path()).unwrap();
		let mut names = Vec::new();
		while let Some((path, name)) = members.next_file() {
			assert!(path.exists());
			names.push(name);
		}
		names.sort();
		assert_eq!(
			names,
			[
				"Sans/SubsetOTF/SC/NotoSansSC-Bold.otf",
				"Sans/SubsetOTF/SC/NotoSansSC-Regular.otf"
			]
		);
		// Nothing matched is an error rather than an empty success.
		assert!(
			unpack(Container::Zip, &archive, &["**/*.ttf".into()], dir.path())
				.is_err()
		);
	}

	/// Distinct members of one archive may share a basename; they must not
	/// share a destination.
	#[test]
	fn two_members_that_share_a_basename_land_apart() {
		let font = font_bytes();
		let regular = local_name("family", "regular/font.ttf", &font);
		let bold = local_name("family", "bold/font.ttf", &font);
		assert_ne!(regular, bold);
		assert!(regular.starts_with("font-"), "{regular}");
		assert!(bold.starts_with("font-"), "{bold}");
		// One member has one name, whenever it is asked for.
		assert_eq!(regular, local_name("family", "regular/font.ttf", &font));
	}

	/// A member that was unpacked but never taken into place must not outlive
	/// the archive that produced it.
	#[test]
	fn an_abandoned_extraction_removes_its_temporary_files() {
		let font = font_bytes();
		let archive =
			zip_of(&[("a/Regular.otf", &font), ("a/Bold.otf", &font)]);
		let dir = tempfile::tempdir().unwrap();
		let extracted =
			unpack(Container::Zip, &archive, &["**/*.otf".into()], dir.path())
				.unwrap();
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
		drop(extracted);
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}

	#[test]
	fn a_tarball_unpacks_the_same_way() {
		let font = font_bytes();
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("a.tar.gz");
		{
			let file = fs::File::create(&path).unwrap();
			let encoder = flate2::write::GzEncoder::new(
				file,
				flate2::Compression::fast(),
			);
			let mut builder = tar::Builder::new(encoder);
			let mut header = tar::Header::new_gnu();
			header.set_size(font.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			builder
				.append_data(
					&mut header,
					"Serif/SubsetOTF/SC/NotoSerifSC-Regular.otf",
					&font[..],
				)
				.unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let container = sniff_file(&path).unwrap();
		assert_eq!(container, Container::Gzip);
		let mut members =
			unpack(container, &path, &["**/SC/*.otf".into()], dir.path())
				.unwrap();
		let (temp, name) = members.next_file().unwrap();
		assert!(temp.exists());
		assert_eq!(name, "Serif/SubsetOTF/SC/NotoSerifSC-Regular.otf");
		assert!(members.next_file().is_none());
	}

	/// Arch ships a font as a zstd tarball, so that container must unpack like
	/// the others.
	#[test]
	fn a_zstd_package_unpacks_the_same_way() {
		let font = font_bytes();
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("ttf-fira-code-6.2-4-any.pkg.tar.zst");
		{
			let file = fs::File::create(&path).unwrap();
			let encoder = zstd::stream::write::Encoder::new(file, 0).unwrap();
			let mut builder = tar::Builder::new(encoder);
			let mut header = tar::Header::new_gnu();
			header.set_size(font.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			builder
				.append_data(
					&mut header,
					"usr/share/fonts/TTF/FiraCode-Regular.ttf",
					&font[..],
				)
				.unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let container = sniff_file(&path).unwrap();
		assert_eq!(container, Container::Zstd);
		let mut members = unpack(
			container,
			&path,
			&["usr/share/fonts/TTF/FiraCode-*.ttf".into()],
			dir.path(),
		)
		.unwrap();
		let (temp, name) = members.next_file().unwrap();
		assert!(temp.exists());
		assert_eq!(name, "usr/share/fonts/TTF/FiraCode-Regular.ttf");
		assert!(members.next_file().is_none());
	}

	/// A transport that serves canned bodies and mirrors, with no network.
	#[derive(Default)]
	struct Fake {
		bodies: BTreeMap<String, Vec<u8>>,
		fail: Vec<String>,
		probe: BTreeMap<String, Duration>,
		seen: Mutex<Vec<String>>,
		/// Flipped by a successful transfer, so a test can cancel a job after
		/// its download finished but before it extracts or installs.
		arm: Option<Arc<std::sync::atomic::AtomicBool>>,
	}
	impl Transport for Fake {
		async fn fetch(
			&self,
			url: &str,
			path: &Path,
			_cap: u64,
			progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
			_cancel: &CancellationToken,
		) -> Result<()> {
			self.seen.lock().unwrap().push(url.to_owned());
			if self.fail.iter().any(|pattern| url.contains(pattern)) {
				bail!("HTTP 404");
			}
			let body = self
				.bodies
				.get(url)
				.with_context(|| format!("no canned body for {url}"))?;
			fs::write(path, body)?;
			if let Some(arm) = &self.arm {
				arm.store(true, std::sync::atomic::Ordering::Relaxed);
			}
			progress(body.len() as u64, Some(body.len() as u64));
			Ok(())
		}
		async fn probe(&self, url: &str) -> Result<Duration> {
			Ok(self
				.probe
				.get(&host_of(url))
				.copied()
				.unwrap_or(Duration::from_millis(50)))
		}
	}

	fn fan(id: &str, urls: &[&str]) -> FontFamily {
		family(id, urls)
	}

	/// A color emoji face carries no outline table, only `CBDT` strikes, and
	/// the reader draws it, so a download must store it like any other face.
	#[test]
	fn a_bitmap_color_emoji_is_a_downloadable_font() {
		let dir = tempfile::tempdir().unwrap();
		let emoji = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-core/tests/fonts/NotoColorEmoji-subset.ttf",
		))
		.unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://good.example/emoji.ttf".into(), emoji);
		let summary = run(
			&[fan("noto-emoji", &["https://good.example/emoji.ttf"])],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1, "{:?}", summary.failed);
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert!(names.iter().all(|name| name.ends_with(".ttf")), "{names:?}");
	}

	#[test]
	fn completed_files_report_before_a_slower_first_file_finishes() {
		struct Delayed {
			fake: Fake,
			release: tokio::sync::Notify,
		}
		impl Transport for Delayed {
			async fn fetch(
				&self,
				url: &str,
				path: &Path,
				cap: u64,
				progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
				cancel: &CancellationToken,
			) -> Result<()> {
				if url.ends_with("slow.otf") {
					let body = &self.fake.bodies[url];
					let half = body.len() / 2;
					fs::write(path, &body[..half])?;
					progress(half as u64, Some(body.len() as u64));
					tokio::time::timeout(
						Duration::from_secs(5),
						self.release.notified(),
					)
					.await?;
				}
				self.fake.fetch(url, path, cap, progress, cancel).await
			}
			async fn probe(&self, url: &str) -> Result<Duration> {
				self.fake.probe(url).await
			}
		}
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let urls = [
			"https://good.example/slow.otf",
			"https://good.example/fast.otf",
		];

		let transport = Delayed {
			fake: Fake {
				bodies: urls
					.iter()
					.map(|url| (url.to_string(), font.clone()))
					.collect(),
				..Default::default()
			},
			release: tokio::sync::Notify::new(),
		};
		let mut events = Vec::new();
		let summary = run(
			&[fan("noto", &urls)],
			dir.path(),
			&transport,
			2,
			Arc::new(|_| false),
			&mut |progress| {
				if progress.files_done == 1 && progress.files_progress > 1.0 {
					transport.release.notify_one();
				}
				events.push(progress);
			},
		);
		assert!(summary.failed.is_empty(), "{:?}", summary.failed);
		assert_eq!(summary.stored, 1);
		assert!(events.iter().any(|p| p.files_done == 1
			&& p.files_progress > 1.0
			&& p.files_progress < 2.0
			&& p.phase == Phase::Downloading));
		assert!(
			events
				.windows(2)
				.all(|pair| pair[0].bytes_done <= pair[1].bytes_done
					&& pair[0].files_progress <= pair[1].files_progress)
		);
		let last = events.last().unwrap();
		assert_eq!(last.files_done, 2);
		assert_eq!(last.files_progress, 2.0);
		assert_eq!(last.bytes_done, 2 * font.len() as u64);
	}

	#[test]
	fn a_failing_mirror_is_replaced_by_the_next_one() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut fake = Fake::default();
		// The first source fails, the second serves every file.
		fake.bodies
			.insert("https://good.example/a.otf".into(), font.clone());
		fake.bodies
			.insert("https://good.example/b.otf".into(), font.clone());
		let mut family = fan("noto", &["https://bad.example/a.otf"]);
		family.source.push(FontSource {
			name: Some("good".into()),
			files: vec![
				FontFile::Url("https://good.example/a.otf".into()),
				FontFile::Url("https://good.example/b.otf".into()),
			],
			archives: Vec::new(),
		});
		let mut events = Vec::new();
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			2,
			Arc::new(|_: &str| false),
			&mut |progress| events.push(progress),
		);
		assert_eq!(summary.stored, 1);
		assert!(summary.failed.is_empty());
		// Both files of the surviving source are on disk, and nothing is left
		// behind with a temporary name.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names.len(), 2, "{names:?}");
		assert!(names.iter().all(|name| name.ends_with(".otf")), "{names:?}");
		// Both mirrors were asked, the failing one first.
		let seen = fake.seen.lock().unwrap().clone();
		assert_eq!(seen.len(), 3, "{seen:?}");
	}

	/// Every worker finishes before the results are read, so a failure in one
	/// file must not let another file's success escape the source rollback.
	#[test]
	fn one_failed_file_rolls_back_the_ones_that_succeeded() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family = fan(
			"noto",
			&["https://bad.example/a.otf", "https://good.example/b.otf"],
		);
		// A second source so the family can still end up stored; the point is
		// what the failed one leaves behind.
		family.source.push(FontSource {
			name: Some("good".into()),
			files: vec![FontFile::Url("https://good.example/c.otf".into())],
			archives: Vec::new(),
		});
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://good.example/b.otf".into(), font.clone());
		fake.bodies
			.insert("https://good.example/c.otf".into(), font.clone());
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			2,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		// Only the surviving source's one file is on disk: the failed source's
		// successful transfer was rolled back with it.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names.len(), 1, "{names:?}");
	}

	#[test]
	fn a_directory_that_cannot_be_created_fails_every_family() {
		let dir = tempfile::tempdir().unwrap();
		// A file where the directory should be, so creation cannot succeed.
		let blocked = dir.path().join("fonts");
		fs::write(&blocked, b"not a directory").unwrap();
		let fake = Fake::default();
		let summary = run(
			&[fan("noto", &["https://x.example/a.otf"])],
			&blocked,
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		assert_eq!(summary.failed[0].0, "noto");
		assert!(
			summary.failed[0].1.contains("Cannot create"),
			"{}",
			summary.failed[0].1
		);
		assert_eq!(summary.requested, ["noto"]);
	}

	/// A refresh that succeeds replaces the old copy and keeps no leftover.
	#[test]
	fn a_successful_refresh_replaces_the_installed_copy() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let family = fan("noto", &["https://x.example/a.otf"]);
		let installed = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&installed), b"old copy").unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.otf".into(), font.clone());
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		assert_eq!(fs::read(dir.path().join(&installed)).unwrap(), font);
		// The copy that was moved aside is gone once the new one is in place.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names, [installed], "{names:?}");
	}

	/// Two mirrors may name one face differently. A refresh from the other
	/// mirror must replace the copy it already has, not add a second one.
	#[test]
	fn a_refresh_from_another_mirror_replaces_the_installed_copy() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family =
			fan("noto", &["https://cdn.example/NotoSerif-Regular.ttf"]);
		// The family's names are the ones the fixture itself declares, so the
		// reconciliation can recognize its own copy.
		family.lookfor = declared_names(&font);
		// A release archive names the same face by its member path.
		family.source.push(FontSource {
			name: Some("archive".into()),
			files: Vec::new(),
			archives: vec![FontArchive {
				url: "https://host.example/NotoSerif.zip".into(),
				sha256: None,
				members: vec![
					"NotoSerif/hinted/ttf/NotoSerif-Regular.ttf".into(),
				],
			}],
		});
		let archive = fs::read(zip_of(&[(
			"NotoSerif/hinted/ttf/NotoSerif-Regular.ttf",
			&font,
		)]))
		.unwrap();
		let mut fake = Fake::default();
		fake.bodies.insert(
			"https://cdn.example/NotoSerif-Regular.ttf".into(),
			font.clone(),
		);
		fake.bodies
			.insert("https://host.example/NotoSerif.zip".into(), archive);
		// The direct source is unreachable here, so the archive installs first,
		// under the name its member path gives it.
		fake.fail.push("cdn.example".into());
		let summary = run(
			&[family.clone()],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		let first: Vec<String> = names_in(dir.path());
		assert_eq!(first.len(), 1, "{first:?}");
		// Now the direct mirror answers, and the face moves to its other name.
		fake.fail.clear();
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		let second = names_in(dir.path());
		assert_eq!(second.len(), 1, "{second:?}");
		assert_ne!(first, second, "the name should follow the mirror");
		assert_eq!(fs::read(dir.path().join(&second[0])).unwrap(), font);
	}

	/// A reader's own file, and a face another family also declares, are never
	/// retired by a refresh.
	#[test]
	fn a_refresh_leaves_foreign_files_alone() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family =
			fan("noto", &["https://cdn.example/NotoSerif-Regular.ttf"]);
		family.lookfor = declared_names(&font);
		// A plainly named file that does not follow this application's scheme,
		// and a generated one whose stem nothing installs.
		fs::write(dir.path().join("NotoSerif-Regular.ttf"), &font).unwrap();
		fs::write(dir.path().join("Other-Regular-0123456789abcdef.ttf"), &font)
			.unwrap();
		let mut fake = Fake::default();
		fake.bodies.insert(
			"https://cdn.example/NotoSerif-Regular.ttf".into(),
			font.clone(),
		);
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 1);
		let names = names_in(dir.path());
		assert_eq!(names.len(), 3, "{names:?}");
		assert!(names.contains(&"NotoSerif-Regular.ttf".to_string()));
		assert!(
			names.contains(&"Other-Regular-0123456789abcdef.ttf".to_string())
		);
	}

	/// The family names a font file declares, read through the same scanner the
	/// catalogue uses.
	fn declared_names(font: &[u8]) -> Vec<String> {
		let dir = tempfile::tempdir().unwrap();
		fs::write(dir.path().join("probe.ttf"), font).unwrap();
		describe(dir.path())[0].families.clone()
	}

	fn names_in(dir: &Path) -> Vec<String> {
		let mut names: Vec<String> = fs::read_dir(dir)
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		names
	}

	/// A refresh that fails half way must leave the copies it would have
	/// replaced exactly where they were.
	#[test]
	fn a_failed_refresh_keeps_the_files_it_would_have_replaced() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut family = fan(
			"noto",
			&["https://x.example/a.otf", "https://x.example/b.otf"],
		);
		family.source[0].files[1] = FontFile::Full {
			url: "https://x.example/b.otf".into(),
			sha256: Some("0".repeat(64)),
		};
		// The first file is already installed, and this run replaces it.
		let installed = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&installed), b"old copy").unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.otf".into(), font.clone());
		fake.bodies
			.insert("https://x.example/b.otf".into(), font.clone());
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			2,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		// The old copy is untouched and no new file appeared beside it.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names, std::slice::from_ref(&installed), "{names:?}");
		assert_eq!(fs::read(dir.path().join(&installed)).unwrap(), b"old copy");
	}

	/// A member that fails after earlier ones were staged must take those
	/// staged files with it, exactly as a cancellation in the staging loop
	/// does: neither the popped member nor a `Staged` record may leak a
	/// temporary file into the download directory.
	#[test]
	fn a_failed_member_removes_the_members_staged_before_it() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let archive = fs::read(zip_of(&[
			("a/Good.otf", &font),
			("a/Broken.otf", b"<html>"),
			("a/Never.otf", &font),
		]))
		.unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.zip".into(), archive);
		let mut family = fan("noto", &[]);
		family.source[0].files.clear();
		family.source[0].archives.push(FontArchive {
			url: "https://x.example/a.zip".into(),
			sha256: None,
			members: vec!["**/*.otf".into()],
		});
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		assert!(
			summary.failed[0].1.contains("not a font"),
			"{}",
			summary.failed[0].1
		);
		// Neither the staged member nor the ones never reached survive.
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert!(names.is_empty(), "{names:?}");
	}

	/// Cancelling after the archive has arrived must stop the extraction and
	/// the installation too, not only the transfer.
	#[test]
	fn a_cancelled_extraction_installs_nothing() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let archive = fs::read(zip_of(&[
			("a/Regular.otf", &font),
			("a/Bold.otf", &font),
		]))
		.unwrap();
		let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
		let mut fake = Fake {
			arm: Some(cancelled.clone()),
			..Default::default()
		};
		fake.bodies
			.insert("https://x.example/a.zip".into(), archive);
		let mut family = fan("noto", &[]);
		family.source[0].files.clear();
		family.source[0].archives.push(FontArchive {
			url: "https://x.example/a.zip".into(),
			sha256: None,
			members: vec!["**/*.otf".into()],
		});
		let cancel = {
			let cancelled = cancelled.clone();
			Arc::new(move |_: &str| {
				cancelled.load(std::sync::atomic::Ordering::Relaxed)
			})
		};
		let summary = run(&[family], dir.path(), &fake, 1, cancel, &mut |_| {});
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.cancelled, ["noto"]);
		// Neither an installed face nor a temporary file survives.
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}

	#[test]
	fn a_bad_digest_fails_the_source_rather_than_storing_it() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.otf".into(), font.clone());
		let mut family = fan("noto", &["https://x.example/a.otf"]);
		family.source[0].files[0] = FontFile::Full {
			url: "https://x.example/a.otf".into(),
			sha256: Some("0".repeat(64)),
		};
		let summary = run(
			&[family],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		assert!(
			summary.failed[0].1.contains("sha256"),
			"{}",
			summary.failed[0].1
		);
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}

	#[test]
	fn a_cancelled_family_stores_nothing() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let mut fake = Fake::default();
		fake.bodies.insert("https://x.example/a.otf".into(), font);
		let summary = run(
			&[fan("noto", &["https://x.example/a.otf"])],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| true),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.cancelled, ["noto"]);
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}

	#[test]
	fn a_body_that_is_not_a_font_is_refused() {
		let dir = tempfile::tempdir().unwrap();
		let mut fake = Fake::default();
		fake.bodies
			.insert("https://x.example/a.otf".into(), b"<html>".to_vec());
		let summary = run(
			&[fan("noto", &["https://x.example/a.otf"])],
			dir.path(),
			&fake,
			1,
			Arc::new(|_: &str| false),
			&mut |_| {},
		);
		assert_eq!(summary.stored, 0);
		assert_eq!(summary.failed.len(), 1);
		assert!(
			summary.failed[0].1.contains("not a font"),
			"{}",
			summary.failed[0].1
		);
		assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
	}

	#[test]
	fn the_catalogue_follows_the_layers_and_the_disk() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let low = family("noto", &["https://x.example/a.otf"]);
		let high = FontFamily {
			description: Some("higher".into()),
			..low.clone()
		};
		// Nothing on disk and no system font: the family is missing.
		let config = crate::test_support::fonts();
		let listed = catalog(
			[("builtin", std::slice::from_ref(&low))],
			Some(dir.path()),
			&config,
		);
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].owners, ["builtin"]);
		assert_eq!(listed[0].state, State::Missing);
		// A higher layer replaces the entry, and both owners are kept.
		let listed = catalog(
			[
				("builtin", std::slice::from_ref(&low)),
				("mine", std::slice::from_ref(&high)),
			],
			Some(dir.path()),
			&config,
		);
		assert_eq!(listed.len(), 1);
		assert_eq!(listed[0].family.description.as_deref(), Some("higher"));
		assert_eq!(listed[0].owners, ["builtin", "mine"]);
		// A stored file whose own family name matches makes it downloaded.
		let name = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&name), &font).unwrap();
		let mut named = low.clone();
		named.lookfor = vec!["Noto Serif".into()];
		let listed = catalog(
			[("builtin", std::slice::from_ref(&named))],
			Some(dir.path()),
			&config,
		);
		assert_eq!(listed[0].state, State::Downloaded);
		assert_eq!(listed[0].files, [name]);
		assert_eq!(listed[0].bytes, font.len() as u64);
	}

	/// The bulk action says "missing"; an explicit request may still name a
	/// family an installed face already covers.
	#[test]
	fn a_selection_narrows_by_state() {
		let entry = |id: &str, state: State| Family {
			family: fan(id, &["https://x.example/a.otf"]),
			owners: vec!["builtin".into()],
			state,
			files: Vec::new(),
			bytes: 0,
		};
		let catalog = vec![
			entry("have", State::Downloaded),
			entry("installed", State::Provided),
			entry("want", State::Missing),
		];
		let ids = vec![
			"have".to_string(),
			"installed".to_string(),
			"want".to_string(),
		];
		let names = |scope| {
			select(&catalog, &ids, scope)
				.into_iter()
				.map(|family| family.id.clone())
				.collect::<Vec<_>>()
		};
		assert_eq!(names(Scope::Missing), ["want"]);
		assert_eq!(names(Scope::Named), ["installed", "want"]);
		assert_eq!(names(Scope::All), ["have", "installed", "want"]);
	}

	#[test]
	fn a_selection_leaves_out_what_is_already_downloaded() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let family = family("noto", &["https://x.example/a.otf"]);
		let mut named = family.clone();
		named.lookfor = vec!["Noto Serif".into()];
		let name = local_name("noto", "a.otf", &font);
		fs::write(dir.path().join(&name), &font).unwrap();
		let config = crate::test_support::fonts();
		let listed = catalog(
			[("builtin", std::slice::from_ref(&named))],
			Some(dir.path()),
			&config,
		);
		let ids = vec!["noto".to_string()];
		assert!(select(&listed, &ids, Scope::Named).is_empty());
		assert_eq!(select(&listed, &ids, Scope::All).len(), 1);
	}
}
