//! Source resolution and bounded reads; independent of decoding and scheduling.
use anyhow::{Context, Result, bail};
use base64::Engine;
use std::{
	fs,
	io::Read,
	path::{Path, PathBuf},
	time::SystemTime,
};
pub(super) const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Source {
	File(PathBuf),
	Http(String),
	Data(String),
	/// Mermaid diagram source, rendered locally instead of fetched.
	Diagram(String),
}

pub(super) fn source(src: &str, document: &Path) -> Result<Source> {
	if src.is_empty() {
		bail!("Missing image source");
	}
	if let Some(code) = src.strip_prefix(markview_core::image::MERMAID_SCHEME) {
		return Ok(Source::Diagram(code.to_owned()));
	}
	if let Ok(url) = url::Url::parse(src) {
		return match url.scheme() {
			// A document-controlled URL is fetched only through the pinned
			// client in `net`. `--offline` is applied there rather than here,
			// so an already cached body can still be served.
			"http" | "https" => Ok(Source::Http(url.to_string())),
			"data" => Ok(Source::Data(src.to_owned())),
			_ => anyhow::bail!("Unsupported image URL scheme"),
		};
	}
	let decoded = percent_encoding::percent_decode_str(src)
		.decode_utf8()
		.context("Invalid path encoding")?;
	let path = Path::new(decoded.as_ref());
	// Only paths relative to the document are reachable. `..` is allowed: it
	// names another relative location, and the reader cannot exfiltrate what
	// it reads. See `docs/security.md` for the accepted residual.
	if rooted(path) {
		bail!("Absolute image paths are not allowed");
	}
	let path = document.parent().unwrap_or(Path::new(".")).join(path);
	Ok(Source::File(fs::canonicalize(&path).unwrap_or(path)))
}

/// Whether a path names an absolute location, including the Windows forms
/// `\foo` (rooted, not `is_absolute`) and `C:foo` (drive-relative).
pub(super) fn rooted(path: &Path) -> bool {
	if path.is_absolute() {
		return true;
	}
	matches!(
		path.components().next(),
		Some(std::path::Component::Prefix(_) | std::path::Component::RootDir)
	)
}

pub(super) fn bounded(mut reader: impl Read) -> Result<Vec<u8>> {
	let mut bytes = Vec::new();
	reader
		.by_ref()
		.take((MAX_BYTES + 1) as u64)
		.read_to_end(&mut bytes)?;
	if bytes.len() > MAX_BYTES {
		bail!("Image exceeds 32 MiB");
	}
	Ok(bytes)
}

/// Reads a source. A remote source goes through the shared pinned client and
/// the disk cache, so `--offline` is decided here rather than at resolution.
pub(super) fn fetch(
	source: &Source,
	offline: bool,
	cache: Option<&super::cache::Cache>,
) -> Result<Vec<u8>> {
	match source {
		Source::File(path) => {
			let file = fs::File::open(path).context("Cannot open image")?;
			if !file.metadata()?.is_file() {
				bail!("Image is not a regular file");
			}
			bounded(file)
		}
		Source::Http(url) => super::cache::fetch_http(url, offline, cache),
		// The rendered SVG feeds the same rasterizer as an SVG file.
		Source::Diagram(code) => {
			Ok(super::diagram::svg(code)?.as_bytes().to_vec())
		}
		Source::Data(uri) => {
			let (header, data) =
				uri.split_once(',').context("Invalid data URI")?;
			if !header.to_ascii_lowercase().starts_with("data:image/") {
				bail!("Data URI must contain an image");
			}
			if data.len() > MAX_BYTES * 3 {
				bail!("Image exceeds 32 MiB");
			}
			let data =
				percent_encoding::percent_decode_str(data).collect::<Vec<_>>();
			let bytes = if header.to_ascii_lowercase().ends_with(";base64") {
				base64::engine::general_purpose::STANDARD.decode(data)?
			} else {
				data
			};
			if bytes.len() > MAX_BYTES {
				bail!("Image exceeds 32 MiB");
			}
			Ok(bytes)
		}
	}
}

pub(super) fn stamp(source: &Source) -> Option<(u64, Option<SystemTime>)> {
	if let Source::File(path) = source {
		fs::metadata(path)
			.ok()
			.map(|m| (m.len(), m.modified().ok()))
	} else {
		None
	}
}
