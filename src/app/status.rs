//! The transient status line the footer and the warning strip read.
use std::time::{Duration, Instant};

/// The reader's transient line, with its error bit and deadline.
#[derive(Default)]
pub(super) struct Status {
	message: String,
	error: bool,
	until: Option<Instant>,
}

impl Status {
	/// Replaces the message, the error bit and the deadline.
	pub(super) fn set(
		&mut self,
		message: impl Into<String>,
		error: bool,
		seconds: u64,
	) {
		self.message = message.into();
		self.error = error;
		self.until = deadline_after(seconds);
	}

	/// Replaces the message and the deadline, keeping the error bit.
	pub(super) fn set_timed(
		&mut self,
		message: impl Into<String>,
		seconds: u64,
	) {
		self.message = message.into();
		self.until = deadline_after(seconds);
	}

	/// Replaces the message only.
	pub(super) fn set_message(&mut self, message: impl Into<String>) {
		self.message = message.into();
	}

	/// Sets the error bit only.
	pub(super) fn set_error(&mut self, error: bool) {
		self.error = error;
	}

	/// Clears the message only.
	pub(super) fn clear_message(&mut self) {
		self.message.clear();
	}

	/// Clears the error bit and the deadline, keeping the message.
	pub(super) fn reset(&mut self) {
		self.error = false;
		self.until = None;
	}

	/// Clears the message, the error bit and the deadline.
	pub(super) fn clear(&mut self) {
		self.clear_message();
		self.reset();
	}

	/// The footer's trailing line, while the message's deadline is live.
	pub(super) fn current(&self, now: Instant) -> Option<&str> {
		self.live(now).then_some(self.message())
	}

	/// The warning strip's line: a failure that outlived its deadline.
	pub(super) fn warning(&self, now: Instant) -> Option<&str> {
		(self.error && !self.live(now)).then_some(self.message())
	}

	/// When the deadline passes, while one is pending.
	pub(super) fn deadline(&self) -> Option<Instant> {
		self.until
	}

	/// The stored message, whether or not a line shows now.
	pub(super) fn message(&self) -> &str {
		&self.message
	}

	fn live(&self, now: Instant) -> bool {
		self.until.is_some_and(|until| until > now)
	}

	/// Whether the stored line reports a failure.
	#[cfg(test)]
	pub(super) fn is_error(&self) -> bool {
		self.error
	}
}

/// The deadline `seconds` from now; no deadline for zero.
fn deadline_after(seconds: u64) -> Option<Instant> {
	(seconds > 0).then(|| Instant::now() + Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn expiry_ends_a_timed_line() {
		let mut status = Status::default();
		status.set("Saved", false, 2);
		assert_eq!(status.current(Instant::now()), Some("Saved"));
		let after = Instant::now() + Duration::from_secs(5);
		assert_eq!(status.current(after), None);
		assert_eq!(status.warning(after), None);
	}

	#[test]
	fn clear_removes_the_line_and_its_deadline() {
		let mut status = Status::default();
		status.set("Saved", false, 10);
		status.clear();
		assert_eq!(status.message(), "");
		assert_eq!(status.current(Instant::now()), None);
		assert_eq!(status.deadline(), None);
	}

	#[test]
	fn an_error_takes_precedence_over_its_deadline() {
		let mut status = Status::default();
		status.set("Export failed", true, 1);
		let now = Instant::now();
		assert_eq!(status.current(now), Some("Export failed"));
		assert_eq!(status.warning(now), None);
		let after = now + Duration::from_secs(60);
		assert_eq!(status.current(after), None);
		assert_eq!(status.warning(after), Some("Export failed"));
	}

	#[test]
	fn zero_seconds_clears_the_line_immediately() {
		let mut status = Status::default();
		status.set("Saving", false, 5);
		status.set("Saved", false, 0);
		assert_eq!(status.current(Instant::now()), None);
		assert_eq!(status.deadline(), None);
	}

	#[test]
	fn a_message_only_write_keeps_the_live_deadline() {
		let mut status = Status::default();
		status.set("Path copied", false, 3);
		status.set_message("Selection copied");
		assert!(status.deadline().is_some());
		assert_eq!(status.current(Instant::now()), Some("Selection copied"));
		let after = Instant::now() + Duration::from_secs(10);
		assert_eq!(status.current(after), None);
	}

	#[test]
	fn reset_clears_the_deadline_and_keeps_the_message() {
		let mut status = Status::default();
		status.set("Saving", true, 10);
		status.reset();
		assert_eq!(status.message(), "Saving");
		assert!(!status.is_error());
		assert_eq!(status.deadline(), None);
		assert_eq!(status.current(Instant::now()), None);
		assert_eq!(status.warning(Instant::now()), None);
	}
}
