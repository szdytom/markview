//! `winit` owns window styles and client sizing; this hook adds native hit tests.
#![allow(unsafe_code)]
use crate::{FrameMetrics, HitTest};
use anyhow::{Result, anyhow};
use std::{cell::Cell, ptr};
use windows_sys::Win32::{
	Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
	Graphics::Gdi::ScreenToClient,
	UI::{
		HiDpi::GetDpiForWindow,
		Input::KeyboardAndMouse::{
			ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT,
			TRACKMOUSEEVENT, TrackMouseEvent,
		},
		Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
		WindowsAndMessaging::*,
	},
};
use winit::{
	raw_window_handle::{HasWindowHandle, RawWindowHandle},
	window::{ResizeDirection, Window},
};

const SUBCLASS: usize = 0x4d56;

struct State {
	hit_test: Box<dyn Fn(FrameMetrics, f32, f32) -> HitTest>,
	fullscreen: Cell<bool>,
	hover: Cell<bool>,
	pressed: Cell<bool>,
	destroyed: Cell<bool>,
	redraw: Box<dyn Fn()>,
}

/// The stable allocation outlives the hook and drops before the `Window`.
pub struct NativeFrame {
	hwnd: HWND,
	state: Box<State>,
}
impl NativeFrame {
	pub fn new(
		window: &Window,
		hit_test: impl Fn(FrameMetrics, f32, f32) -> HitTest + 'static,
		redraw: impl Fn() + 'static,
	) -> Result<Option<Self>> {
		let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw()
		else {
			unreachable!();
		};
		let hwnd = handle.hwnd.get() as HWND;
		let state = Box::new(State {
			hit_test: Box::new(hit_test),
			fullscreen: Cell::new(false),
			hover: Cell::new(false),
			pressed: Cell::new(false),
			destroyed: Cell::new(false),
			redraw: Box::new(redraw),
		});
		let data = ptr::from_ref(state.as_ref()) as usize;
		// SAFETY: This is the window's event-loop thread. `state` stays at this
		// address until `Drop` removes the hook, before the window is dropped.
		if unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS, data) }
			== 0
		{
			return Err(anyhow!("Cannot install window caption hit testing"));
		}
		Ok(Some(Self { hwnd, state }))
	}
	pub fn set_fullscreen(&self, fullscreen: bool) {
		self.state.fullscreen.set(fullscreen);
	}
	pub fn feedback(&self) -> (bool, bool) {
		(self.state.hover.get(), self.state.pressed.get())
	}
	pub fn cancel(&self) {
		if self.state.pressed.get() {
			self.state.feedback(false, false);
			// SAFETY: Only our outstanding maximize press owns capture.
			unsafe {
				ReleaseCapture();
			}
		}
	}
}
impl Drop for NativeFrame {
	fn drop(&mut self) {
		if !self.state.destroyed.get() {
			// SAFETY: The guard and window live on the same thread. Removing
			// the subclass prevents further access to its allocation.
			unsafe {
				RemoveWindowSubclass(self.hwnd, Some(subclass), SUBCLASS);
			}
		}
	}
}
impl State {
	fn hit(&self, hwnd: HWND, x: f32, y: f32) -> HitTest {
		let mut rect = RECT::default();
		// SAFETY: `hwnd` is alive during the callback; these queries retain no pointers.
		let (scale, maximized) = unsafe {
			GetClientRect(hwnd, &mut rect);
			(
				GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
				IsZoomed(hwnd) != 0,
			)
		};
		(self.hit_test)(
			FrameMetrics {
				width: (rect.right - rect.left) as f32 / scale,
				height: (rect.bottom - rect.top) as f32 / scale,
				scale,
				fullscreen: self.fullscreen.get(),
				maximized,
			},
			x,
			y,
		)
	}
	fn point(&self, hwnd: HWND, mut point: POINT) -> (f32, f32) {
		// SAFETY: `point` is writable and `hwnd` is alive during its callback.
		let scale = unsafe {
			ScreenToClient(hwnd, &mut point);
			GetDpiForWindow(hwnd).max(96) as f32 / 96.0
		};
		(point.x as f32 / scale, point.y as f32 / scale)
	}
	fn over_expand(&self, hwnd: HWND) -> bool {
		let mut point = POINT::default();
		// SAFETY: `point` is writable; the query retains no pointer.
		unsafe {
			GetCursorPos(&mut point);
		}
		let (x, y) = self.point(hwnd, point);
		self.hit(hwnd, x, y) == HitTest::Maximize
	}
	fn feedback(&self, hover: bool, pressed: bool) {
		if self.hover.replace(hover) != hover || self.pressed.get() != pressed {
			self.pressed.set(pressed);
			(self.redraw)();
		}
	}
}

// SAFETY: Windows calls this only on the installing thread. The guard keeps
// `data` alive until this hook is removed; no reference to `App` crosses it.
#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep Win32 message dispatch, ABI return values and DefSubclassProc fallback together for callback lifetime and reentrancy auditing."
	)
)]
unsafe extern "system" fn subclass(
	hwnd: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
	_: usize,
	data: usize,
) -> LRESULT {
	// SAFETY: `data` is the stable `State` registered by `NativeFrame::new`.
	let state = unsafe { &*(data as *const State) };
	match message {
		WM_NCHITTEST => {
			let point = POINT {
				x: (lparam as u16 as i16) as i32,
				y: ((lparam >> 16) as u16 as i16) as i32,
			};
			let (x, y) = state.point(hwnd, point);
			return match state.hit(hwnd, x, y) {
				HitTest::Client => HTCLIENT,
				HitTest::Caption => HTCAPTION,
				HitTest::Maximize => HTMAXBUTTON,
				HitTest::Resize(edge) => match edge {
					ResizeDirection::North => HTTOP,
					ResizeDirection::South => HTBOTTOM,
					ResizeDirection::West => HTLEFT,
					ResizeDirection::East => HTRIGHT,
					ResizeDirection::NorthWest => HTTOPLEFT,
					ResizeDirection::NorthEast => HTTOPRIGHT,
					ResizeDirection::SouthWest => HTBOTTOMLEFT,
					ResizeDirection::SouthEast => HTBOTTOMRIGHT,
				},
			} as LRESULT;
		}
		WM_SETCURSOR if lparam as u16 as u32 == HTMAXBUTTON => {
			// SAFETY: The shared system cursor needs no cleanup; the callback
			// runs on the window thread and retains no pointers.
			unsafe {
				SetCursor(LoadCursorW(ptr::null_mut(), IDC_HAND));
			}
			return 1;
		}
		WM_NCMOUSEMOVE => {
			state.feedback(wparam == HTMAXBUTTON as usize, state.pressed.get());
			let mut tracking = TRACKMOUSEEVENT {
				cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
				dwFlags: TME_LEAVE | TME_NONCLIENT,
				hwndTrack: hwnd,
				dwHoverTime: 0,
			};
			// SAFETY: Windows reads this initialized structure during the call.
			unsafe {
				TrackMouseEvent(&mut tracking);
			}
			// Forwarding preserves the Windows 11 maximize-hover Snap menu.
		}
		WM_NCLBUTTONDOWN if wparam == HTMAXBUTTON as usize => {
			state.feedback(true, true);
			// SAFETY: The callback owns an alive window on this thread.
			unsafe {
				SetCapture(hwnd);
			}
			return 0;
		}
		WM_MOUSEMOVE if state.pressed.get() => {
			state.feedback(state.over_expand(hwnd), true);
			return 0;
		}
		WM_LBUTTONUP if state.pressed.get() => {
			let over = state.over_expand(hwnd);
			state.feedback(over, false);
			// SAFETY: Capture is ours, and posting a scalar system command
			// retains no Rust pointers. Native code owns the maximize action.
			unsafe {
				ReleaseCapture();
				if over {
					PostMessageW(
						hwnd,
						WM_SYSCOMMAND,
						if IsZoomed(hwnd) != 0 {
							SC_RESTORE
						} else {
							SC_MAXIMIZE
						} as usize,
						0,
					);
				}
			}
			return 0;
		}
		WM_NCMOUSELEAVE => state.feedback(false, state.pressed.get()),
		WM_CANCELMODE => {
			let pressed = state.pressed.get();
			state.feedback(false, false);
			if pressed {
				// SAFETY: Only our outstanding caption press owns capture.
				unsafe {
					ReleaseCapture();
				}
			}
		}
		WM_CAPTURECHANGED if lparam != hwnd as LPARAM => {
			state.feedback(false, false)
		}
		WM_NCDESTROY => state.destroyed.set(true),
		_ => {}
	}
	// SAFETY: Forward the original callback arguments to the next hook;
	// `winit` continues to own sizing, activation and all unrelated input.
	unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
	use super::*;
	use winit::{
		event_loop::EventLoop, platform::windows::EventLoopBuilderExtWindows,
	};
	#[test]
	#[allow(deprecated)]
	fn native_hit_test_uses_the_application_callback() {
		let event_loop =
			EventLoop::builder().with_any_thread(true).build().unwrap();
		let window = event_loop
			.create_window(
				Window::default_attributes()
					.with_visible(false)
					.with_decorations(false),
			)
			.unwrap();
		let native = NativeFrame::new(
			&window,
			|metrics, _, _| {
				assert!(
					metrics.width > 0.0
						&& metrics.height > 0.0
						&& metrics.scale > 0.0
				);
				if metrics.fullscreen {
					HitTest::Client
				} else {
					HitTest::Maximize
				}
			},
			|| {},
		)
		.unwrap()
		.unwrap();
		// SAFETY: The hidden window and its hook live on this thread during these messages.
		unsafe {
			assert_eq!(
				SendMessageW(native.hwnd, WM_NCHITTEST, 0, 0),
				HTMAXBUTTON as isize
			);
			assert_eq!(
				SendMessageW(
					native.hwnd,
					WM_SETCURSOR,
					native.hwnd as usize,
					((WM_MOUSEMOVE << 16) | HTMAXBUTTON) as LPARAM
				),
				1
			);
			assert_eq!(GetCursor(), LoadCursorW(ptr::null_mut(), IDC_HAND));
			native.set_fullscreen(true);
			assert_eq!(
				SendMessageW(native.hwnd, WM_NCHITTEST, 0, 0),
				HTCLIENT as isize
			);
		}
	}
}
