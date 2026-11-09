//! Capabilities and desktop conventions queried by the reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowButtons {
	Windows,
	Macos,
	Linux,
}
pub const fn window_buttons() -> WindowButtons {
	if cfg!(target_os = "macos") {
		WindowButtons::Macos
	} else if cfg!(target_os = "linux") {
		WindowButtons::Linux
	} else {
		WindowButtons::Windows
	}
}
pub const fn command_key() -> bool {
	cfg!(target_os = "macos")
}
pub const fn touch_layout() -> bool {
	cfg!(target_os = "android")
}
pub const fn touch_inset() -> bool {
	cfg!(any(target_os = "android", target_os = "ios"))
}
/// A native window connection cannot be shared by GPU backends.
pub const fn exclusive_gpu_surface() -> bool {
	cfg!(target_os = "android")
}
pub const fn folder_reveal() -> bool {
	!cfg!(target_os = "android")
}
pub const fn desktop_cli() -> bool {
	!cfg!(target_os = "android")
}
pub const fn client_resize() -> bool {
	cfg!(target_os = "linux")
}
pub const fn native_buttons() -> bool {
	cfg!(target_os = "macos")
}
pub const fn native_scroll_inertia() -> bool {
	cfg!(target_os = "macos")
}
pub const fn wheel_packets() -> bool {
	cfg!(windows)
}
pub const fn portal_theme() -> bool {
	cfg!(target_os = "linux")
}
pub const fn mobile_session() -> bool {
	cfg!(target_os = "android")
}
pub fn ua_os_comment() -> &'static str {
	if cfg!(target_os = "windows") {
		"Windows NT 10.0; Win64; x64"
	} else if cfg!(target_os = "macos") {
		"Macintosh; Intel Mac OS X 10_15_7"
	} else if cfg!(target_os = "android") {
		"Linux; Android 10"
	} else if cfg!(target_arch = "aarch64") {
		"X11; Linux aarch64"
	} else {
		"X11; Linux x86_64"
	}
}
