//! Windows Direct Manipulation panning.
//!
//! The pump policy — folding viewport statuses and content transforms into
//! pixel-pan phases and deltas — is platform-neutral so it tests everywhere;
//! the COM assembly that feeds it lives in [`win`] below.

// Only the other-desktop stub's signatures name it; the COM assembly
// imports its own.
#[cfg(not(windows))]
use std::time::Instant;
use winit::event::TouchPhase;

use markview_selection::{PanFeed, PanPhase, PanStatus};

/// Folds one pump's worth of viewport news into seam events, in logical
/// pixels.
///
/// The statuses of a batch come first, but a transform observed in the same
/// batch belongs to the gesture it closed: its delta is fed before the
/// release, so the last motion of a gesture is not orphaned after its
/// `Ended`.
///
/// While `pinned`, the OS inertia is coasting the content over a contact
/// that never left the pad: the deltas drop, but the transform still
/// anchors, so a re-grip pans by the hand's motion alone.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn fold(
	feed: &mut PanFeed,
	statuses: Vec<PanStatus>,
	transform: Option<(f32, f32)>,
	scale: f32,
	pinned: bool,
) -> Vec<(TouchPhase, f32, f32)> {
	let release = statuses
		.iter()
		.position(|s| {
			!matches!(
				s,
				PanStatus::Building | PanStatus::Running | PanStatus::Inertia
			)
		})
		.unwrap_or(statuses.len());
	let (opening, closing) = statuses.split_at(release);
	let mut events: Vec<_> = phases(feed, opening.iter().copied()).collect();
	if let Some((x, y)) = transform
		&& let Some((dx, dy)) = feed.transform(x / scale, y / scale)
	{
		if pinned {
			log::debug!("the pin drops a coast delta ({dx:.1},{dy:.1})");
		} else {
			events.push((TouchPhase::Moved, dx, dy));
		}
	}
	events.extend(phases(feed, closing.iter().copied()));
	events
}

/// Whether the page pins while the OS inertia engine coasts the content.
///
/// The engine can coast while a contact is still on the pad; such a coast is
/// not the hand's motion, so the page holds still under it. The pin, once
/// set, outlives the contact's lift — a dropped glide must not resurrect
/// when the fingers leave — and it ends when the hand re-grips (`Running`)
/// or the coast is spent (`Ready`).
#[cfg_attr(not(windows), allow(dead_code))]
fn pin(latched: bool, contact_held: bool, status: Option<PanStatus>) -> bool {
	match status {
		Some(PanStatus::Inertia) => latched || contact_held,
		Some(PanStatus::Running | PanStatus::Ready | PanStatus::Disabled) => {
			false
		}
		_ => latched,
	}
}

/// The phases of a status run, in order.
fn phases(
	feed: &mut PanFeed,
	statuses: impl Iterator<Item = PanStatus>,
) -> impl Iterator<Item = (TouchPhase, f32, f32)> {
	statuses
		.filter_map(|s| feed.status(s).map(touch))
		.map(|phase| (phase, 0.0, 0.0))
}

/// Ends the gesture a held stream was carrying, silencing it: a
/// pointer-driven interaction owns the input, the gesture in flight speaks
/// its `Cancelled` once, and its deltas drop — so nothing stale resumes
/// panning after the owner lets go.
#[cfg_attr(not(windows), allow(dead_code))]
fn hold(feed: &mut PanFeed) -> Vec<(TouchPhase, f32, f32)> {
	feed.abandon()
		.map(|phase| vec![(touch(phase), 0.0, 0.0)])
		.unwrap_or_default()
}

/// The pan lifecycle in the seam's terms; the two spell the same three
/// moments.
fn touch(phase: PanPhase) -> TouchPhase {
	match phase {
		PanPhase::Started => TouchPhase::Started,
		PanPhase::Ended => TouchPhase::Ended,
		PanPhase::Cancelled => TouchPhase::Cancelled,
	}
}

/// The COM assembly: manager, update manager and one viewport per window,
/// feeding the fold above through the callbacks' inbox.
#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
	use super::{fold, hold, pin};
	use std::cell::{Cell, RefCell};
	use std::rc::Rc;
	use std::time::{Duration, Instant};
	use winit::event::TouchPhase;
	use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

	use markview_selection::{PanFeed, PanStatus};
	use windows::Win32::Foundation::{HWND, RECT};
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
	use windows::Win32::UI::WindowsAndMessaging::{
		POINTER_INPUT_TYPE, PT_TOUCHPAD,
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

	/// What the viewport callbacks recorded during one `Update`.
	struct Inbox {
		statuses: RefCell<Vec<PanStatus>>,
		transform: Cell<Option<(f32, f32)>>,
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
			log::debug!("viewport status {:?}", status(current));
			self.inbox.statuses.borrow_mut().push(status(current));
			if current != DIRECTMANIPULATION_READY {
				self.inbox.resetting.set(false);
				return Ok(());
			}
			if self.inbox.resetting.get() {
				self.inbox.resetting.set(false);
				return Ok(());
			}
			// Park the content back at its origin so the next gesture starts
			// from identity. The updates this synthesizes are the viewport
			// moving itself, so they are marked and dropped until the cycle
			// closes.
			if !self
				.inbox
				.transform
				.get()
				.is_some_and(|(x, y)| x != 0.0 || y != 0.0)
			{
				return Ok(());
			}
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
			self.inbox.transform.set(None);
			self.inbox.resetting.set(true);
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
			self.inbox.transform.set(Some((matrix[4], matrix[5])));
			Ok(())
		}
	}

	/// Maps a viewport status; `ENABLED` and `READY` are both idle.
	fn status(status: DIRECTMANIPULATION_STATUS) -> PanStatus {
		if status == DIRECTMANIPULATION_BUILDING {
			PanStatus::Building
		} else if status == DIRECTMANIPULATION_RUNNING {
			PanStatus::Running
		} else if status == DIRECTMANIPULATION_INERTIA {
			PanStatus::Inertia
		} else if status == DIRECTMANIPULATION_DISABLED {
			PanStatus::Disabled
		} else {
			PanStatus::Ready
		}
	}

	/// One window's Direct Manipulation viewport: a precision touchpad pans
	/// with the OS's own inertia, folded at the pixel-pan seam.
	pub(crate) struct DirectManipulation {
		manager: IDirectManipulationManager,
		updates: IDirectManipulationUpdateManager,
		viewport: IDirectManipulationViewport,
		feed: PanFeed,
		inbox: Rc<Inbox>,
		/// When the last touchpad contact was offered; the pump stays awake
		/// for it even before the viewport reports a gesture.
		contact_at: Option<Instant>,
		/// The touch contacts the viewport owns whose leaving it has not
		/// seen yet.
		contacts: Vec<u32>,
		/// True while the page pins: the OS inertia coasts over a held
		/// contact, and once set it outlives the lift until the hand
		/// re-grips or the coast is spent.
		pinned: bool,
		/// True while the viewport has a gesture, contact or inertia in
		/// flight.
		live: bool,
		hwnd: HWND,
	}

	impl DirectManipulation {
		/// Opts `window` into precision-touchpad panning, or returns `None`
		/// to keep today's wheel handling; every failure is silent.
		pub(crate) fn new(window: &winit::window::Window) -> Option<Self> {
			if std::env::var_os("MARKVIEW_NO_DM").is_some() {
				return None;
			}
			let Ok(handle) = window.window_handle() else {
				return None;
			};
			let RawWindowHandle::Win32(handle) = handle.as_raw() else {
				return None;
			};
			let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
			let size = window.inner_size();
			let rect = RECT {
				left: 0,
				top: 0,
				right: size.width as i32,
				bottom: size.height as i32,
			};
			let inbox = Rc::new(Inbox {
				statuses: RefCell::new(Vec::new()),
				transform: Cell::new(None),
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
					Ok(Self {
						manager,
						updates,
						viewport,
						feed: PanFeed::default(),
						inbox,
						contact_at: None,
						contacts: Vec::new(),
						pinned: false,
						live: false,
						hwnd,
					})
				}
			};
			match assembly() {
				Ok(owner) => Some(owner),
				Err(error) => {
					log::debug!("direct manipulation unavailable: {error}");
					None
				}
			}
		}

		/// Offers a touch contact to the viewport; `true` when it was a
		/// precision touchpad's and Direct Manipulation owns it now.
		pub(crate) fn contact(&mut self, pointer: u64) -> bool {
			let id = pointer as u32;
			let mut kind = POINTER_INPUT_TYPE::default();
			// SAFETY: `GetPointerType` writes one `POINTER_INPUT_TYPE` for a
			// live pointer id, and the event loop is still dispatching this
			// contact's message.
			if unsafe { GetPointerType(id, &mut kind) }.is_err()
				|| kind != PT_TOUCHPAD
			{
				return false;
			}
			// SAFETY: `Enable` and `SetContact` configure the window's own
			// viewport on this thread; `Enable` repeats harmlessly on one
			// that was never disabled.
			if unsafe { self.viewport.Enable() }.is_err()
				|| unsafe { self.viewport.SetContact(id) }.is_err()
			{
				return false;
			}
			self.contacts.push(id);
			self.contact_at = Some(Instant::now());
			log::debug!(
				"the viewport holds contact {id} ({} now)",
				self.contacts.len()
			);
			true
		}

		/// Retires a touch contact the viewport owned; `true` when it was
		/// the viewport's. Its answer tells the touch path that the contact
		/// was never a gesture of its own.
		pub(crate) fn release(&mut self, pointer: u64) -> bool {
			let id = pointer as u32;
			let before = self.contacts.len();
			self.contacts.retain(|&held| held != id);
			if before == self.contacts.len() {
				return false;
			}
			log::debug!("contact {id} left ({} remain)", self.contacts.len());
			true
		}

		/// Cancels the viewport's gesture: the OS stops all its transforms
		/// at once, and the stream's bookkeeping ends, so no stale delta
		/// speaks for a view that focus loss, a resize, a reload or a tab
		/// switch replaced. The next contact re-enables the viewport.
		pub(crate) fn abandon(&mut self) {
			let _ = self.feed.abandon();
			self.live = false;
			self.contact_at = None;
			self.contacts.clear();
			self.pinned = false;
			// What the callbacks recorded for the dead gesture must not
			// survive it: a later pump would fold it onto a fresh one.
			self.inbox.statuses.borrow_mut().clear();
			self.inbox.transform.set(None);
			self.inbox.resetting.set(false);
			// SAFETY: `Disable` stops the window's own viewport on this
			// thread; `Enable` from the next contact resumes it.
			let _ = unsafe { self.viewport.Disable() };
		}

		/// Follows the window: the viewport rect is the client area, and the
		/// transform turns into logical pixels at the display's scale.
		pub(crate) fn resize(&mut self, width: u32, height: u32, scale: f32) {
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

		/// Pumps the update manager and folds what its callbacks recorded
		/// into seam events, in logical pixels. While `held`, a
		/// pointer-driven interaction owns the input and the stream is
		/// silenced instead.
		pub(crate) fn pump(
			&mut self,
			held: bool,
		) -> Vec<(TouchPhase, f32, f32)> {
			if !self.live
				&& self
					.contact_at
					.is_none_or(|at| at.elapsed() >= CONTACT_WINDOW)
			{
				self.contact_at = None;
				return Vec::new();
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
					PanStatus::Building
						| PanStatus::Running
						| PanStatus::Inertia
				);
				if self.live {
					self.contact_at = None;
				}
			}
			let pinned = pin(
				self.pinned,
				!self.contacts.is_empty(),
				statuses.last().copied(),
			);
			if pinned != self.pinned {
				log::debug!(
					"the page {}",
					if pinned { "pins" } else { "releases the pin" }
				);
			}
			self.pinned = pinned;
			if held {
				return hold(&mut self.feed);
			}
			fold(
				&mut self.feed,
				statuses,
				transform,
				self.inbox.scale.get(),
				self.pinned,
			)
		}

		/// The next pump, while a gesture or an offered contact may still
		/// have news.
		pub(crate) fn deadline(&self, now: Instant) -> Option<Instant> {
			(self.live || self.contact_at.is_some())
				.then_some(now + PUMP_PERIOD)
		}
	}

	impl Drop for DirectManipulation {
		fn drop(&mut self) {
			// SAFETY: the teardown stops and releases the viewport and the
			// window's registration before the interfaces drop themselves.
			unsafe {
				let _ = self.viewport.Stop();
				let _ = self.viewport.Abandon();
				let _ = self.manager.Deactivate(self.hwnd);
			}
		}
	}
}

#[cfg(windows)]
pub(crate) use win::DirectManipulation;

/// Other desktops have no Direct Manipulation to offer; the reader pans by
/// the wheel paths it already had.
#[cfg(not(windows))]
pub(crate) struct DirectManipulation;

#[cfg(not(windows))]
impl DirectManipulation {
	pub(crate) fn new(_: &winit::window::Window) -> Option<Self> {
		None
	}
	pub(crate) fn contact(&mut self, _: u64) -> bool {
		false
	}
	pub(crate) fn release(&mut self, _: u64) -> bool {
		false
	}
	pub(crate) fn resize(&mut self, _: u32, _: u32, _: f32) {}
	pub(crate) fn abandon(&mut self) {}
	pub(crate) fn pump(&mut self, _: bool) -> Vec<(TouchPhase, f32, f32)> {
		Vec::new()
	}
	pub(crate) fn deadline(&self, _: Instant) -> Option<Instant> {
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn moved(events: &[(TouchPhase, f32, f32)]) -> Vec<(f32, f32)> {
		events
			.iter()
			.filter(|(phase, _, _)| matches!(phase, TouchPhase::Moved))
			.map(|(_, dx, dy)| (*dx, *dy))
			.collect()
	}

	#[test]
	fn a_pump_batch_starts_the_gesture_before_its_first_transform() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running];
		// Physical pixels on a 2× display; the first transform names the
		// origin, so it moves nothing however far it sits from zero.
		let events =
			fold(&mut feed, statuses, Some((200.0, -60.0)), 2.0, false);
		assert_eq!(events, vec![(TouchPhase::Started, 0.0, 0.0)]);
	}

	#[test]
	fn the_last_transform_of_a_closing_gesture_precedes_its_release() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((200.0, -60.0)), 2.0, false);
		// The lift and the final motion land in one pump.
		let statuses = vec![PanStatus::Ready];
		let events =
			fold(&mut feed, statuses, Some((240.0, -80.0)), 2.0, false);
		assert_eq!(moved(&events), vec![(20.0, -10.0)]);
		assert_eq!(
			events.last(),
			Some(&(TouchPhase::Ended, 0.0, 0.0)),
			"the release closes the gesture, after its last delta"
		);
	}

	#[test]
	fn the_inertia_tail_keeps_arriving_as_plain_deltas() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running, PanStatus::Inertia];
		fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, false);
		let events = fold(&mut feed, vec![], Some((30.0, 0.0)), 1.0, false);
		assert_eq!(moved(&events), vec![(30.0, 0.0)]);
	}

	#[test]
	fn a_reset_transform_between_gestures_moves_nothing() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((300.0, 0.0)), 1.0, false);
		let statuses = vec![PanStatus::Ready];
		fold(&mut feed, statuses, Some((300.0, 0.0)), 1.0, false);
		// The viewport resets its transform to the origin while idle; the
		// next gesture's first transform still only names its origin.
		let statuses = vec![PanStatus::Building];
		let events = fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, false);
		assert_eq!(events, vec![(TouchPhase::Started, 0.0, 0.0)]);
		let events = fold(&mut feed, vec![], Some((25.0, 10.0)), 1.0, false);
		assert_eq!(moved(&events), vec![(25.0, 10.0)]);
	}

	#[test]
	fn a_held_stream_is_silenced_and_cannot_resume_after_the_owner_lets_go() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, false);
		let events = fold(&mut feed, vec![], Some((30.0, 0.0)), 1.0, false);
		assert_eq!(moved(&events), vec![(30.0, 0.0)]);
		// A pointer drag takes the input: the gesture in flight ends once,
		// and the delta the batch carried drops with it.
		let events = hold(&mut feed);
		assert_eq!(events, vec![(TouchPhase::Cancelled, 0.0, 0.0)]);
		// The stale stream keeps arriving while the drag owns the input,
		// and pans nothing.
		let events = hold(&mut feed);
		assert!(events.is_empty());
		// The owner lets go; the stream is still stale and pans nothing.
		let events = fold(&mut feed, vec![], Some((50.0, 0.0)), 1.0, false);
		assert!(events.is_empty(), "a stale stream must not resume panning");
		// A fresh gesture after the drag pans again.
		let statuses = vec![PanStatus::Building];
		let events = fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, false);
		assert_eq!(events, vec![(TouchPhase::Started, 0.0, 0.0)]);
		let events = fold(&mut feed, vec![], Some((12.0, 0.0)), 1.0, false);
		assert_eq!(moved(&events), vec![(12.0, 0.0)]);
	}

	#[test]
	fn a_disabled_viewport_cancels_what_it_was_panning() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((0.0, 0.0)), 1.0, false);
		let statuses = vec![PanStatus::Disabled];
		let events = fold(&mut feed, statuses, Some((10.0, 0.0)), 1.0, false);
		// The teardown still delivers the contact's last motion, then cancels.
		assert_eq!(
			events,
			vec![
				(TouchPhase::Moved, 10.0, 0.0),
				(TouchPhase::Cancelled, 0.0, 0.0)
			]
		);
	}

	#[test]
	fn a_coast_over_a_held_contact_pins_the_page_and_keeps_the_anchor() {
		let mut feed = PanFeed::default();
		let statuses = vec![PanStatus::Running];
		fold(&mut feed, statuses, Some((30.0, 0.0)), 1.0, false);
		// The OS coasts the content while the contact is still on the pad;
		// the deltas drop, but the anchor rides the coast.
		let events = fold(
			&mut feed,
			vec![PanStatus::Inertia],
			Some((90.0, 0.0)),
			1.0,
			true,
		);
		assert!(
			moved(&events).is_empty(),
			"a coast the hand never asked for pans nothing"
		);
		let events = fold(&mut feed, vec![], Some((120.0, 0.0)), 1.0, true);
		assert!(moved(&events).is_empty());
		// The hand re-grips: the page pans by the hand's motion alone, not
		// by the distance the coast stole while pinned.
		let events = fold(
			&mut feed,
			vec![PanStatus::Running],
			Some((126.0, 0.0)),
			1.0,
			false,
		);
		assert_eq!(moved(&events), vec![(6.0, 0.0)]);
	}

	#[test]
	fn the_pin_sets_on_a_held_coast_and_outlives_the_contact() {
		assert!(pin(false, true, Some(PanStatus::Inertia)));
		// The fingers leave mid-coast; the dropped glide must not resurrect.
		assert!(pin(true, false, Some(PanStatus::Inertia)));
		assert!(pin(true, false, None));
		// A re-grip or a spent coast hands the page back.
		assert!(!pin(true, false, Some(PanStatus::Running)));
		assert!(!pin(true, false, Some(PanStatus::Ready)));
		// Without a coast there is nothing to pin.
		assert!(!pin(false, true, Some(PanStatus::Running)));
		assert!(!pin(false, true, None));
	}
}
