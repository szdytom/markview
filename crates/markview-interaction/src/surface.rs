//! Scrollable UI surfaces register their motion with the frame clock on input.
use crate::{ScrollBounds, ScrollPhysics, WheelMode};
use std::{
	cell::RefCell,
	rc::{Rc, Weak},
};
use web_time::Instant;

type SurfaceState = Rc<RefCell<ScrollPhysics>>;
type Surfaces = RefCell<Vec<Weak<RefCell<ScrollPhysics>>>>;

/// The frame clock owns only weak references; closing a surface stops its motion.
#[derive(Clone)]
pub struct ScrollClock {
	surfaces: Rc<Surfaces>,
	wake: Rc<dyn Fn()>,
}

impl std::fmt::Debug for ScrollClock {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ScrollClock").finish_non_exhaustive()
	}
}

/// A scroll request carries its clock and input semantics through routing.
/// Only [`ScrollSurface::apply`] can turn it into movement.
#[must_use = "pass the request to a scroll surface"]
#[derive(Clone, Debug)]
pub struct ScrollRequest {
	clock: ScrollClock,
	at: Instant,
	motion: Motion,
}

#[derive(Clone, Copy, Debug)]
enum Motion {
	Wheel(f32, WheelMode),
	By(f32),
	To(f32),
}

impl ScrollClock {
	/// Wakes the host for the first frame, including directly tracked movement.
	pub fn new(wake: impl Fn() + 'static) -> Self {
		Self {
			surfaces: Rc::default(),
			wake: Rc::new(wake),
		}
	}
	pub fn wheel(
		&self,
		delta: f32,
		at: Instant,
		mode: WheelMode,
	) -> ScrollRequest {
		ScrollRequest {
			clock: self.clone(),
			at,
			motion: Motion::Wheel(delta, mode),
		}
	}
	/// A keyboard or navigation step, eased from the pending destination.
	pub fn step(&self, delta: f32, at: Instant) -> ScrollRequest {
		ScrollRequest {
			clock: self.clone(),
			at,
			motion: Motion::By(delta),
		}
	}
	/// An absolute navigation destination, eased within the surface's bounds.
	pub fn to(&self, offset: f32, at: Instant) -> ScrollRequest {
		ScrollRequest {
			clock: self.clone(),
			at,
			motion: Motion::To(offset),
		}
	}
	pub fn animating(&self) -> bool {
		self.surfaces.borrow().iter().any(|surface| {
			surface
				.upgrade()
				.is_some_and(|surface| surface.borrow().animating())
		})
	}
	/// Advances every registered surface once, retaining only active motion.
	pub fn advance(&self, now: Instant) -> bool {
		self.surfaces.borrow_mut().retain(|surface| {
			surface
				.upgrade()
				.is_some_and(|surface| surface.borrow_mut().advance(now))
		});
		!self.surfaces.borrow().is_empty()
	}
}

/// UI scrolling has no writable offset or manual animation registration.
/// Wheel and navigation requests always use the shared motion engine and clock.
/// Pointer tracking and restoration use the explicitly immediate methods.
///
/// ```compile_fail
/// use markview_interaction::ScrollSurface;
/// let mut surface = ScrollSurface::default();
/// surface.offset = 100.0;
/// ```
///
/// ```compile_fail
/// use markview_interaction::ScrollSurface;
/// let mut surface = ScrollSurface::default();
/// surface.apply(100.0); // A distance cannot discard its input mode and clock.
/// ```
#[derive(Debug, Default)]
pub struct ScrollSurface {
	state: SurfaceState,
	clock: Weak<Surfaces>,
}

impl ScrollSurface {
	pub fn offset(&self) -> f32 {
		self.state.borrow().offset()
	}
	pub fn animating(&self) -> bool {
		self.state.borrow().animating()
	}
	pub fn fit(&mut self, bounds: ScrollBounds) {
		self.state.borrow_mut().fit(bounds);
	}
	pub fn set_max(&mut self, max: f32) {
		self.state.borrow_mut().set_max(max);
	}
	pub fn cancel(&mut self) {
		self.state.borrow_mut().cancel();
	}
	pub fn reset(&mut self) {
		self.state.borrow_mut().reset();
	}
	/// Restores a position or follows a dragged scrollbar without easing.
	pub fn jump_to(&mut self, offset: f32) {
		self.state.borrow_mut().set(offset);
	}
	/// Follows a pointer pan or another input that already supplies each frame.
	pub fn pan_by(&mut self, delta: f32) {
		self.state.borrow_mut().by(delta);
	}
	pub fn apply(&mut self, request: ScrollRequest) {
		let mut state = self.state.borrow_mut();
		match request.motion {
			Motion::Wheel(delta, mode) => state.wheel(delta, request.at, mode),
			Motion::By(delta) => {
				let base = state.target.unwrap_or(state.offset);
				let target = (base + delta).clamp(0.0, state.bounds_max());
				state.animate_to(target, request.at);
			}
			Motion::To(target) => {
				let target = target.clamp(0.0, state.bounds_max());
				state.animate_to(target, request.at);
			}
		}
		let animating = state.animating();
		drop(state);
		if !animating {
			(request.clock.wake)();
			return;
		}
		let weak = Rc::downgrade(&self.state);
		if let Some(previous) = self.clock.upgrade()
			&& !Rc::ptr_eq(&previous, &request.clock.surfaces)
		{
			previous.borrow_mut().retain(|entry| !entry.ptr_eq(&weak));
		}
		self.clock = Rc::downgrade(&request.clock.surfaces);
		let mut surfaces = request.clock.surfaces.borrow_mut();
		surfaces.retain(|entry| entry.strong_count() > 0);
		if !surfaces.iter().any(|entry| entry.ptr_eq(&weak)) {
			surfaces.push(weak);
		}
		drop(surfaces);
		(request.clock.wake)();
	}
}

impl From<f32> for ScrollSurface {
	fn from(offset: f32) -> Self {
		Self {
			state: Rc::new(RefCell::new(ScrollPhysics::from(offset))),
			clock: Weak::new(),
		}
	}
}
