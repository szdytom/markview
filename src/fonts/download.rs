//! Download orchestration: probe each source once, retry a family across its
//! mirrors, transfer the files, and merge every event into one progress table.
use anyhow::{Result, bail};
use futures_util::{FutureExt, StreamExt, stream::FuturesUnordered};
use markview_layout::style::{FontArchive, FontFamily, FontFile, FontSource};
use std::{
	collections::{BTreeMap, HashMap},
	fs,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};
use tokio_util::sync::CancellationToken;

use super::{
	MAX_ARCHIVE_BYTES, MAX_FILE_BYTES, Phase, Progress, Summary, Transport,
	archive::{sha256_file, sniff_file, temp_path, unpack},
	catalog::{Staged, install, retire_replaced, stage, url_basename},
	report::{Change, Reporter, initial_states, merge_progress},
};

/// A probe shared by every source on one host.
pub(super) type Probe<'a> =
	futures_util::future::Shared<futures_util::future::BoxFuture<'a, Duration>>;

#[cfg(test)]
pub(super) type CancelPredicate =
	std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub(super) struct Download<'a, T> {
	pub(super) transport: &'a T,
	pub(super) services: &'a crate::services::Handle,
	pub(super) cancel: &'a Cancellation,
}

#[cfg(not(test))]
#[derive(Clone)]
pub(super) struct Cancellation {
	pub(super) token: CancellationToken,
}

/// In tests a predicate cancels a family between its transfers.
#[cfg(test)]
#[derive(Clone)]
pub(super) struct Cancellation {
	pub(super) token: CancellationToken,
	predicate: Option<(String, CancelPredicate)>,
}

impl Cancellation {
	pub(super) fn new(token: CancellationToken) -> Self {
		Self {
			token,
			#[cfg(test)]
			predicate: None,
		}
	}
	pub(super) fn is_cancelled(&self) -> bool {
		#[cfg(test)]
		if let Some((id, predicate)) = &self.predicate
			&& predicate(id)
		{
			self.token.cancel();
		}
		self.token.is_cancelled()
	}
}

#[cfg(test)]
impl Cancellation {
	/// A cancellation that also consults a per-family test predicate.
	pub(super) fn with_predicate(
		token: CancellationToken,
		id: &str,
		predicate: CancelPredicate,
	) -> Self {
		Self {
			token,
			predicate: Some((id.to_owned(), predicate)),
		}
	}
}

/// Runs one download over every requested family.
pub(super) async fn execute<T: Transport>(
	families: &[FontFamily],
	dir: &Path,
	transport: &T,
	services: &crate::services::Handle,
	cancels: HashMap<String, Cancellation>,
	mut report: impl FnMut(Progress) + Send,
) -> Summary {
	if families.is_empty() {
		return Summary::default();
	}
	let mut summary = Summary {
		requested: families.iter().map(|f| f.id.clone()).collect(),
		..Default::default()
	};
	if !prepare_directory(dir, families, &mut summary, &mut report).await {
		return summary;
	}
	let run = next_run();
	let probes = probe_limit(families, transport, services);
	let (tx, mut rx) = tokio::sync::mpsc::channel(64);
	let mut states = initial_states(families, &mut report);
	let mut running = FuturesUnordered::new();
	for family in families {
		let reporter = Reporter {
			id: family.id.clone(),
			run,
			attempt: 0,
			tx: tx.clone(),
		};
		let cancel = cancels.get(&family.id).cloned().unwrap_or_else(|| {
			Cancellation::new(services.cancel.child_token())
		});
		running.push(
			retry_family(
				family,
				dir,
				transport,
				services,
				cancel,
				probes.clone(),
				reporter,
			)
			.boxed(),
		);
	}
	drop(tx);
	merge_progress(
		&mut summary,
		&mut running,
		&mut rx,
		&mut states,
		run,
		&mut report,
	)
	.await;
	summarize(summary)
}

/// Creates the download directory, failing every family if it cannot.
async fn prepare_directory(
	dir: &Path,
	families: &[FontFamily],
	summary: &mut Summary,
	report: &mut (impl FnMut(Progress) + Send),
) -> bool {
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
		return false;
	}
	true
}

fn next_run() -> usize {
	static RUN: AtomicUsize = AtomicUsize::new(1);
	RUN.fetch_add(1, Ordering::Relaxed)
}

/// One probe per distinct host, shared by every source that names it.
fn probe_limit<'a, T: Transport>(
	families: &[FontFamily],
	transport: &'a T,
	services: &'a crate::services::Handle,
) -> HashMap<String, Probe<'a>> {
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
	probes
}

/// Downloads one family, trying its sources nearest-first until one works.
async fn retry_family<'a, T: Transport>(
	family: &'a FontFamily,
	dir: &'a Path,
	transport: &'a T,
	services: &'a crate::services::Handle,
	cancel: Cancellation,
	probes: HashMap<String, Probe<'a>>,
	reporter: Reporter,
) -> (String, Outcome) {
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
			family, dir, transport, services, &latencies, &reporter, &cancel,
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
		Outcome::Failed(reason) => (Phase::Failed, Some(reason.clone())),
		Outcome::Cancelled => (Phase::Cancelled, None),
	};
	// Terminal events apply to the latest attempt, independently of file events.
	let mut terminal = reporter.clone();
	terminal.attempt = usize::MAX;
	terminal.send(Change::Terminal { phase, note }).await;
	(family.id.clone(), outcome)
}

/// Settles the summary a caller reads: failures and cancellations in order.
fn summarize(mut summary: Summary) -> Summary {
	summary.failed.sort();
	summary.cancelled.sort();
	summary
}

pub(super) fn first_url(source: &FontSource) -> Option<&str> {
	source
		.files
		.first()
		.map(FontFile::url)
		.or_else(|| source.archives.first().map(|a| a.url.as_str()))
}
pub(super) fn host_of(url: &str) -> String {
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
	let download = Download {
		transport,
		services,
		cancel,
	};
	let mut staged =
		fetch_files(family, source, dir, &download, reporter).await?;
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
	install_source(services, cancel, dir, family, staged, bytes).await
}

/// Transfers one source's files, keeping the first error for the whole source.
async fn fetch_files<T: Transport>(
	family: &FontFamily,
	source: &FontSource,
	dir: &Path,
	download: &Download<'_, T>,
	reporter: &Reporter,
) -> Result<Vec<Staged>> {
	// Only the budget's worth of file futures are constructed at a time.
	let mut pending = source.files.iter().enumerate();
	let mut files = FuturesUnordered::new();
	let mut staged = Vec::new();
	let mut first_error = None;
	loop {
		while files.len() < download.services.jobs {
			let Some((index, file)) = pending.next() else {
				break;
			};
			files.push(
				fetch_file(family, file, dir, download, reporter, index)
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
	match first_error {
		Some(error) => Err(error),
		None => Ok(staged),
	}
}

/// Installs a source's staged files only after every transfer succeeded.
async fn install_source(
	services: &crate::services::Handle,
	cancel: &Cancellation,
	dir: &Path,
	family: &FontFamily,
	mut staged: Vec<Staged>,
	bytes: u64,
) -> Result<u64> {
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
	let temp = DownloadTemp(temp_path(dir)?, true);
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
	let temp = DownloadTemp(temp_path(dir)?, true);
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
			extract_archive(&family, &archive, &temp.0, &dir, &cancelled)
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

/// Verifies and unpacks one archive body, staging every member it contributes.
fn extract_archive(
	family: &FontFamily,
	archive: &FontArchive,
	path: &Path,
	dir: &Path,
	cancelled: &Cancellation,
) -> Result<Vec<Staged>> {
	let basename = url_basename(&archive.url);
	if let Some(expected) = &archive.sha256
		&& !sha256_file(path)?.eq_ignore_ascii_case(expected)
	{
		bail!("sha256 mismatch for {basename}");
	}
	let container = sniff_file(path)?;
	let mut extracted = unpack(container, path, &archive.members, dir)?;
	let mut staged = Vec::new();
	while !extracted.is_empty() {
		if cancelled.is_cancelled() {
			bail!("Cancelled");
		}
		let Some((member_temp, member_name)) = extracted.next_file() else {
			break;
		};
		let mut member = DownloadTemp(member_temp, true);
		staged.push(stage(family, &member.0, &member_name, None)?);
		member.1 = false;
	}
	Ok(staged)
}

/// What one family's download ended as.
pub(super) type FamilyJob<'a> =
	futures_util::future::BoxFuture<'a, (String, Outcome)>;

/// What one family's download ended as.
pub(super) enum Outcome {
	Stored { bytes: u64 },
	Failed(String),
	Cancelled,
}

#[cfg(test)]
mod tests {
	use super::super::{
		local_name, run,
		test_support::{Fake, fan, font_bytes},
	};
	use super::*;
	use std::sync::Arc;

	/// A color emoji face carries no outline table, only `CBDT` strikes, and
	/// the reader draws it, so a download must store it like any other face.
	#[test]
	fn a_bitmap_color_emoji_is_a_downloadable_font() {
		let dir = tempfile::tempdir().unwrap();
		let emoji = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-layout/tests/fonts/NotoColorEmoji-subset.ttf",
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
}
