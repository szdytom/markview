//! Native touchpad viewport; application code owns gesture interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
	Building,
	Running,
	Inertia,
	Ready,
	Disabled,
}
#[derive(Default)]
pub struct Batch {
	pub statuses: Vec<Status>,
	pub transform: Option<(f32, f32)>,
	pub scale: f32,
}
#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
	use super::{Batch, Status};
	use std::cell::{Cell, RefCell};
	use std::rc::Rc;
	use std::time::{Duration, Instant};
	use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

	use windows::Win32::Foundation::{
		E_FAIL, HWND, LPARAM, LRESULT, RECT, WPARAM,
	};
	use windows::Win32::Graphics::DirectManipulation::{
		DIRECTMANIPULATION_BUILDING,
		DIRECTMANIPULATION_CONFIGURATION_INTERACTION,
		DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_INERTIA,
		DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_X,
		DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_Y,
		DIRECTMANIPULATION_DISABLED, DIRECTMANIPULATION_INERTIA,
		DIRECTMANIPULATION_READY, DIRECTMANIPULATION_RUNNING,
		DIRECTMANIPULATION_STATUS,
		DIRECTMANIPULATION_VIEWPORT_OPTIONS_DISABLEPIXELSNAPPING,
		DIRECTMANIPULATION_VIEWPORT_OPTIONS_MANUALUPDATE,
		IDirectManipulationContent, IDirectManipulationFrameInfoProvider,
		IDirectManipulationManager, IDirectManipulationUpdateManager,
		IDirectManipulationViewport, IDirectManipulationViewportEventHandler,
		IDirectManipulationViewportEventHandler_Impl,
	};
	use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
	use windows::Win32::UI::Input::Pointer::GetPointerType;
	use windows::Win32::UI::Shell::{
		DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass,
	};
	use windows::Win32::UI::WindowsAndMessaging::{
		DM_POINTERHITTEST, POINTER_INPUT_TYPE, PT_TOUCHPAD,
		WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERUP,
		WM_POINTERUPDATE,
	};
	use windows::core::{GUID, Ref, implement};

	/// The `DirectManipulationManager` class, the one CLSID the reader needs.
	const MANAGER: GUID =
		GUID::from_u128(0x54e211b6_3650_4f75_8334_fa359598e1c5);

	/// How long the pump keeps asking whether an offered contact took,
	/// before settling back to sleep.
	const CONTACT_WINDOW: Duration = Duration::from_millis(250);
	/// The pump's period while a gesture or an offered contact may still
	/// have news: one frame at the display's own rate.
	const PUMP_PERIOD: Duration = Duration::from_millis(16);

	/// The id the reader's pointer hook hangs under; hook ids are private to
	/// a window, and one window owns one viewport.
	const HOOK: usize = 1;

	thread_local! {
		/// The touchpad pointers the window's hook hit-tested, offered to
		/// the viewport by the next tick.
		static HIT_TESTS: RefCell<Vec<u32>> =
			const { RefCell::new(Vec::new()) };
	}

	/// The window's pointer hook. Only `DM_POINTERHITTEST` can begin a
	/// touchpad input sequence — Chromium's helper hangs off the same
	/// message — and winit never surfaces it, so the hook records the
	/// pointer for the next tick to offer the viewport. Every message still
	/// reaches the window's own procedure.
	unsafe extern "system" fn hook(
		hwnd: HWND,
		msg: u32,
		wparam: WPARAM,
		lparam: LPARAM,
		_id: usize,
		_data: usize,
	) -> LRESULT {
		if msg == DM_POINTERHITTEST {
			let id = (wparam.0 & 0xffff) as u32;
			HIT_TESTS.with(|hit_tests| hit_tests.borrow_mut().push(id));
			log::debug!("the window hit-tests pointer {id}");
		} else if matches!(
			msg,
			WM_POINTERDOWN
				| WM_POINTERUPDATE
				| WM_POINTERUP
				| WM_POINTERCAPTURECHANGED
		) {
			log::debug!("pointer message {msg:#06x} for {:#x}", wparam.0);
		}
		// SAFETY: the hook records and answers for nothing; the window's own
		// procedure sees every message exactly as without it.
		unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
	}

	/// What the viewport callbacks recorded during one `Update`.
	struct Inbox {
		statuses: RefCell<Vec<Status>>,
		transform: Cell<Option<(f32, f32)>>,
		/// Where the content last rested, kept across pumps: the pump
		/// drains `transform` before the release's batch, so the
		/// ready-reset reads this to learn whether it must park.
		content: Cell<(f32, f32)>,
		/// Display scale, physical transform to logical pixels.
		scale: Cell<f32>,
		/// True while the ready-reset's own updates arrive: the viewport
		/// repositioning itself, not the hand.
		resetting: Cell<bool>,
	}

	#[implement(IDirectManipulationViewportEventHandler)]
	struct Handler {
		inbox: Rc<Inbox>,
	}

	impl IDirectManipulationViewportEventHandler_Impl for Handler_Impl {
		fn OnViewportStatusChanged(
			&self,
			viewport: Ref<'_, IDirectManipulationViewport>,
			current: DIRECTMANIPULATION_STATUS,
			_previous: DIRECTMANIPULATION_STATUS,
		) -> windows::core::Result<()> {
			// The ready-reset's own cycle — its `Running` and its closing
			// `Ready` — is the viewport moving itself, not the hand, so
			// nothing it says is recorded; otherwise its content jump back
			// to the origin would fold as the gesture's last motion and
			// throw the page back by everything the hand had travelled. A
			// fresh gesture's `Building` closes the cycle too, so a hand
			// faster than the reset is never swallowed.
			if self.inbox.resetting.get() {
				log::debug!(
					"the reset moves the viewport ({:?}, unrecorded)",
					status(current)
				);
				if matches!(
					current,
					DIRECTMANIPULATION_READY | DIRECTMANIPULATION_BUILDING
				) {
					self.inbox.resetting.set(false);
					// The reset parks the content at the origin the next
					// gesture reads its travel from.
					self.inbox.content.set((0.0, 0.0));
				}
				return Ok(());
			}
			log::debug!("viewport status {:?}", status(current));
			self.inbox.statuses.borrow_mut().push(status(current));
			if current != DIRECTMANIPULATION_READY {
				return Ok(());
			}
			// Park the content back at its origin so the next gesture reads
			// its travel from there. The decision reads the content's own
			// record, which no pump drains, and the pending transform is
			// left alone: the last motion of the gesture reaches the seam
			// even when it shares this release's batch. The updates the
			// reset synthesizes are the viewport moving itself, so they are
			// marked and dropped until the cycle closes.
			if self.inbox.content.get() == (0.0, 0.0) {
				return Ok(());
			}
			self.inbox.resetting.set(true);
			if let Some(viewport) = viewport.as_ref() {
				// SAFETY: `GetViewportRect` reads the viewport's own rect and
				// `ZoomToRect` moves only that viewport; zooming it onto its
				// own rect is the identity it resets to.
				unsafe {
					if let Ok(rect) = viewport.GetViewportRect() {
						let _ = viewport.ZoomToRect(
							rect.left as f32,
							rect.top as f32,
							rect.right as f32,
							rect.bottom as f32,
							false,
						);
					}
				}
			}
			Ok(())
		}

		fn OnViewportUpdated(
			&self,
			_viewport: Ref<'_, IDirectManipulationViewport>,
		) -> windows::core::Result<()> {
			Ok(())
		}

		fn OnContentUpdated(
			&self,
			_viewport: Ref<'_, IDirectManipulationViewport>,
			content: Ref<'_, IDirectManipulationContent>,
		) -> windows::core::Result<()> {
			if self.inbox.resetting.get() {
				return Ok(());
			}
			let Some(content) = content.as_ref() else {
				return Ok(());
			};
			let mut matrix = [0.0f32; 6];
			// SAFETY: `GetContentTransform` writes six floats into `matrix`.
			unsafe {
				content.GetContentTransform(&mut matrix)?;
			};
			let position = (matrix[4], matrix[5]);
			self.inbox.transform.set(Some(position));
			self.inbox.content.set(position);
			Ok(())
		}
	}

	/// Maps a viewport status; `ENABLED` and `READY` are both idle.
	fn status(status: DIRECTMANIPULATION_STATUS) -> Status {
		if status == DIRECTMANIPULATION_BUILDING {
			Status::Building
		} else if status == DIRECTMANIPULATION_RUNNING {
			Status::Running
		} else if status == DIRECTMANIPULATION_INERTIA {
			Status::Inertia
		} else if status == DIRECTMANIPULATION_DISABLED {
			Status::Disabled
		} else {
			Status::Ready
		}
	}

	/// One window's Direct Manipulation viewport: a precision touchpad pans
	/// with the OS's own inertia, folded at the pixel-pan seam.
	pub struct DirectManipulation {
		manager: IDirectManipulationManager,
		updates: IDirectManipulationUpdateManager,
		viewport: IDirectManipulationViewport,
		inbox: Rc<Inbox>,
		/// When the last touchpad contact was offered; the pump stays awake
		/// for it even before the viewport reports a gesture.
		contact_at: Option<Instant>,
		/// True while the viewport has a gesture, contact or inertia in
		/// flight.
		live: bool,
		hwnd: HWND,
	}

	impl DirectManipulation {
		/// Opts `window` into precision-touchpad panning, or returns `None`
		/// to keep today's wheel handling; every failure is silent.
		pub fn new(window: &dyn winit::window::Window) -> Option<Self> {
			if std::env::var_os("MARKVIEW_NO_DM").is_some() {
				log::debug!(
					"touchpad panning stays on the wheel paths: \
						`MARKVIEW_NO_DM` is set"
				);
				return None;
			}
			let Ok(handle) = window.window_handle() else {
				return None;
			};
			let RawWindowHandle::Win32(handle) = handle.as_raw() else {
				return None;
			};
			let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
			let size = window.surface_size();
			let rect = RECT {
				left: 0,
				top: 0,
				right: size.width as i32,
				bottom: size.height as i32,
			};
			let inbox = Rc::new(Inbox {
				statuses: RefCell::new(Vec::new()),
				transform: Cell::new(None),
				content: Cell::new((0.0, 0.0)),
				scale: Cell::new(window.scale_factor() as f32),
				resetting: Cell::new(false),
			});
			// SAFETY: the calls below assemble the viewport on this thread,
			// which winit has already initialized as a COM apartment, and
			// hand it the window's own handle; a failure anywhere leaves the
			// created objects to release themselves on drop.
			let assembly = || -> windows::core::Result<Self> {
				unsafe {
					let manager: IDirectManipulationManager = CoCreateInstance(
						&MANAGER,
						None::<&windows::core::IUnknown>,
						CLSCTX_INPROC_SERVER,
					)?;
					let updates: IDirectManipulationUpdateManager =
						manager.GetUpdateManager()?;
					let viewport: IDirectManipulationViewport = manager
						.CreateViewport(
							None::<&IDirectManipulationFrameInfoProvider>,
							hwnd,
						)?;
					// Pan, and only pan: translation on both axes with the
					// OS inertia engine, so more interactions can join later
					// without the seam learning a new shape.
					viewport.ActivateConfiguration(
						DIRECTMANIPULATION_CONFIGURATION_INTERACTION
							| DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_X
							| DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_Y
							| DIRECTMANIPULATION_CONFIGURATION_TRANSLATION_INERTIA,
					)?;
					viewport.SetViewportOptions(
						DIRECTMANIPULATION_VIEWPORT_OPTIONS_MANUALUPDATE
							| DIRECTMANIPULATION_VIEWPORT_OPTIONS_DISABLEPIXELSNAPPING,
					)?;
					viewport.SetViewportRect(&rect)?;
					let handler: IDirectManipulationViewportEventHandler =
						Handler {
							inbox: inbox.clone(),
						}
						.into();
					viewport.AddEventHandler(Some(hwnd), &handler)?;
					manager.Activate(hwnd)?;
					viewport.Enable()?;
					// One initial pump services the fresh viewport and
					// confirms the whole assembly answers.
					updates.Update(
						None::<&IDirectManipulationFrameInfoProvider>,
					)?;
					// The hook is the input's doorway: without it no
					// touchpad gesture can reach the viewport, so an
					// assembly that cannot hang one is no assembly. It goes
					// up last, so a failure above it leaves nothing
					// recording.
					if !SetWindowSubclass(hwnd, Some(hook), HOOK, 0).as_bool() {
						return Err(windows::core::Error::from_hresult(E_FAIL));
					}
					Ok(Self {
						manager,
						updates,
						viewport,
						inbox,
						contact_at: None,
						live: false,
						hwnd,
					})
				}
			};
			match assembly() {
				Ok(owner) => {
					log::debug!("the viewport owns touchpad panning");
					Some(owner)
				}
				Err(error) => {
					log::debug!("direct manipulation unavailable: {error}");
					None
				}
			}
		}

		/// Offers a touchpad pointer to the viewport: from here the OS's own
		/// gesture recognition and inertia engine own the pan.
		pub fn contact(&mut self, pointer: u32) {
			let mut kind = POINTER_INPUT_TYPE::default();
			// SAFETY: `GetPointerType` writes one `POINTER_INPUT_TYPE` for a
			// live pointer id, and the hook recorded it within the pointer's
			// lifetime.
			if unsafe { GetPointerType(pointer, &mut kind) }.is_err()
				|| kind != PT_TOUCHPAD
			{
				return;
			}
			// SAFETY: `Enable` and `SetContact` configure the window's own
			// viewport on this thread; `Enable` repeats harmlessly on one
			// that was never disabled.
			if unsafe { self.viewport.Enable() }.is_err()
				|| unsafe { self.viewport.SetContact(pointer) }.is_err()
			{
				log::debug!("the viewport could not take pointer {pointer}");
				return;
			}
			self.contact_at = Some(Instant::now());
			log::debug!("the viewport holds pointer {pointer}");
		}

		/// The touchpad pointers the window's hook hit-tested since the last
		/// tick, whether or not a viewport waits for them.
		pub fn take_offered_pointers() -> Vec<u32> {
			HIT_TESTS
				.with(|hit_tests| std::mem::take(&mut *hit_tests.borrow_mut()))
		}

		/// Cancels the viewport's gesture: the OS stops all its transforms
		/// at once, and the stream's bookkeeping ends, so no stale delta
		/// speaks for a view that focus loss, a resize, a reload or a tab
		/// switch replaced. The next contact re-enables the viewport.
		pub fn abandon(&mut self) {
			self.live = false;
			self.contact_at = None;
			// What the callbacks recorded for the dead gesture must not
			// survive it: a later pump would fold it onto a fresh one.
			self.inbox.statuses.borrow_mut().clear();
			self.inbox.transform.set(None);
			self.inbox.content.set((0.0, 0.0));
			self.inbox.resetting.set(false);
			// SAFETY: `Disable` stops the window's own viewport on this
			// thread; `Enable` from the next contact resumes it.
			let _ = unsafe { self.viewport.Disable() };
		}

		/// Follows the window: the viewport rect is the client area, and the
		/// transform turns into logical pixels at the display's scale.
		pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
			self.inbox.scale.set(scale);
			// SAFETY: `SetViewportRect` reads one `RECT`.
			let _ = unsafe {
				self.viewport.SetViewportRect(&RECT {
					left: 0,
					top: 0,
					right: width as i32,
					bottom: height as i32,
				})
			};
		}

		/// Pump native updates and drain their statuses and physical transform.
		pub fn pump(&mut self) -> Batch {
			if !self.live
				&& self
					.contact_at
					.is_none_or(|at| at.elapsed() >= CONTACT_WINDOW)
			{
				self.contact_at = None;
				return Batch::default();
			}
			// SAFETY: `Update` services the viewport on this thread and
			// fires the handler synchronously into the inbox.
			let _ = unsafe {
				self.updates
					.Update(None::<&IDirectManipulationFrameInfoProvider>)
			};
			let statuses =
				std::mem::take(&mut *self.inbox.statuses.borrow_mut());
			let transform = self.inbox.transform.take();
			// The viewport's own statuses say whether news may still come,
			// whether the seam hears it or not.
			if let Some(last) = statuses.last() {
				self.live = matches!(
					last,
					Status::Building | Status::Running | Status::Inertia
				);
				if self.live {
					self.contact_at = None;
				}
			}
			Batch {
				statuses,
				transform,
				scale: self.inbox.scale.get(),
			}
		}

		/// The next pump, while a gesture or an offered contact may still
		/// have news.
		pub fn deadline(&self, now: Instant) -> Option<Instant> {
			(self.live || self.contact_at.is_some())
				.then_some(now + PUMP_PERIOD)
		}
	}

	impl Drop for DirectManipulation {
		fn drop(&mut self) {
			// SAFETY: the teardown removes the window's hook and stops and
			// releases the viewport and the window's registration before the
			// interfaces drop themselves.
			unsafe {
				let _ = RemoveWindowSubclass(self.hwnd, Some(hook), HOOK);
				let _ = self.viewport.Stop();
				let _ = self.viewport.Abandon();
				let _ = self.manager.Deactivate(self.hwnd);
			}
		}
	}
}

#[cfg(windows)]
pub use win::DirectManipulation;

#[cfg(not(windows))]
pub struct DirectManipulation;
#[cfg(not(windows))]
impl DirectManipulation {
	pub fn new(_: &dyn winit::window::Window) -> Option<Self> {
		None
	}
	pub fn contact(&mut self, _: u32) {}
	pub fn take_offered_pointers() -> Vec<u32> {
		Vec::new()
	}
	pub fn resize(&mut self, _: u32, _: u32, _: f32) {}
	pub fn abandon(&mut self) {}
	pub fn pump(&mut self) -> Batch {
		Batch::default()
	}
	pub fn deadline(
		&self,
		_: std::time::Instant,
	) -> Option<std::time::Instant> {
		None
	}
}
