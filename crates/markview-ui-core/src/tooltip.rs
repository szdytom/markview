use std::time::{Duration, Instant};

/// A passive hint's lifecycle. The host supplies targets, time and painting.
/// A dismissed target stays hidden until the target changes or leaves.
#[derive(Debug)]
pub struct Tooltip<T> {
	target: Option<T>,
	deadline: Option<Instant>,
	visible: bool,
}

impl<T> Default for Tooltip<T> {
	fn default() -> Self {
		Self {
			target: None,
			deadline: None,
			visible: false,
		}
	}
}

impl<T: PartialEq> Tooltip<T> {
	/// Reconcile the target; repeated motion within it preserves the deadline.
	/// Keyboard focus can supply a zero delay to show its hint immediately.
	pub fn update(
		&mut self,
		target: Option<T>,
		now: Instant,
		delay: Duration,
	) -> bool {
		if self.target == target {
			if self.deadline.is_some() && delay.is_zero() {
				self.deadline = None;
				self.visible = true;
				return true;
			}
			return false;
		}
		let was_visible = self.visible;
		self.target = target;
		self.visible = self.target.is_some() && delay.is_zero();
		self.deadline =
			(self.target.is_some() && !self.visible).then_some(now + delay);
		was_visible || self.visible
	}

	pub fn deadline(&self) -> Option<Instant> {
		self.deadline
	}

	/// Advance a pending hint once, leaving no timer after it opens.
	pub fn tick(&mut self, now: Instant) -> bool {
		if self.deadline.is_some_and(|at| at <= now) {
			self.deadline = None;
			self.visible = true;
			return true;
		}
		false
	}

	pub fn visible(&self) -> Option<&T> {
		self.target.as_ref().filter(|_| self.visible)
	}

	pub fn dismiss(&mut self) -> bool {
		self.deadline = None;
		std::mem::take(&mut self.visible)
	}
}

#[cfg(test)]
mod tests;
