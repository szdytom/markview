//! Native file access and permissions; callers validate contents and own transactions.
use std::{fs, io, path::Path};

/// Create temporary storage writable by the application on this platform.
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

/// The native display name of a folder, including Android's imported tree name.
pub fn folder_name(root: &Path) -> String {
	#[cfg(not(target_os = "android"))]
	{
		root.file_name()
			.unwrap_or(root.as_os_str())
			.to_string_lossy()
			.into_owned()
	}
	#[cfg(target_os = "android")]
	{
		fs::read_to_string(root.join(".markview-folder-name"))
			.unwrap_or_default()
	}
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

	#[test]
	fn folder_display_name_follows_native_conventions() {
		let dir = tempfile::tempdir().unwrap();
		let root = dir.path().join("local-folder");
		fs::create_dir(&root).unwrap();
		fs::write(root.join(".markview-folder-name"), "Imported folder")
			.unwrap();
		#[cfg(not(target_os = "android"))]
		{
			assert_eq!(folder_name(&root), "local-folder");
			let root = root.ancestors().last().unwrap();
			assert_eq!(folder_name(root), root.as_os_str().to_string_lossy());
		}
		#[cfg(target_os = "android")]
		{
			assert_eq!(folder_name(&root), "Imported folder");
			fs::remove_file(root.join(".markview-folder-name")).unwrap();
			assert_eq!(folder_name(&root), "");
		}
	}
}
