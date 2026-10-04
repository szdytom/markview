//! OS effects stay outside the reading core.

#[cfg(target_os = "android")]
pub(crate) mod android;

#[cfg(windows)]
pub(crate) mod window_frame;

#[cfg(target_os = "linux")]
pub(crate) mod touch_frame;

/// Move native traffic lights without replacing their system behavior.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn align_traffic_lights(window: &winit::window::Window) {
	use objc2_app_kit::{NSView, NSWindowButton};
	use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
	let RawWindowHandle::AppKit(handle) =
		window.window_handle().unwrap().as_raw()
	else {
		unreachable!();
	};
	// SAFETY: `winit` supplies its live `NSView`; this call runs on the
	// event-loop's main thread, and `window` keeps the view alive.
	let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
	let Some(native) = view.window() else {
		return;
	};
	for kind in [
		NSWindowButton::CloseButton,
		NSWindowButton::MiniaturizeButton,
		NSWindowButton::ZoomButton,
	] {
		let Some(button) = native.standardWindowButton(kind) else {
			continue;
		};
		// SAFETY: The button and its parent are AppKit views accessed only
		// on the main thread; the returned parent is retained for this call.
		let Some(parent) = (unsafe { button.superview() }) else {
			continue;
		};
		let mut rect = parent.convertRect_toView(button.frame(), Some(view));
		let (top, bottom) = crate::app::frame::traffic_light_margins(
			rect.size.height,
			window.scale_factor(),
		);
		let y = if view.isFlipped() {
			top
		} else {
			view.bounds().size.height - top - rect.size.height
		};
		if (rect.origin.y - y).abs() > 0.01 {
			rect.origin.y = y;
			button.setFrameOrigin(
				parent.convertRect_fromView(rect, Some(view)).origin,
			);
			log::debug!(
				"Traffic light {} margins: top {top:.2}, bottom {bottom:.2} logical px",
				kind.0
			);
		}
	}
}

/// Honor the desktop's titlebar double-click preference.
#[cfg(target_os = "macos")]
pub(crate) fn titlebar_double_click(window: &winit::window::Window) {
	use objc2_foundation::{NSUserDefaults, ns_string};
	let action = NSUserDefaults::standardUserDefaults()
		.stringForKey(ns_string!("AppleActionOnDoubleClick"))
		.map(|value| value.to_string());
	match action.as_deref() {
		Some("Minimize") => window.set_minimized(true),
		Some("None") => {}
		_ => window.set_maximized(!window.is_maximized()),
	}
}

/// The `SystemParametersInfoW` action selector, under a shorter name.
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION as SystemParametersInfoAction;

/// Attaches the process to the console it was launched from.
///
/// A release build runs in the Windows subsystem, so the standard handles are
/// inherited only from launchers that pass them on. Attaching gives the
/// diagnostic entry points a console to report on; an Explorer launch has no
/// parent console to borrow, and its output is dropped.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn attach_parent_console() {
	// SAFETY: `AttachConsole` reads one process id and attaches this process
	// to that console; failing leaves the process unchanged.
	unsafe {
		let _ = windows_sys::Win32::System::Console::AttachConsole(
			windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
		);
	}
}

/// Other platforms keep the console they were launched from.
#[cfg(not(windows))]
pub(crate) fn attach_parent_console() {}

/// What one wheel notch travels on each axis, as the desktop configures it.
///
/// Windows stores a separate choice per axis. macOS bakes its speed into the
/// deltas and reports the resulting lines. X11 and Wayland report one unit per
/// detent with no user setting behind it, and scale only the continuous axes a
/// touchpad produces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WheelNotch {
	pub vertical: WheelAmount,
	pub horizontal: WheelAmount,
}

/// How far one wheel notch travels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum WheelAmount {
	/// A whole number of text lines, or of characters across when sideways.
	Lines(f32),
	/// One screenful along the axis the notch moves.
	#[cfg_attr(not(windows), allow(dead_code))]
	Page,
}

/// The lines one notch scrolls where the platform reports none.
#[cfg_attr(target_os = "macos", allow(dead_code))]
const DEFAULT_WHEEL_LINES: f32 = 3.0;

/// Reads the desktop's wheel-notch choice for each axis.
#[cfg(windows)]
pub(crate) fn wheel_notch() -> WheelNotch {
	use windows_sys::Win32::UI::WindowsAndMessaging::{
		SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES,
	};
	WheelNotch {
		vertical: axis(SPI_GETWHEELSCROLLLINES),
		horizontal: axis(SPI_GETWHEELSCROLLCHARS),
	}
}

/// Reads one axis's per-notch value, or the shipped default.
#[cfg(windows)]
#[allow(unsafe_code)]
fn axis(action: SystemParametersInfoAction) -> WheelAmount {
	use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
	let mut value: u32 = 0;
	// SAFETY: `SystemParametersInfoW` writes one `u32` into `value` for both
	// of these actions and reads no other memory.
	let ok = unsafe {
		SystemParametersInfoW(
			action,
			0,
			std::ptr::from_mut(&mut value).cast(),
			0,
		)
	};
	// A failed call and "no scrolling" both read as the default: freezing the
	// wheel silently is worse than ignoring an option nobody sets.
	if ok == 0 || value == 0 {
		return WheelAmount::Lines(DEFAULT_WHEEL_LINES);
	}
	match value {
		u32::MAX => WheelAmount::Page,
		n => WheelAmount::Lines(n as f32),
	}
}

/// macOS reports the lines the desktop's own speed produced, so one is enough.
#[cfg(target_os = "macos")]
pub(crate) fn wheel_notch() -> WheelNotch {
	let lines = WheelAmount::Lines(1.0);
	WheelNotch {
		vertical: lines,
		horizontal: lines,
	}
}

/// X11 buttons 4 and 5 and Wayland's discrete axis both report one unit per
/// detent, and neither desktop publishes a lines-per-notch value, so a detent
/// takes the conventional three; Windows ships the same default.
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn wheel_notch() -> WheelNotch {
	let lines = WheelAmount::Lines(DEFAULT_WHEEL_LINES);
	WheelNotch {
		vertical: lines,
		horizontal: lines,
	}
}

#[derive(Default)]
pub struct Clipboard {
	#[cfg(not(target_os = "android"))]
	inner: Option<arboard::Clipboard>,
}
impl Clipboard {
	#[cfg(not(target_os = "android"))]
	pub fn read(&mut self) -> anyhow::Result<String> {
		if self.inner.is_none() {
			self.inner = Some(arboard::Clipboard::new()?);
		}
		Ok(self.inner.as_mut().unwrap().get_text()?)
	}

	#[cfg(not(target_os = "android"))]
	pub fn write(&mut self, text: String) -> anyhow::Result<()> {
		if self.inner.is_none() {
			self.inner = Some(arboard::Clipboard::new()?);
		}
		self.inner.as_mut().unwrap().set_text(text)?;
		Ok(())
	}
	#[cfg(target_os = "android")]
	pub fn read(&mut self) -> anyhow::Result<String> {
		android::clipboard_read()
	}
	#[cfg(target_os = "android")]
	pub fn write(&mut self, text: String) -> anyhow::Result<()> {
		android::call_string("copyText", &text)
	}
}

pub(crate) fn pick_document(
	done: impl FnOnce(Option<std::path::PathBuf>) + Send + 'static,
) {
	#[cfg(not(target_os = "android"))]
	std::thread::spawn(move || {
		done(
			rfd::FileDialog::new()
				.add_filter("Markdown", &["md", "markdown", "mdown", "txt"])
				.pick_file(),
		);
	});
	#[cfg(target_os = "android")]
	android::pick_document(done);
}

pub(crate) fn open_external(
	target: impl AsRef<std::ffi::OsStr>,
) -> anyhow::Result<()> {
	#[cfg(not(target_os = "android"))]
	open::that_detached(target)?;
	#[cfg(target_os = "android")]
	android::call_string("openExternal", &target.as_ref().to_string_lossy())?;
	Ok(())
}

#[cfg(all(test, not(any(windows, target_os = "macos"))))]
mod tests {
	#[cfg(not(any(windows, target_os = "macos")))]
	use super::*;

	/// X11 and Wayland report a detent as one unit with no desktop value
	/// behind it, so it takes the three lines Windows ships with.
	#[cfg(not(any(windows, target_os = "macos")))]
	#[test]
	fn a_detent_without_a_desktop_value_is_three_lines() {
		let lines = WheelAmount::Lines(DEFAULT_WHEEL_LINES);
		assert_eq!(
			wheel_notch(),
			WheelNotch {
				vertical: lines,
				horizontal: lines
			}
		);
	}
}
