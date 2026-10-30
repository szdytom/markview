use super::{Easing, Tween};
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(220);

pub(crate) struct DrawerAnimation(Tween);

impl DrawerAnimation {
	#[cfg(test)]
	pub(crate) fn freeze(&mut self, elapsed: Duration) {
		self.0.freeze(elapsed);
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
		*animation = (from != to).then(|| {
			Self(Tween::new(
				from,
				to,
				now,
				DURATION.mul_f32((to - from).abs()),
				Easing::CubicOut,
			))
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
		self.0.is_finished(now)
	}
}

fn position(
	animation: &Option<DrawerAnimation>,
	open: bool,
	now: Instant,
) -> f32 {
	animation
		.as_ref()
		.map_or(u8::from(open) as f32, |animation| animation.0.value(now))
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
