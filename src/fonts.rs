//! The user font directory and the font files a stylesheet offers to download.
//!
//! Nothing here runs on its own: a stylesheet only names URLs, and the reader's
//! Styles panel turns that list into an explicit download. Files are verified
//! and renamed into place, so the directory never holds a partial face.
use anyhow::{Context, Result, bail};
use std::{
	fs,
	io::Write,
	path::{Path, PathBuf},
};

/// The largest single downloaded font accepted. A full Noto CJK collection
/// exceeds this on purpose; the subset and per-script faces fit.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// The largest total the user font directory may hold.
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

/// A font that failed to download, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
	/// The local file name, which is what the reader sees.
	pub file: String,
	pub reason: String,
}

/// What the Styles panel draws about a download.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
	/// Files this job set out to fetch.
	pub total: usize,
	/// Files it has finished.
	pub done: usize,
	/// Files it actually wrote. A job that stored nothing changed nothing,
	/// so the reader must not treat it as a font change.
	pub stored: usize,
	/// The file being fetched now.
	pub current: Option<String>,
	/// Failures, in the order they happened.
	pub failures: Vec<Failure>,
	pub running: bool,
	/// A message shown in place of progress: an offline refusal, or a job
	/// that found every file already present.
	pub note: Option<String>,
}

/// The user font directory beside `settings.toml`.
pub fn directory() -> Option<PathBuf> {
	crate::settings::config_path()
		.and_then(|path| path.parent().map(|parent| parent.join("fonts")))
}

/// The local name a URL is stored under.
///
/// The last path segment, reduced to characters a file name may hold, plus a
/// hash of the whole URL and a font extension. Two URLs that end in the same
/// basename therefore name two files instead of one standing in for the other,
/// while the readable basename stays as the prefix. This name is also how an
/// already-downloaded file is recognized, so it must be stable for one URL
/// across runs. Names from before this scheme are not a compatibility concern,
/// because the feature is unreleased.
pub fn file_name(url: &str) -> String {
	let raw = url::Url::parse(url)
		.ok()
		.and_then(|url| {
			url.path_segments()
				.and_then(|mut path| path.next_back())
				.map(str::to_owned)
		})
		.unwrap_or_default();
	let decoded = percent_encoding::percent_decode_str(&raw)
		.decode_utf8_lossy()
		.into_owned();
	let mut stem: String = decoded
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
				c
			} else {
				'_'
			}
		})
		.collect();
	let known = Path::new(&stem)
		.extension()
		.and_then(|extension| extension.to_str())
		.map(str::to_ascii_lowercase);
	let extension = match known.as_deref() {
		Some(extension @ ("ttf" | "otf" | "ttc" | "otc")) => {
			stem.truncate(stem.len() - extension.len() - 1);
			extension.to_owned()
		}
		_ => "ttf".into(),
	};
	if stem.is_empty() || stem == "." || stem == ".." {
		stem = "font".into();
	}
	// The hash is fixed length, so bounding the stem keeps the whole name
	// well inside a file name's limit.
	stem.truncate(96);
	format!("{stem}-{:016x}.{extension}", url_hash(url))
}

/// A stable hash of a URL, used only to keep two basenames apart.
///
/// FNV-1a is a few lines and never changes between builds; a `DefaultHasher`
/// is explicitly allowed to.
fn url_hash(url: &str) -> u64 {
	const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
	const PRIME: u64 = 0x0000_0100_0000_01b3;
	url.bytes().fold(OFFSET, |hash, byte| {
		(hash ^ u64::from(byte)).wrapping_mul(PRIME)
	})
}

/// Whether `url` is already stored in the user font directory.
///
/// Only this module writes that directory, and it verifies a body before
/// renaming it into place, so a nonempty file of the right name is complete.
pub fn present(url: &str) -> bool {
	directory().is_some_and(|dir| present_in(&dir, &file_name(url)))
}

fn present_in(dir: &Path, name: &str) -> bool {
	fs::metadata(dir.join(name))
		.is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

/// Writes `bytes` under `name` atomically, refusing a body that is not a font.
///
/// The temporary file keeps a `.tmp` name, which the shaper's directory scan
/// ignores, so a half-written download is never registered.
pub fn store(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
	if bytes.len() as u64 > MAX_FILE_BYTES {
		bail!("Font file exceeds {} MiB", MAX_FILE_BYTES / (1024 * 1024));
	}
	if !markview_core::fonts::is_font(bytes) {
		bail!("Not a font file");
	}
	fs::create_dir_all(dir)
		.with_context(|| format!("Cannot create {}", dir.display()))?;
	let mut temp = tempfile::NamedTempFile::new_in(dir)?;
	temp.write_all(bytes)?;
	temp.as_file().sync_all()?;
	temp.persist(dir.join(name))?;
	if let Ok(parent) = fs::File::open(dir) {
		let _ = parent.sync_all();
	}
	Ok(())
}

fn total_bytes(dir: &Path) -> u64 {
	fs::read_dir(dir)
		.map(|entries| {
			entries
				.flatten()
				.filter_map(|entry| entry.metadata().ok())
				.filter(|meta| meta.is_file())
				.map(|meta| meta.len())
				.sum()
		})
		.unwrap_or(0)
}

/// Fetches one URL's body, bounded by `max` bytes. Production passes
/// [`crate::images::get_body`]; tests inject a canned response.
pub type Fetch<'a> = dyn FnMut(&str, u64) -> Result<Vec<u8>> + 'a;

/// Downloads `urls` into `dir` one at a time, reporting after every change.
///
/// A file already present is skipped, and a failure is recorded rather than
/// fatal: the files after it still run. The last report has `running` false.
pub fn run(
	urls: &[String],
	dir: &Path,
	fetch: &mut Fetch<'_>,
	report: &mut dyn FnMut(Status),
) -> Status {
	let mut status = Status {
		total: urls.len(),
		running: true,
		..Default::default()
	};
	for url in urls {
		let name = file_name(url);
		if present_in(dir, &name) {
			status.done += 1;
			report(status.clone());
			continue;
		}
		status.current = Some(name.clone());
		report(status.clone());
		match download(url, dir, &name, fetch) {
			Ok(()) => {
				status.done += 1;
				status.stored += 1;
			}
			Err(error) => status.failures.push(Failure {
				file: name,
				reason: format!("{error:#}"),
			}),
		}
		report(status.clone());
	}
	status.current = None;
	status.running = false;
	report(status.clone());
	status
}

fn download(
	url: &str,
	dir: &Path,
	name: &str,
	fetch: &mut Fetch<'_>,
) -> Result<()> {
	let bytes = fetch(url, MAX_FILE_BYTES)?;
	if total_bytes(dir).saturating_add(bytes.len() as u64) > MAX_TOTAL_BYTES {
		bail!(
			"Font directory would exceed {} MiB",
			MAX_TOTAL_BYTES / (1024 * 1024)
		);
	}
	store(dir, name, &bytes)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A real face, so the "is a font" check is exercised rather than stubbed.
	fn font_bytes() -> Vec<u8> {
		fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(
			"crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf",
		))
		.unwrap()
	}

	#[test]
	fn a_url_becomes_a_plain_file_name() {
		let name = file_name("https://example.com/fonts/NotoSerif-Regular.ttf");
		assert!(name.starts_with("NotoSerif-Regular-"), "{name}");
		assert!(name.ends_with(".ttf"), "{name}");
		let name = file_name("https://example.com/a%20b.otf?token=1#x");
		assert!(name.starts_with("a_b-"), "{name}");
		assert!(name.ends_with(".otf"), "{name}");
		// A name without a known extension still lands in the scan.
		assert!(file_name("https://example.com/Serif").ends_with(".ttf"));
		// A URL that names no file gets a stable placeholder.
		assert!(file_name("https://example.com/").starts_with("font-"));
		assert!(file_name("https://example.com/../").starts_with("font-"));
	}

	/// The name keys the presence check, so two URLs must never share one even
	/// when their last path segment is the same.
	#[test]
	fn two_urls_with_the_same_basename_do_not_share_a_file() {
		let a = file_name("https://example.com/fonts/NotoSerif-Regular.ttf");
		let b = file_name("https://mirror.example/other/NotoSerif-Regular.ttf");
		assert_ne!(a, b);
		// The same URL is stable, so a later run recognizes the stored file.
		assert_eq!(
			a,
			file_name("https://example.com/fonts/NotoSerif-Regular.ttf")
		);
		// A query string is part of the identity too.
		assert_ne!(
			file_name("https://example.com/Serif.ttf?rev=1"),
			file_name("https://example.com/Serif.ttf?rev=2")
		);
	}

	#[test]
	fn storage_is_atomic_and_refuses_bytes_that_are_not_a_font() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		store(dir.path(), "a.otf", &font).unwrap();
		assert_eq!(fs::read(dir.path().join("a.otf")).unwrap(), font);
		// An error page or a truncated transfer never becomes a file at all.
		assert!(store(dir.path(), "b.otf", b"<!doctype html>").is_err());
		assert!(store(dir.path(), "c.otf", &font[..64]).is_err());
		let names: Vec<String> = fs::read_dir(dir.path())
			.unwrap()
			.flatten()
			.map(|entry| entry.file_name().to_string_lossy().into_owned())
			.collect();
		assert_eq!(names, ["a.otf"]);
	}

	#[test]
	fn a_present_file_is_skipped_and_failures_do_not_abort_the_job() {
		let dir = tempfile::tempdir().unwrap();
		let font = font_bytes();
		let present = file_name("https://example.com/Regular.ttf");
		store(dir.path(), &present, &font).unwrap();
		let urls = vec![
			"https://example.com/Regular.ttf".to_string(),
			"https://example.com/Missing.ttf".to_string(),
			"https://example.com/Bold.ttf".to_string(),
		];
		let mut fetched = Vec::new();
		let mut fetch = |url: &str, _max: u64| -> Result<Vec<u8>> {
			fetched.push(url.to_string());
			if url.ends_with("Missing.ttf") {
				bail!("HTTP 404");
			}
			Ok(font.clone())
		};
		let mut reports = Vec::new();
		let status = run(&urls, dir.path(), &mut fetch, &mut |status| {
			reports.push(status);
		});
		// The present file was never requested, and the 404 did not stop Bold.
		assert_eq!(
			fetched,
			[
				"https://example.com/Missing.ttf",
				"https://example.com/Bold.ttf"
			]
		);
		assert_eq!(status.total, 3);
		assert_eq!(status.done, 2);
		// Only Bold was written, so only it changed the directory.
		assert_eq!(status.stored, 1);
		assert!(!status.running);
		assert_eq!(status.failures.len(), 1);
		assert_eq!(
			status.failures[0].file,
			file_name("https://example.com/Missing.ttf")
		);
		assert!(status.failures[0].reason.contains("404"));
		assert!(
			dir.path()
				.join(file_name("https://example.com/Bold.ttf"))
				.exists()
		);
		// Progress only ever moves forward, and the last report is settled.
		let mut done = 0;
		for report in &reports {
			assert!(report.done >= done);
			done = report.done;
		}
		assert!(!reports.last().unwrap().running);
	}

	/// A job in which every request fails writes nothing, which the caller
	/// reads as "no font change".
	#[test]
	fn a_fully_failed_job_stores_nothing() {
		let dir = tempfile::tempdir().unwrap();
		let urls = vec!["https://example.com/Missing.ttf".to_string()];
		let mut fetch =
			|_: &str, _: u64| -> Result<Vec<u8>> { bail!("HTTP 404") };
		let status = run(&urls, dir.path(), &mut fetch, &mut |_| {});
		assert_eq!(status.done, 0);
		assert_eq!(status.stored, 0);
		assert_eq!(status.failures.len(), 1);
	}

	#[test]
	fn a_bounded_total_refuses_further_files() {
		let dir = tempfile::tempdir().unwrap();
		// A sparse file costs no disk while it fills the directory budget.
		fs::File::create(dir.path().join("big.ttf"))
			.unwrap()
			.set_len(MAX_TOTAL_BYTES)
			.unwrap();
		let font = font_bytes();
		let urls = vec!["https://example.com/Bold.ttf".to_string()];
		let mut fetch = |_: &str, _: u64| Ok(font.clone());
		let status = run(&urls, dir.path(), &mut fetch, &mut |_| {});
		assert_eq!(status.done, 0);
		assert_eq!(status.stored, 0);
		assert_eq!(status.failures.len(), 1);
		assert!(status.failures[0].reason.contains("MiB"));
		assert!(
			!dir.path()
				.join(file_name("https://example.com/Bold.ttf"))
				.exists()
		);
	}

	#[test]
	fn the_presence_check_looks_in_the_given_directory() {
		let dir = tempfile::tempdir().unwrap();
		assert!(!present_in(dir.path(), "a.ttf"));
		fs::write(dir.path().join("a.ttf"), b"x").unwrap();
		assert!(present_in(dir.path(), "a.ttf"));
		// An empty placeholder is not a download.
		fs::write(dir.path().join("b.ttf"), b"").unwrap();
		assert!(!present_in(dir.path(), "b.ttf"));
	}
}
