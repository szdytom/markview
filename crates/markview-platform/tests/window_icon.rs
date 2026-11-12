//! The window must advertise the icon the shell paints a taskbar button from.
//!
//! `winit` allows one event loop per process, so this lives in an integration
//! test: the unit-test binary already owns an event loop for its window.
#![cfg(windows)]
#![allow(unsafe_code, deprecated)]

mod support;

use windows_sys::Win32::UI::WindowsAndMessaging::{
	ICON_BIG, ICON_SMALL, SendMessageW, WM_GETICON,
};
use winit::{
	event_loop::EventLoop,
	platform::windows::EventLoopBuilderExtWindows,
	raw_window_handle::{HasWindowHandle, RawWindowHandle},
};

/// Windows paints a taskbar button from `ICON_BIG`; a window that advertises
/// only `ICON_SMALL` leaves the button blank until the shell re-reads it.
#[test]
fn the_window_advertises_both_icon_sizes() {
	let mut event_loop =
		EventLoop::builder().with_any_thread(true).build().unwrap();
	let attributes = markview_platform::window_attributes(
		winit::window::WindowAttributes::default().with_visible(false),
		false,
	);
	let window = support::create_window(&mut event_loop, attributes);
	let RawWindowHandle::Win32(handle) =
		window.window_handle().unwrap().as_raw()
	else {
		unreachable!();
	};
	let hwnd = handle.hwnd.get() as _;
	// SAFETY: `winit` owns a live `HWND` on this thread for both messages.
	let (small, big) = unsafe {
		(
			SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0),
			SendMessageW(hwnd, WM_GETICON, ICON_BIG as usize, 0),
		)
	};
	assert_ne!(small, 0, "the window advertises no small icon");
	assert_ne!(big, 0, "the taskbar button has no icon to paint");
}
