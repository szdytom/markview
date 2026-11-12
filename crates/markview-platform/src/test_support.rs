//! Native fixtures shared by application integration tests.
use std::{ffi::OsString, fs, io, path::Path};

/// Whether a subprocess can isolate Linux font discovery with Fontconfig.
pub const fn isolated_fontconfig() -> bool {
	cfg!(target_os = "linux")
}
pub const fn special_files() -> bool {
	cfg!(unix)
}
pub const fn symlinks() -> bool {
	cfg!(unix)
}
pub const fn desktop_goldens() -> bool {
	cfg!(target_os = "linux")
}
pub const fn shell_launch() -> bool {
	cfg!(target_os = "linux")
}
pub fn symlink(
	source: impl AsRef<Path>,
	destination: impl AsRef<Path>,
) -> io::Result<()> {
	#[cfg(unix)]
	{
		std::os::unix::fs::symlink(source, destination)
	}
	#[cfg(not(unix))]
	{
		let _ = (source, destination);
		Ok(())
	}
}
pub fn permission_mode(permissions: fs::Permissions) -> u32 {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		permissions.mode() & 0o777
	}
	#[cfg(not(unix))]
	{
		let _ = permissions;
		0
	}
}
pub fn set_mode(path: impl AsRef<Path>, mode: u32) -> io::Result<()> {
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		fs::set_permissions(path, fs::Permissions::from_mode(mode))
	}
	#[cfg(not(unix))]
	{
		let _ = (path, mode);
		Ok(())
	}
}
/// A file name that has no Unicode representation.
pub fn non_unicode_name() -> OsString {
	#[cfg(unix)]
	{
		use std::os::unix::ffi::OsStringExt;
		OsString::from_vec(b"pic\xff.png".to_vec())
	}
	#[cfg(windows)]
	{
		use std::os::windows::ffi::OsStringExt;
		let mut name: Vec<u16> = "pic".encode_utf16().collect();
		name.push(0xd800);
		name.extend(".png".encode_utf16());
		OsString::from_wide(&name)
	}
	#[cfg(not(any(unix, windows)))]
	{
		OsString::from("pic\u{fffd}.png")
	}
}
pub fn raw_os_string(bytes: Vec<u8>) -> OsString {
	#[cfg(unix)]
	{
		use std::os::unix::ffi::OsStringExt;
		OsString::from_vec(bytes)
	}
	#[cfg(not(unix))]
	{
		OsString::from(String::from_utf8_lossy(&bytes).as_ref())
	}
}
pub fn fifo(path: impl AsRef<Path>) {
	#[cfg(unix)]
	{
		let status = std::process::Command::new("mkfifo")
			.arg(path.as_ref())
			.status()
			.expect("run mkfifo");
		assert!(
			status.success(),
			"mkfifo {} failed",
			path.as_ref().display()
		);
	}
	#[cfg(not(unix))]
	let _ = path;
}

/// Build a native test loop on a worker thread where the backend permits it.
pub fn input_event_loop()
-> Result<winit::event_loop::EventLoop, winit::error::EventLoopError> {
	let mut builder = winit::event_loop::EventLoop::builder();
	#[cfg(target_os = "linux")]
	{
		use winit::platform::x11::EventLoopBuilderExtX11;
		builder.with_x11().with_any_thread(true);
	}
	builder.build()
}

#[cfg(windows)]
#[path = "../tests/support/mod.rs"]
mod native_window;
#[cfg(windows)]
pub use native_window::create_window;
