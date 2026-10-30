//! Time-based motion shared by the chrome's drawers and list rows.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Easing {
	CubicOut,
	ExpoOut,
}

impl Easing {
	fn ease(self, t: f32) -> f32 {
		match self {
			Self::CubicOut => 1.0 - (1.0 - t).powi(3),
			Self::ExpoOut if t >= 1.0 => 1.0,
			Self::ExpoOut => 1.0 - 2.0_f32.powf(-10.0 * t),
		}
	}
}

/// A scalar that moves from one value to another over a fixed duration.
pub(crate) struct Tween {
	from: f32,
	to: f32,
	started: Instant,
	duration: Duration,
	easing: Easing,
	#[cfg(test)]
	frozen_at: Option<Instant>,
}

impl Tween {
	pub(crate) fn new(
		from: f32,
		to: f32,
		now: Instant,
		duration: Duration,
		easing: Easing,
	) -> Self {
		Self {
			from,
			to,
			started: now,
			duration,
			easing,
			#[cfg(test)]
			frozen_at: None,
		}
	}

	/// The eased progress in `0..=1` at `now`.
	pub(crate) fn progress(&self, now: Instant) -> f32 {
		#[cfg(test)]
		let now = self.frozen_at.unwrap_or(now);
		let t = now.saturating_duration_since(self.started).as_secs_f32()
			/ self.duration.as_secs_f32();
		self.easing.ease(t.clamp(0.0, 1.0))
	}

	pub(crate) fn value(&self, now: Instant) -> f32 {
		self.from + (self.to - self.from) * self.progress(now)
	}

	pub(crate) fn is_finished(&self, now: Instant) -> bool {
		now >= self.started + self.duration
	}

	#[cfg(test)]
	pub(crate) fn freeze(&mut self, elapsed: Duration) {
		self.frozen_at = Some(self.started + elapsed);
	}
}
