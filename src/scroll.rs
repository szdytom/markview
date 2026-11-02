//! Eased scrolling shared by the non-document surfaces.
use markview_selection::{ScrollBounds, ScrollState};
use std::time::Instant;

/// A clamped offset with eased wheel travel and its own frame deadline.
///
/// The document owns [`ScrollState`] directly because its bounds grow while
/// the layout streams in. Every other surface knows its whole bound at wheel
/// time, so this wrapper supplies the same motion with that bound in hand.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScrollArea {
	state: ScrollState,
	/// The bound the last wheel or frame supplied.
	max: f32,
}

impl ScrollArea {
	pub(crate) fn new(offset: f32) -> Self {
		let mut area = Self {
			// A detached offset has no bound until the surface says otherwise.
			max: f32::MAX,
			..Self::default()
		};
		area.state.offset = offset.max(0.0);
		area
	}
	pub(crate) fn offset(&self) -> f32 {
		self.state.offset.clamp(0.0, self.max)
	}
	/// Replaces the bound, clamping a resting offset to it.
	pub(crate) fn set_max(&mut self, max: f32) {
		self.max = max.max(0.0);
		self.state.resolve(self.bounds());
	}
	/// Stops motion where it is, keeping the displayed offset.
	pub(crate) fn cancel(&mut self) {
		self.state.cancel();
	}
	/// Clears any motion and returns the area to its start.
	pub(crate) fn reset(&mut self) {
		self.state.cancel();
		self.state.offset = 0.0;
		self.state.target = None;
	}
	/// Moves to `offset` without easing, as a scrollbar drag does.
	pub(crate) fn set(&mut self, offset: f32) {
		self.state.set(offset, self.bounds());
	}
	/// Moves by `delta` without easing, as a pan gesture does.
	pub(crate) fn by(&mut self, delta: f32) {
		self.state.by(delta, self.bounds());
	}
	/// Starts or retargets an eased wheel step.
	pub(crate) fn wheel(&mut self, delta: f32, now: Instant) {
		self.state.wheel_by(delta, now);
	}
	/// Feeds one packet of a high-resolution wheel stream, which carries its
	/// own momentum instead of a single eased step.
	pub(crate) fn wheel_packet(&mut self, delta: f32, now: Instant) {
		self.state.coast_wheel_by(delta, now);
	}
	/// Whether a high-resolution stream is live enough to adopt a whole
	/// detent.
	pub(crate) fn wheel_stream_alive(&self, now: Instant) -> bool {
		self.state.wheel_stream_alive(now)
	}
	/// Advances eased motion. Returns whether another frame is due.
	pub(crate) fn advance(&mut self, now: Instant) -> bool {
		self.state.advance(now, self.bounds())
	}
	pub(crate) fn animating(&self) -> bool {
		self.state.animating()
	}
	pub(crate) fn deadline(&self, now: Instant) -> Option<Instant> {
		self.state.deadline(now)
	}
	fn bounds(&self) -> ScrollBounds {
		ScrollBounds {
			max: self.max,
			complete: true,
		}
	}
}

impl From<f32> for ScrollArea {
	fn from(offset: f32) -> Self {
		Self::new(offset)
	}
}
