//! The one policy for what a document-controlled link may do.
//!
//! A link is either a remote URL, a Markdown document that opens in a tab, an
//! inert file or a directory handed to the operating system, or an unknown
//! local target that needs the reader's confirmation. There is no second path:
//! every `open::that_detached` call on document content goes through here.
use std::path::{Path, PathBuf};

/// What clicking a link asks for, before any I/O happens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
	/// A `http`/`https`/`mailto` URL for the OS handler.
	Remote(String),
	/// A Markdown document, parsed in a new tab.
	Markdown(PathBuf),
	/// An inert local file, or a directory, for the OS handler.
	OsDirect(PathBuf),
	/// A local file whose type is not known to be safe.
	Confirm(PathBuf),
}

/// Markdown documents the reader opens itself.
const MARKDOWN: &[&str] = &["md", "markdown", "mdown"];

/// Types a system viewer consumes as data. None of them is a script or an
/// installer, and each is handled by a renderer rather than a shell. `.svg` is
/// scriptable XML and `.txt` is text; both are accepted knowingly, see
/// `docs/security.md`.
const INERT: &[&str] = &[
	// Plain text
	"txt", // Images
	"png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "avif", "tiff",
	"tif", "heic", // Fixed-layout documents and e-books
	"pdf", "epub", "mobi", "azw3", "djvu", "cbz", "cbr", "xps", "oxps",
	// Audio
	"mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "wma", "aiff",
	"mid", "midi", // Video
	"mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg",
	"3gp", "ogv",
];

/// Resolves a link against the open document's directory.
///
/// `None` means the link is refused outright: an unknown URL scheme, an empty
/// target, or a bare fragment, which the caller resolves inside the reader.
pub(super) fn resolve(
	link: &str,
	document_dir: Option<&Path>,
) -> Option<Target> {
	let link = link.trim();
	if link.is_empty() {
		return None;
	}
	// An absolute URL is an allowed remote scheme, a local file URL, or
	// refused. Only the OS can consume a URL, so no path is involved.
	if let Ok(url) = url::Url::parse(link)
		&& !url.scheme().is_empty()
	{
		return match url.scheme().to_ascii_lowercase().as_str() {
			"http" | "https" | "mailto" => {
				Some(Target::Remote(url.to_string()))
			}
			"file" => url.to_file_path().ok().map(|p| classify(&canonical(p))),
			_ => None,
		};
	}
	let target = link.split(['#', '?']).next().unwrap_or(link);
	if target.is_empty() {
		return None;
	}
	let decoded =
		percent_encoding::percent_decode_str(target).decode_utf8_lossy();
	let path = Path::new(decoded.as_ref());
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		document_dir.unwrap_or(Path::new(".")).join(path)
	};
	Some(classify(&canonical(path)))
}

/// Resolves symlinks when the path exists, so the confirmation shows the real
/// target and a duplicate tab reuses the first one.
fn canonical(path: PathBuf) -> PathBuf {
	std::fs::canonicalize(&path).unwrap_or(path)
}

fn classify(path: &Path) -> Target {
	// A directory opens in the file manager, which cannot run what it shows.
	if path.is_dir() {
		return Target::OsDirect(path.to_path_buf());
	}
	let extension = path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase);
	match extension.as_deref() {
		Some(ext) if MARKDOWN.contains(&ext) => Target::Markdown(path.into()),
		Some(ext) if INERT.contains(&ext) => Target::OsDirect(path.into()),
		_ => Target::Confirm(path.into()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn real(path: &Path) -> PathBuf {
		std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
	}

	#[test]
	fn only_three_remote_schemes_are_allowed() {
		for link in [
			"https://example.com/a?b=c#d",
			"HTTP://example.com",
			"mailto:reader@example.com",
		] {
			assert!(
				matches!(resolve(link, None), Some(Target::Remote(_))),
				"{link}"
			);
		}
		for link in [
			"javascript:alert(1)",
			"ftp://example.com",
			"data:text/html,<script>alert(1)</script>",
			"#section",
			"",
		] {
			assert_eq!(resolve(link, None), None, "{link}");
		}
		// A file URL is a local path, so it goes through the same classes.
		assert!(matches!(
			resolve("file:///nonexistent/x", None),
			Some(Target::Confirm(_))
		));
	}

	#[test]
	fn local_files_are_classified_by_extension() {
		let dir = tempfile::tempdir().unwrap();
		let docs = dir.path().join("docs");
		std::fs::create_dir_all(&docs).unwrap();
		let write = |name: &str| -> PathBuf {
			let path = docs.join(name);
			std::fs::write(&path, b"x").unwrap();
			real(&path)
		};
		let markdown = |name: &str| {
			let path = docs.join(name);
			std::fs::canonicalize(path).unwrap()
		};
		std::fs::write(docs.join("note.md"), b"x").unwrap();
		assert_eq!(
			resolve("note.md", Some(&docs)),
			Some(Target::Markdown(markdown("note.md")))
		);
		for name in ["a.txt", "a.pdf", "a.png", "a.svg", "a.epub", "a.mp3"] {
			assert_eq!(
				resolve(name, Some(&docs)),
				Some(Target::OsDirect(write(name))),
				"{name}"
			);
		}
		for name in ["a.html", "a.ps", "a.eps", "a.swf", "a.zip", "a.ps1"] {
			assert_eq!(
				resolve(name, Some(&docs)),
				Some(Target::Confirm(write(name))),
				"{name}"
			);
		}
		assert_eq!(
			resolve("noext", Some(&docs)),
			Some(Target::Confirm(write("noext")))
		);
	}

	#[test]
	fn directories_and_file_urls_open_directly() {
		let dir = tempfile::tempdir().unwrap();
		let sub = dir.path().join("sub");
		std::fs::create_dir(&sub).unwrap();
		let expected = Some(Target::OsDirect(real(&sub)));
		assert_eq!(resolve("sub", Some(dir.path())), expected);
		assert_eq!(resolve("sub/", Some(dir.path())), expected);
		let url = url::Url::from_directory_path(&sub).unwrap().to_string();
		assert_eq!(resolve(&url, Some(dir.path())), expected);
	}

	#[test]
	fn dot_dot_still_names_a_relative_location() {
		let dir = tempfile::tempdir().unwrap();
		let docs = dir.path().join("docs");
		std::fs::create_dir_all(&docs).unwrap();
		std::fs::write(dir.path().join("up.md"), b"x").unwrap();
		assert_eq!(
			resolve("../up.md", Some(&docs)),
			Some(Target::Markdown(real(&dir.path().join("up.md"))))
		);
	}
}
