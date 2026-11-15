//! Canonical real-file storage shared by tests and fuzz harnesses.
use std::path::{Path, PathBuf};

/// Real test storage with the same canonical root used by local document state.
pub struct TestDir {
	_directory: tempfile::TempDir,
	root: PathBuf,
}

impl TestDir {
	#[cfg_attr(
		any(),
		markview::allow_raw_paths(
			reason = "Own temporary storage before canonicalizing its public root"
		)
	)]
	pub fn new() -> Self {
		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().canonicalize().unwrap();
		Self {
			_directory: directory,
			root,
		}
	}

	pub fn path(&self) -> &Path {
		&self.root
	}
}

impl Default for TestDir {
	fn default() -> Self {
		Self::new()
	}
}
