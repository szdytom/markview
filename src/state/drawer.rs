use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(220);

pub(crate) struct DrawerAnimation {
	from: f32,
	to: f32,
	started: Instant,
	duration: Duration,
	#[cfg(test)]
	frozen_at: Option<Instant>,
}

impl DrawerAnimation {
	#[cfg(test)]
	pub(crate) fn freeze(&mut self, elapsed: Duration) {
		self.frozen_at = Some(self.started + elapsed);
	}

	fn position(&self, now: Instant) -> f32 {
		#[cfg(test)]
		let now = self.frozen_at.unwrap_or(now);
		let t = (now.saturating_duration_since(self.started).as_secs_f32()
			/ self.duration.as_secs_f32())
		.clamp(0.0, 1.0);
		let eased = 1.0 - (1.0 - t).powi(3);
		self.from + (self.to - self.from) * eased
	}

	pub(crate) fn retarget(
		animation: &mut Option<Self>,
		open: bool,
		was_open: bool,
	) {
		Self::retarget_at(animation, open, was_open, Instant::now());
	}

	fn retarget_at(
		animation: &mut Option<Self>,
		open: bool,
		was_open: bool,
		now: Instant,
	) {
		if open == was_open {
			return;
		}
		let from = position(animation, was_open, now);
		let to = u8::from(open) as f32;
		*animation = (from != to).then_some(Self {
			from,
			to,
			started: now,
			duration: DURATION.mul_f32((to - from).abs()),
			#[cfg(test)]
			frozen_at: None,
		});
	}

	/// The drawer's reveal at `now`, or its settled value while it rests.
	pub(crate) fn reveal(
		animation: &Option<Self>,
		open: bool,
		now: Instant,
	) -> f32 {
		position(animation, open, now)
	}

	/// Whether the animation has reached its target and may be dropped.
	pub(crate) fn is_finished(&self, now: Instant) -> bool {
		now >= self.started + self.duration
	}
}

fn position(
	animation: &Option<DrawerAnimation>,
	open: bool,
	now: Instant,
) -> f32 {
	animation
		.as_ref()
		.map_or(u8::from(open) as f32, |a| a.position(now))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn drawer_motion_eases_reverses_continuously_and_settles() {
		let start = Instant::now();
		let mut animation = None;
		DrawerAnimation::retarget_at(&mut animation, true, false, start);
		assert_eq!(position(&animation, true, start), 0.0);
		let halfway = start + DURATION / 2;
		let revealed = position(&animation, true, halfway);
		assert_eq!(revealed, 0.875);
		assert_eq!(position(&animation, true, start + DURATION), 1.0);
		DrawerAnimation::retarget_at(&mut animation, false, true, halfway);
		assert_eq!(position(&animation, false, halfway), revealed);
		assert!(position(&animation, false, halfway + DURATION / 4) < revealed);
		assert_eq!(position(&animation, false, halfway + DURATION), 0.0);
	}
}
