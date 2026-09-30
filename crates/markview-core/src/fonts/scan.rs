//! Reading faces from a directory on a host that has one.
use super::is_font_file;
use parley::fontique::{Blob, Collection, FamilyId, FontInfo};
use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

/// Registers every font file directly inside `directory`, returning the
/// families added so the caller can recognize a CJK face.
pub(super) fn register(
	collection: &mut Collection,
	directory: &Path,
) -> Vec<(FamilyId, Vec<FontInfo>)> {
	let entries = match std::fs::read_dir(directory) {
		Ok(entries) => entries,
		Err(error) => {
			log::warn!(
				"Font: cannot read directory {}: {error}",
				directory.display()
			);
			return Vec::new();
		}
	};
	let mut paths: Vec<PathBuf> =
		entries.flatten().map(|entry| entry.path()).collect();
	// Sorting keeps the registration order, and so the fallback order, the
	// same on every filesystem.
	paths.sort();
	let mut added = Vec::new();
	for path in paths {
		if !is_font_file(&path) {
			continue;
		}
		match map_font(&path) {
			Ok(Some(bytes)) => {
				added.extend(
					collection.register_fonts(Blob::new(Arc::new(bytes)), None),
				);
			}
			Ok(None) => {}
			Err(error) => {
				log::warn!("Font: cannot read {}: {error}", path.display())
			}
		}
	}
	added
}

/// Maps a font file instead of copying it into memory.
///
/// The collection owns the mapping and the shaper pages in only the tables the
/// document touches, so a directory of downloaded faces costs what is drawn
/// rather than the whole directory. An empty file holds no face, so it is
/// skipped instead of reported.
#[allow(unsafe_code)]
fn map_font(path: &Path) -> std::io::Result<Option<memmap2::Mmap>> {
	let file = std::fs::File::open(path)?;
	if file.metadata()?.len() == 0 {
		return Ok(None);
	}
	// SAFETY: the mapping is read-only, and a face is installed under its
	// final name by renaming a fresh file into place, so the bytes behind a
	// mapping are never rewritten or shortened in place.
	unsafe { memmap2::Mmap::map(&file) }.map(Some)
}
#[cfg(test)]
mod tests {
	use super::*;
	use crate::fonts::{FontConfig, context};
	use parley::fontique::Script;

	#[test]
	fn a_directory_supplies_families_and_a_cjk_fallback() {
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts"),
			],
			..Default::default()
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_some());
		// No system fonts are loaded, so Han text only reaches a face through
		// the fallback the directory supplied.
		let fallback: Vec<_> = context
			.collection
			.fallback_families(Script::from_bytes(*b"Hani"))
			.collect();
		assert!(!fallback.is_empty());
	}
	/// An empty file in a font directory holds no face, and one beside a real
	/// face must not cost it.
	#[test]
	fn an_empty_font_file_is_skipped_beside_a_real_one() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join("empty.ttf"), b"").unwrap();
		std::fs::copy(
			Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("tests/fonts/NotoSerif-Regular-subset.otf"),
			dir.path().join("face.otf"),
		)
		.unwrap();
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir.path().to_owned()],
			..Default::default()
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_some());
	}
	#[test]
	fn an_unknown_directory_is_ignored_rather_than_fatal() {
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![Path::new("does-not-exist").to_owned()],
			..Default::default()
		};
		let mut context = context(&config);
		assert!(context.collection.family_by_name("Noto Serif").is_none());
	}
	/// A later download adds files to a directory already in the
	/// configuration, so the revision has to make the same paths read as a
	/// new collection.
	#[test]
	fn a_download_into_a_registered_directory_changes_the_collection() {
		let dir = tempfile::tempdir().unwrap();
		let download = |name: &str| {
			std::fs::copy(
				Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts")
					.join(name),
				dir.path().join(name),
			)
			.unwrap();
		};
		// The directory is already registered and holds one face.
		download("NotoSerif-Regular-subset.otf");
		let config = FontConfig {
			ignore_system_fonts: true,
			directories: vec![dir.path().to_owned()],
			..Default::default()
		};
		let mut before = context(&config);
		assert!(before.collection.family_by_name("Noto Serif").is_some());
		assert!(before.collection.family_by_name("Noto Sans").is_none());
		// A second download lands another family without changing the paths.
		download("NotoSans-Regular-subset.otf");
		let mut downloaded = config.clone();
		downloaded.revision += 1;
		let mut after = context(&downloaded);
		assert!(after.collection.family_by_name("Noto Sans").is_some());
		// The collection the old configuration built before the download is
		// untouched. (Assert on `before` rather than re-asking the global
		// cache: a parallel test can evict the cached slot in between.)
		assert!(before.collection.family_by_name("Noto Sans").is_none());
	}
}
