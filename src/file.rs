//! Bounded, read-only local file loading.
#[cfg(test)]
mod proptests;
use anyhow::{Context, Result, bail};
use std::{
	fs,
	io::{Read, Seek},
	path::Path,
};
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// The picture source of a document synthesized from one image file.
pub(crate) const STANDALONE_IMAGE_SOURCE: &str = "markview-picture";

/// Opens `path` for reading, refusing anything that is not a regular file.
///
/// The open is the one step no cancellation token reaches: a FIFO with no
/// writer blocks a pathname open forever, and a thread already inside it never
/// observes a shutdown. Opening non-blockingly keeps that wait out of the
/// process, and the type then comes from the handle the open returned, so a
/// special file is refused before a byte is read from it. Reading the type
/// after the open is what makes it a race the caller cannot lose: a path
/// swapped for a FIFO between check and open cannot reintroduce a wait, because
/// the open itself does not wait.
pub(crate) fn open_regular(path: &Path) -> Result<fs::File> {
	let file = markview_platform::open_nonblocking(path)
		.with_context(|| format!("Cannot open {}", path.display()))?;
	if !file.metadata()?.is_file() {
		bail!("Not a regular file: {}", path.display());
	}
	Ok(file)
}

/// Whether two paths name the same file, including through symlinks and hard
/// links. Metadata identity keeps this from opening a FIFO or device.
pub(crate) fn same_file(first: &Path, second: &Path) -> bool {
	markview_platform::same_file(first, second)
}

/// Image files the reader can show as a one-picture document.
pub(crate) fn is_image(path: &Path) -> bool {
	path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
		["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"]
			.iter()
			.any(|known| e.eq_ignore_ascii_case(known))
	})
}

/// Whether a file is a picture, even when its selected name hides the target's
/// extension.
pub(crate) fn is_picture(path: &Path) -> bool {
	if is_image(path) {
		return true;
	}
	let Ok(mut file) = open_regular(path) else {
		return false;
	};
	if file
		.metadata()
		.is_ok_and(|metadata| metadata.len() > MAX_FILE_BYTES)
	{
		return false;
	}
	let mut header = [0u8; 512];
	let Ok(read) = file.read(&mut header) else {
		return false;
	};
	// A short magic prefix alone misclassifies text such as `P1 issues` or
	// `BMP documentation`; asking the decoder for dimensions proves the file.
	// The reader sees the whole bounded stream, past metadata before a JPEG's
	// frame header.
	if file.rewind().is_err() {
		return false;
	}
	let reader = image::ImageReader::new(std::io::BufReader::new(file));
	if let Ok(reader) = reader.with_guessed_format()
		&& reader.into_dimensions().is_ok()
	{
		return true;
	}
	let text = String::from_utf8_lossy(&header[..read]);
	let text = text.trim_start_matches('\u{feff}').trim_start();
	if !text.starts_with("<svg")
		&& !(text.starts_with("<?xml") && text.contains("<svg"))
	{
		return false;
	}
	// Inline SVG is ordinary Markdown content, so only a whole document that
	// parses as SVG is a standalone picture. The probe must not touch external
	// resources before the image pipeline has authorized them.
	read_document(path).is_ok_and(|source| {
		let mut options = resvg::usvg::Options::default();
		options.image_href_resolver.resolve_data = Box::new(|_, _, _| None);
		options.image_href_resolver.resolve_string = Box::new(|_, _| None);
		resvg::usvg::Tree::from_str(&source, &options).is_ok()
	})
}

/// Reads a Markdown document, or synthesizes one picture from an image file.
pub(crate) fn read_markdown(path: &Path) -> Result<String> {
	if !is_picture(path) {
		return read_document(path);
	}
	// A missing or unreadable picture is a failed input, not an optional image.
	drop(open_regular(path)?);
	Ok(format!("![]({STANDALONE_IMAGE_SOURCE})\n"))
}

pub fn read_document(path: &Path) -> Result<String> {
	let mut file = open_regular(path)?;
	let before = file.metadata()?;
	if before.len() > MAX_FILE_BYTES {
		bail!("MVP file size limit is 32 MiB");
	}
	let mut bytes = Vec::with_capacity(before.len() as usize);
	Read::by_ref(&mut file)
		.take(MAX_FILE_BYTES + 1)
		.read_to_end(&mut bytes)?;
	if bytes.len() as u64 > MAX_FILE_BYTES {
		bail!("MVP file size limit is 32 MiB");
	}
	let after = file.metadata()?;
	if before.len() != after.len()
		|| before.modified().ok() != after.modified().ok()
	{
		bail!("File is still being written; waiting for the next update");
	}
	let text = String::from_utf8(bytes)
		.context("File is not complete UTF-8; waiting for a valid update")?;
	Ok(match text.strip_prefix('\u{feff}') {
		Some(text) => text.to_string(),
		None => text,
	})
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;

	use markview_platform::test_support::fifo;

	/// Runs `read` off the test thread, so a regression fails the test instead
	/// of hanging the suite.
	fn within_a_second(read: impl FnOnce() -> Result<String> + Send + 'static) {
		let (send, recv) = std::sync::mpsc::channel();
		std::thread::spawn(move || {
			let _ = send.send(read());
		});
		let result = recv
			.recv_timeout(Duration::from_secs(5))
			.expect("reading a FIFO blocked instead of being refused");
		assert!(result.is_err(), "a FIFO was read as a document");
	}

	#[test]
	fn a_fifo_document_is_refused_without_waiting() {
		if !markview_platform::test_support::special_files() {
			return;
		}
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("note.md");
		fifo(&path);
		within_a_second(move || read_document(&path));
	}

	#[test]
	fn a_directory_is_refused_as_a_document() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("folder.md");
		std::fs::create_dir(&path).unwrap();
		// Windows cannot open a directory without `FILE_FLAG_BACKUP_SEMANTICS`,
		// so it refuses before the type check; both refusals are correct.
		assert!(read_document(&path).is_err());
	}

	#[test]
	fn an_image_path_becomes_one_standalone_picture() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("Picture one.png");
		fs::write(&path, b"picture").unwrap();
		assert_eq!(
			read_markdown(&path).unwrap(),
			format!("![]({STANDALONE_IMAGE_SOURCE})\n")
		);
		fs::remove_file(&path).unwrap();
		assert!(read_markdown(&path).is_err());
	}

	#[test]
	fn an_extensionless_picture_target_is_detected() {
		let dir = tempfile::tempdir().unwrap();
		let named = dir.path().join("picture.png");
		image::RgbImage::from_pixel(3, 2, image::Rgb([12, 34, 56]))
			.save(&named)
			.unwrap();
		let target = dir.path().join("picture");
		fs::rename(named, &target).unwrap();
		assert!(is_picture(&target));
		assert_eq!(
			read_markdown(&target).unwrap(),
			format!("![]({STANDALONE_IMAGE_SOURCE})\n")
		);
	}

	#[test]
	fn markdown_prefixes_are_not_mistaken_for_pictures() {
		let dir = tempfile::tempdir().unwrap();
		for (name, source) in [
			("issues.md", "P1 issues\n\nText.\n"),
			("bmp.md", "BMP documentation\n\nText.\n"),
		] {
			let path = dir.path().join(name);
			fs::write(&path, source).unwrap();
			assert!(!is_picture(&path), "{name}");
			assert_eq!(read_markdown(&path).unwrap(), source);
		}
	}

	#[test]
	fn markdown_with_leading_inline_svg_stays_text() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("inline.md");
		let source = "<svg width=\"4\" height=\"4\"></svg>\n\n# Heading\n";
		fs::write(&path, source).unwrap();
		assert!(!is_picture(&path));
		assert_eq!(read_markdown(&path).unwrap(), source);
	}

	#[test]
	fn a_picture_with_metadata_beyond_the_first_prefix_is_detected() {
		let dir = tempfile::tempdir().unwrap();
		let named = dir.path().join("photo.jpg");
		image::RgbImage::from_pixel(3, 2, image::Rgb([12, 34, 56]))
			.save(&named)
			.unwrap();
		let mut bytes = fs::read(&named).unwrap();
		let segment = [0xff, 0xe1, 0x02, 0x5a];
		bytes.splice(2..2, segment.into_iter().chain([0u8; 600]));
		let target = dir.path().join("photo");
		fs::write(&target, bytes).unwrap();
		assert!(is_picture(&target));
	}

	#[test]
	fn svg_probe_does_not_open_external_resources() {
		if !markview_platform::test_support::special_files() {
			return;
		}
		let dir = tempfile::tempdir().unwrap();
		let pipe = dir.path().join("picture.pipe");
		fifo(&pipe);
		let path = dir.path().join("probe.md");
		fs::write(
			&path,
			format!(
				"<svg xmlns=\"http://www.w3.org/2000/svg\"><image href=\"{}\"/></svg>",
				pipe.display()
			),
		)
		.unwrap();
		let (send, recv) = std::sync::mpsc::channel();
		let probed = path.clone();
		std::thread::spawn(move || {
			let _ = send.send(is_picture(&probed));
		});
		assert!(recv.recv_timeout(Duration::from_secs(5)).unwrap());
	}

	#[test]
	fn hard_linked_paths_name_the_same_file() {
		let dir = tempfile::tempdir().unwrap();
		let first = dir.path().join("source.png");
		let second = dir.path().join("output.png");
		fs::write(&first, b"picture").unwrap();
		fs::hard_link(&first, &second).unwrap();
		assert!(same_file(&first, &second));
		fs::remove_file(&second).unwrap();
		assert!(!same_file(&first, &second));
		if markview_platform::test_support::special_files() {
			let pipe = dir.path().join("output.pipe");
			fifo(&pipe);
			assert!(!same_file(&first, &pipe));
		}
	}

	#[test]
	fn a_regular_document_still_reads() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("note.md");
		fs::write(&path, "\u{feff}# Title\n").unwrap();
		assert_eq!(read_document(&path).unwrap(), "# Title\n");
	}
}
