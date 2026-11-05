//! Time-based motion shared by the chrome's drawers and list rows.
use std::time::{Duration, Instant};

#[cfg(test)]
mod proptests;

/// A continuous visual animation advanced by the frame driver.
///
/// `advance` returns whether another frame is needed. The value itself is
/// sampled separately at the frame time, so an animation that has reached its
/// end is not sampled again.
pub(crate) trait Animation {
	fn advance(&mut self, now: Instant) -> bool;
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Easing {
	CubicOut,
	ExpoOut,
	/// Firefox's `--animation-easing-function`.
	TabDrag,
}

impl Easing {
	fn ease(self, t: f32) -> f32 {
		match self {
			Self::CubicOut => 1.0 - (1.0 - t).powi(3),
			Self::ExpoOut if t >= 1.0 => 1.0,
			Self::ExpoOut => 1.0 - 2.0_f32.powf(-10.0 * t),
			Self::TabDrag => cubic_bezier(0.07, 0.95, 0.0, 1.0, t),
		}
	}
}

/// The eased y of `cubic-bezier(x1, y1, x2, y2)` at `t`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
	let (mut lo, mut hi) = (0.0, 1.0);
	let mut u = t;
	for _ in 0..24 {
		let x = segment(x1, x2, u);
		if (x - t).abs() < 1e-5 {
			break;
		}
		if x < t {
			lo = u;
		} else {
			hi = u;
		}
		u = (lo + hi) / 2.0;
	}
	segment(y1, y2, u)
}

fn segment(c1: f32, c2: f32, u: f32) -> f32 {
	let v = 1.0 - u;
	3.0 * v * v * u * c1 + 3.0 * v * u * u * c2 + u * u * u
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

impl Animation for Tween {
	fn advance(&mut self, now: Instant) -> bool {
		!self.is_finished(now)
	}
}

impl Animation for Option<Tween> {
	fn advance(&mut self, now: Instant) -> bool {
		let Some(tween) = self else {
			return false;
		};
		if tween.advance(now) {
			true
		} else {
			*self = None;
			false
		}
	}
}

/// A linear opacity fade, used by the scrollbars after their linger expires.
pub(crate) struct Fade {
	from: f32,
	to: f32,
	started: Instant,
	duration: Duration,
}

impl Fade {
	pub(crate) fn new(
		from: f32,
		to: f32,
		now: Instant,
		duration: Duration,
	) -> Self {
		Self {
			from,
			to,
			started: now,
			duration,
		}
	}

	pub(crate) fn value(&self, now: Instant) -> f32 {
		let t = now.saturating_duration_since(self.started).as_secs_f32()
			/ self.duration.as_secs_f32();
		self.from + (self.to - self.from) * t.clamp(0.0, 1.0)
	}

	pub(crate) fn is_finished(&self, now: Instant) -> bool {
		now >= self.started + self.duration
	}
}

impl Animation for Fade {
	fn advance(&mut self, now: Instant) -> bool {
		now < self.started + self.duration
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_finished_tween_clears_and_a_fade_samples_linearly() {
		let start = Instant::now();
		let mut tween = Some(Tween::new(
			0.0,
			1.0,
			start,
			Duration::from_millis(100),
			Easing::CubicOut,
		));
		assert!(tween.advance(start + Duration::from_millis(50)));
		assert!(!tween.advance(start + Duration::from_millis(100)));
		assert!(tween.is_none());

		let fade = Fade::new(1.0, 0.0, start, Duration::from_millis(100));
		assert_eq!(fade.value(start), 1.0);
		assert_eq!(fade.value(start + Duration::from_millis(50)), 0.5);
		assert_eq!(fade.value(start + Duration::from_millis(100)), 0.0);
	}
	#[test]
	fn firefox_easing_leaves_fast_and_settles() {
		assert_eq!(Easing::TabDrag.ease(0.0), 0.0);
		assert_eq!(Easing::TabDrag.ease(1.0), 1.0);
		assert!(Easing::TabDrag.ease(0.5) > 0.95);
	}
}
