//! Windows Direct Manipulation bookkeeping, kept platform-neutral so it tests
//! everywhere; the COM event handler is a thin adapter over [`PanFeed`].

/// The status of a panning viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanStatus {
	/// The viewport is configuring a gesture.
	Building,
	/// The viewport is idle.
	Ready,
	/// A contact pans the viewport.
	Running,
	/// The OS inertia engine is carrying the gesture.
	Inertia,
	/// The viewport is disabled.
	Disabled,
}

/// A gesture lifecycle event for the pixel-pan seam, in the semantics of
/// macOS pixel scrolling: `Started` cancels whatever scroll state is running,
/// and `Ended` is a plain release — the deltas carried the whole motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanPhase {
	Started,
	Ended,
	Cancelled,
}

/// Folds a panning viewport's status changes and content transforms into pan
/// phases and per-frame deltas.
///
/// The viewport leaving idle — `Building`, `Running` or `Inertia` — starts a
/// gesture exactly once, and every content transform yields the delta since
/// the last one, the first against the origin the ready-reset parks the
/// content at: the seam pans by exactly these during contact and through the
/// OS inertia tail. The viewport returning to idle
/// releases; after a release or cancellation nothing speaks again until a new
/// gesture starts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanFeed {
	active: bool,
	last: Option<(f32, f32)>,
}

impl PanFeed {
	/// Folds a status change, returning the phase to feed, if any.
	pub fn status(&mut self, status: PanStatus) -> Option<PanPhase> {
		match status {
			PanStatus::Building | PanStatus::Running | PanStatus::Inertia => {
				if self.active {
					return None;
				}
				self.active = true;
				// The ready-reset parks the content at its origin between
				// gestures, so the first transform of a gesture is the
				// travel since it began, not a baseline to swallow.
				self.last = Some((0.0, 0.0));
				Some(PanPhase::Started)
			}
			PanStatus::Ready => {
				if !self.active {
					return None;
				}
				self.active = false;
				self.last = None;
				Some(PanPhase::Ended)
			}
			PanStatus::Disabled => self.abandon(),
		}
	}

	/// Folds a content transform, returning the delta since the last one to
	/// feed as a `Moved` pan. The content rests at its origin between
	/// gestures, so the first transform of a gesture is the travel since it
	/// began; a transform outside a gesture is ignored.
	pub fn transform(&mut self, x: f32, y: f32) -> Option<(f32, f32)> {
		if !self.active {
			return None;
		}
		self.last.replace((x, y)).map(|(lx, ly)| (x - lx, y - ly))
	}

	/// Cancels a gesture in flight, as an abandoned viewport does.
	pub fn abandon(&mut self) -> Option<PanPhase> {
		if !self.active {
			return None;
		}
		self.active = false;
		self.last = None;
		Some(PanPhase::Cancelled)
	}
}
