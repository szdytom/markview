//! Downloadable font families and the user font directory they land in.
//!
//! Nothing here runs on its own: a stylesheet only names families, and the
//! reader or the `fonts` command turns that list into an explicit download.
//! The directory is nothing more than another `--fonts` directory, so what a
//! family is called is decided by the font files themselves; the only state
//! kept here is what is on disk.
//!
//! The work is split by concern: [`catalog`] reads what each file declares and
//! builds the catalogue, [`download`] orchestrates probes, retries and
//! transfers, [`archive`] recognizes and unpacks containers, and [`report`]
//! owns the progress and summary a run reports.
use anyhow::{Context, Result, bail};
use markview_layout::fonts::FontConfig;
use std::{
	collections::HashMap,
	io::Read,
	path::{Path, PathBuf},
	time::Duration,
};
use tokio_util::sync::CancellationToken;

mod archive;
mod catalog;
mod download;
mod report;

#[allow(unused_imports)]
pub use archive::{Container, glob_match, sniff};
#[allow(unused_imports)]
pub use catalog::{
	FaceInfo, FaceStyle, Family, Scope, State, catalog, describe, local_name,
	select,
};
pub(crate) use catalog::{catalog_faces, describe_one};
pub use report::{Phase, Progress, Summary};

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

pub(crate) fn validate_directories(directories: &[PathBuf]) -> Result<()> {
	for directory in directories {
		if !directory.is_dir() {
			bail!("--fonts: {} is not a directory", directory.display());
		}
	}
	Ok(())
}

/// Resolve export faces once; explicit files exclude system and personal fonts.
pub(crate) fn export_config(
	mut config: FontConfig,
	files: &[PathBuf],
) -> Result<FontConfig> {
	validate_directories(&config.directories)?;
	if !files.is_empty() {
		use std::hash::{DefaultHasher, Hash, Hasher};
		let mut identity = DefaultHasher::new();
		let mut faces = Vec::new();
		for path in files {
			let mut data = Vec::new();
			crate::file::open_regular(path)?
				.take(MAX_FILE_BYTES + 1)
				.read_to_end(&mut data)
				.with_context(|| {
					format!("Cannot read font {}", path.display())
				})?;
			if data.len() as u64 > MAX_FILE_BYTES {
				bail!("Font {} exceeds 64 MiB", path.display());
			}
			if !markview_layout::fonts::is_font(&data) {
				bail!("Invalid font file: {}", path.display());
			}
			data.hash(&mut identity);
			faces.push(parley::fontique::Blob::new(std::sync::Arc::new(data)));
		}
		let mut explicit = FontConfig::from_faces(identity.finish(), faces);
		explicit.directories = config.directories;
		return Ok(explicit);
	}
	join_download_directory(&mut config, directory());
	Ok(config)
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

pub async fn run_async<T: Transport>(
	families: &[markview_layout::style::FontFamily],
	dir: &Path,
	transport: &T,
	services: &crate::services::Handle,
	cancels: HashMap<String, CancellationToken>,
	report: impl FnMut(Progress) + Send,
) -> Summary {
	let cancels = cancels
		.into_iter()
		.map(|(id, token)| (id, download::Cancellation::new(token)))
		.collect();
	download::execute(families, dir, transport, services, cancels, report).await
}

#[cfg(test)]
type Predicate = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[cfg(test)]
fn run<T: Transport>(
	families: &[markview_layout::style::FontFamily],
	dir: &Path,
	transport: &T,
	jobs: usize,
	predicate: Predicate,
	report: &mut (dyn FnMut(Progress) + Send),
) -> Summary {
	let services = crate::services::Services::new(jobs);
	let cancels = families
		.iter()
		.map(|family| {
			(
				family.id.clone(),
				download::Cancellation::with_predicate(
					services.handle.cancel.child_token(),
					&family.id,
					predicate.clone(),
				),
			)
		})
		.collect();
	tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.unwrap()
		.block_on(download::execute(
			families,
			dir,
			transport,
			&services.handle,
			cancels,
			report,
		))
}

#[cfg(test)]
mod test_support {
	use super::download::host_of;
	use super::*;
	use markview_layout::style::{FontFamily, FontFile, FontSource};
	use std::{
		collections::BTreeMap,
		fs,
		sync::{Arc, Mutex},
		time::Duration,
	};

	/// A real face, so the "is a font" check is exercised rather than stubbed.
	pub(super) fn font_bytes() -> Vec<u8> {
		fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-layout/tests/fonts/NotoSerif-Regular-subset.otf",
		))
		.unwrap()
	}

	pub(super) fn family(id: &str, files: &[&str]) -> FontFamily {
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

	pub(super) fn fan(id: &str, urls: &[&str]) -> FontFamily {
		family(id, urls)
	}

	/// A transport that serves canned bodies and mirrors, with no network.
	#[derive(Default)]
	pub(super) struct Fake {
		pub(super) bodies: BTreeMap<String, Vec<u8>>,
		pub(super) fail: Vec<String>,
		pub(super) probe: BTreeMap<String, Duration>,
		pub(super) seen: Mutex<Vec<String>>,
		/// Flipped by a successful transfer, so a test can cancel a job after
		/// its download finished but before it extracts or installs.
		pub(super) arm: Option<Arc<std::sync::atomic::AtomicBool>>,
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
}
