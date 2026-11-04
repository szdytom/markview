//! Source resolution and bounded reads; independent of decoding and scheduling.
use anyhow::{Context, Result, bail};
use base64::Engine;
use markview_layout::image::{ImageSource, ResourceBase, ResourceRef};
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
	/// Diagram source, rendered locally instead of fetched.
	Diagram {
		language: String,
		source: String,
	},
}

/// The application cache key for one semantic source.
pub(super) fn key(
	source: &ImageSource,
	document: &Path,
	workspace: Option<&Path>,
	security: &crate::security::Security,
) -> String {
	match source {
		ImageSource::Diagram(diagram) => format!(
			"diagram:{}:{:x}",
			diagram.language,
			crate::document::fingerprint(&diagram.source)
		),
		ImageSource::Resource(resource) => {
			let prefix = match resource.base {
				ResourceBase::Document => "",
				ResourceBase::Vault => "vault:",
			};
			let mut key = format!("{prefix}{}", resource.address);
			if !matches!(
				security.origin,
				crate::security::Origin::Local(crate::security::Trust::Trusted)
			) {
				let base = match resource.base {
					ResourceBase::Document => document,
					ResourceBase::Vault => workspace
						.or_else(|| document.parent())
						.unwrap_or(document),
				};
				key.push('#');
				key.push_str(&format!(
					"{:x}",
					crate::document::fingerprint(&(base, security.cache_key()))
				));
			}
			key
		}
	}
}

#[cfg(test)]
pub(super) fn source(src: &str, document: &Path) -> Result<Source> {
	resolve_resource(
		&ResourceRef::document(src),
		document,
		None,
		&crate::security::Security::local(crate::security::Trust::Trusted),
	)
}

#[cfg(test)]
pub(super) fn resolve_at(
	src: &str,
	document: &Path,
	security: &crate::security::Security,
) -> Result<Source> {
	resolve_resource(&ResourceRef::document(src), document, None, security)
}

pub(super) fn resolve(
	source: &ImageSource,
	document: &Path,
	workspace: Option<&Path>,
	security: &crate::security::Security,
) -> Result<Source> {
	match source {
		ImageSource::Diagram(diagram) => Ok(Source::Diagram {
			language: diagram.language.clone(),
			source: diagram.source.clone(),
		}),
		ImageSource::Resource(resource) => {
			resolve_resource(resource, document, workspace, security)
		}
	}
}

fn resolve_resource(
	resource: &ResourceRef,
	document: &Path,
	workspace: Option<&Path>,
	security: &crate::security::Security,
) -> Result<Source> {
	let src = resource.address.as_str();
	if src.is_empty() {
		bail!("Missing image source");
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
	match &security.origin {
		crate::security::Origin::Web(base) => {
			let url = url::Url::parse(base)?.join(src)?;
			if !matches!(url.scheme(), "http" | "https") {
				bail!("Unsupported image URL scheme");
			}
			return Ok(Source::Http(url.to_string()));
		}
		crate::security::Origin::Clipboard => {
			return resolve_clipboard_image(src, security);
		}
		crate::security::Origin::Local(_) => {}
	}
	let decoded = percent_encoding::percent_decode_str(src)
		.decode_utf8()
		.context("Invalid path encoding")?;
	let path = Path::new(decoded.as_ref());
	// Only paths relative to a local base are reachable. `..` is allowed: it
	// names another relative location, and the reader cannot exfiltrate what
	// it reads. See `docs/developers/security.md` for the accepted residual.
	if rooted(path) {
		bail!("Absolute image paths are not allowed");
	}
	let base = match resource.base {
		ResourceBase::Vault => workspace
			.or_else(|| document.parent())
			.unwrap_or(Path::new(".")),
		ResourceBase::Document => document.parent().unwrap_or(Path::new(".")),
	};
	let path = base.join(path);
	let path = fs::canonicalize(&path).unwrap_or(path);
	security.check_file(&path)?;
	Ok(Source::File(path))
}

fn resolve_clipboard_image(
	src: &str,
	security: &crate::security::Security,
) -> Result<Source> {
	let decoded = percent_encoding::percent_decode_str(src)
		.decode_utf8()
		.context("Invalid path encoding")?;
	if rooted(Path::new(decoded.as_ref())) {
		bail!("Absolute image paths are not allowed");
	}
	match security.selected_image(src) {
		Some(path) => Ok(Source::File(path.to_owned())),
		None => Err(crate::security::PermissionRequired(
			crate::security::Resource::SelectImage(src.to_owned()),
		)
		.into()),
	}
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

pub(super) fn bounded(reader: impl Read) -> Result<Vec<u8>> {
	crate::net::bounded_to(reader, MAX_BYTES as u64, "Image")
}

/// Reads local and inline sources; HTTP reads use the asynchronous cache.
/// Only a diagram reads `theme`; every other source ignores it.
pub(super) fn fetch(
	source: &Source,
	_offline: bool,
	_cache: Option<&super::cache::Cache>,
	theme: &super::diagram::DiagramTheme,
) -> Result<Vec<u8>> {
	match source {
		Source::File(path) => {
			// A sibling FIFO would otherwise hold one of the four image
			// pipelines for as long as it has no writer, so the type check must
			// happen without a waiting open.
			let file =
				crate::file::open_regular(path).context("Cannot open image")?;
			bounded(file)
		}
		Source::Http(_) => {
			unreachable!("HTTP sources are read by the I/O service")
		}
		// The rendered SVG feeds the same rasterizer as an SVG file.
		Source::Diagram { language, source } => {
			if language != "mermaid" {
				bail!("Unsupported diagram language: {language}");
			}
			Ok(super::diagram::svg(source, theme)?.as_bytes().to_vec())
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
