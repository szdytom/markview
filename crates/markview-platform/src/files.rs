//! Native file access and permissions; callers validate contents and own transactions.
use std::{fs, io, path::Path};

/// Create temporary storage writable by the application on this platform.
#[cfg_attr(
	any(),
	markview::allow_raw_paths(
		reason = "Production temporary storage follows the platform writable-directory policy"
	)
)]
pub fn tempdir() -> io::Result<tempfile::TempDir> {
	#[cfg(target_os = "android")]
	{
		tempfile::tempdir_in(
			crate::android::data_path().expect("Android data directory"),
		)
	}
	#[cfg(not(target_os = "android"))]
	{
		tempfile::tempdir()
	}
}

/// The native display name of a folder.
pub fn folder_name(root: &Path) -> String {
	#[cfg(target_os = "android")]
	if let Ok(name) = fs::read_to_string(root.join(".markview-folder-name")) {
		return name;
	}
	root.file_name()
		.unwrap_or(root.as_os_str())
		.to_string_lossy()
		.into_owned()
}

#[cfg(unix)]
pub fn open_nonblocking(path: &Path) -> io::Result<fs::File> {
	use std::os::unix::fs::OpenOptionsExt;
	// `O_NONBLOCK` has no effect on a regular file; it only decides whether the
	// open waits. Standard Rust opens already set `O_CLOEXEC`, so no descriptor
	// escapes into a child.
	fs::OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_NONBLOCK)
		.open(path)
}

#[cfg(not(unix))]
pub fn open_nonblocking(path: &Path) -> io::Result<fs::File> {
	// Windows has no portable non-blocking open, so opening a device or a pipe
	// can still wait there; the type check still refuses it once it returns.
	fs::File::open(path)
}

/// Metadata identity and content times for a cached file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileVersion {
	pub size: u64,
	modified: Option<std::time::SystemTime>,
	created: Option<std::time::SystemTime>,
	#[cfg(unix)]
	unix: (u64, u64, i64, i64),
}

pub fn file_version(path: &Path) -> Option<FileVersion> {
	#[cfg(unix)]
	use std::os::unix::fs::MetadataExt;
	let metadata = fs::metadata(path).ok()?;
	Some(FileVersion {
		size: metadata.len(),
		modified: metadata.modified().ok(),
		created: metadata.created().ok(),
		#[cfg(unix)]
		unix: (
			metadata.dev(),
			metadata.ino(),
			metadata.ctime(),
			metadata.ctime_nsec(),
		),
	})
}

/// Whether two paths name the same file, including through symlinks and hard
/// links. Metadata identity keeps this from opening a FIFO or device.
pub fn same_file(first: &Path, second: &Path) -> bool {
	if first == second {
		return true;
	}
	#[cfg(unix)]
	{
		use std::os::unix::fs::MetadataExt;
		let (Ok(first), Ok(second)) =
			(fs::metadata(first), fs::metadata(second))
		else {
			return false;
		};
		first.dev() == second.dev() && first.ino() == second.ino()
	}
	#[cfg(windows)]
	{
		::same_file::is_same_file(first, second).unwrap_or(false)
	}
	#[cfg(not(any(unix, windows)))]
	{
		false
	}
}

pub fn private_file_options(options: &mut fs::OpenOptions) {
	#[cfg(unix)]
	{
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	#[cfg(not(unix))]
	let _ = options;
}

pub fn export_tempfile(parent: &Path) -> io::Result<tempfile::NamedTempFile> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		tempfile::Builder::new()
			.permissions(fs::Permissions::from_mode(0o666))
			.tempfile_in(parent)
	}
	#[cfg(not(unix))]
	{
		tempfile::NamedTempFile::new_in(parent)
	}
}

/// The application data directory, fixed for the lifetime of the process.
pub fn config_directory() -> Option<std::path::PathBuf> {
	static DIRECTORY: std::sync::OnceLock<Option<std::path::PathBuf>> =
		std::sync::OnceLock::new();
	DIRECTORY
		.get_or_init(|| {
			#[cfg(any(target_os = "windows", target_os = "linux"))]
			if let Ok(executable) = std::env::current_exe()
				&& let Some(directory) = portable_directory(&executable)
			{
				return Some(directory);
			}
			config_base().map(|base| base.join("markview"))
		})
		.clone()
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn portable_directory(executable: &Path) -> Option<std::path::PathBuf> {
	let directory = executable.parent()?;
	directory
		.join("markview-portable-marker.txt")
		.exists()
		.then(|| directory.join("data"))
}

pub fn config_base() -> Option<std::path::PathBuf> {
	#[cfg(not(target_os = "android"))]
	use std::path::PathBuf;
	#[cfg(target_os = "android")]
	{
		crate::android::data_path()
	}
	#[cfg(target_os = "windows")]
	{
		std::env::var_os("APPDATA").map(PathBuf::from)
	}
	#[cfg(target_os = "macos")]
	{
		std::env::var_os("HOME")
			.map(|p| PathBuf::from(p).join("Library/Application Support"))
	}
	#[cfg(not(any(
		target_os = "windows",
		target_os = "macos",
		target_os = "android"
	)))]
	{
		std::env::var_os("XDG_CONFIG_HOME")
			.map(PathBuf::from)
			.filter(|p| p.is_absolute())
			.or_else(|| {
				std::env::var_os("HOME")
					.map(|p| PathBuf::from(p).join(".config"))
			})
	}
}
pub fn system_locale() -> Option<String> {
	sys_locale::get_locale()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(any(target_os = "windows", target_os = "linux"))]
	#[test]
	fn portable_storage_depends_only_on_marker_presence() {
		let directory = markview_test_support::TestDir::new();
		let root = directory.path().join("portable 中文 reader");
		fs::create_dir(&root).unwrap();
		let executable = root.join("markview");
		let marker = root.join("markview-portable-marker.txt");
		assert_eq!(portable_directory(&executable), None);
		for contents in [b"".as_slice(), b"enabled", &[0xff, 0x00]] {
			fs::write(&marker, contents).unwrap();
			assert_eq!(
				portable_directory(&executable),
				Some(root.join("data"))
			);
			assert!(!root.join("data").exists());
		}
		fs::remove_file(marker).unwrap();
		assert_eq!(portable_directory(&executable), None);
	}

	#[test]
	fn folder_display_name_follows_native_conventions() {
		let dir = markview_test_support::TestDir::new();
		let root = dir.path().join("local-folder");
		fs::create_dir(&root).unwrap();
		assert_eq!(folder_name(&root), "local-folder");
		fs::write(root.join(".markview-folder-name"), "Imported folder")
			.unwrap();
		#[cfg(target_os = "android")]
		assert_eq!(folder_name(&root), "Imported folder");
		#[cfg(not(target_os = "android"))]
		assert_eq!(folder_name(&root), "local-folder");
		let root = root.ancestors().last().unwrap();
		assert_eq!(folder_name(root), root.as_os_str().to_string_lossy());
	}
}
