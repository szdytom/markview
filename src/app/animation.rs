//! The shared frame clock and the single entry point every continuous
//! animation advances through.
use crate::state::Animation;
use std::time::{Duration, Instant};

/// A display that reports no refresh rate is treated as 60 Hz.
const DEFAULT_REFRESH_MHZ: u32 = 60_000;
/// A paused or occluded frame must not turn one integration step into a jump.
const MAX_FRAME_DT: Duration = Duration::from_millis(100);

/// Drives animation frames from the surface's presentation.
///
/// `AutoVsync` already paces presentation. The refresh interval is only a
/// floor for backends whose present does not block, so a running animation
/// still asks for at most one frame per physical refresh.
pub(super) struct FrameDriver {
	now: Instant,
	interval: Duration,
	dt: Duration,
	active: bool,
	advancing: bool,
	framed: bool,
	next_at: Option<Instant>,
}

impl Default for FrameDriver {
	fn default() -> Self {
		Self {
			now: Instant::now(),
			interval: Duration::from_secs_f64(1.0 / 60.0),
			dt: Duration::ZERO,
			active: false,
			advancing: false,
			framed: false,
			next_at: None,
		}
	}
}

impl FrameDriver {
	pub(super) fn set_refresh_rate(&mut self, millihertz: Option<u32>) {
		if let Some(millihertz) = millihertz.filter(|mhz| *mhz > 0) {
			self.interval =
				Duration::from_secs_f64(1000.0 / f64::from(millihertz));
		} else {
			self.interval = Duration::from_secs_f64(
				1000.0 / f64::from(DEFAULT_REFRESH_MHZ),
			);
		}
	}

	/// Records the frame time after `acquire` succeeded and returns the time
	/// since the previous active frame.
	pub(super) fn begin(&mut self, now: Instant) -> Duration {
		let dt = if self.active {
			now.saturating_duration_since(self.now)
		} else {
			Duration::ZERO
		};
		self.dt = dt.min(MAX_FRAME_DT);
		self.now = now;
		self.framed = true;
		self.next_at = None;
		self.active = false;
		self.dt
	}

	pub(super) fn set_active(&mut self, active: bool) {
		self.active = active;
	}

	pub(super) fn active(&self) -> bool {
		self.active
	}

	pub(super) fn set_advancing(&mut self, advancing: bool) {
		self.advancing = advancing;
	}

	pub(super) fn is_advancing(&self) -> bool {
		self.advancing
	}

	pub(super) fn now(&self) -> Instant {
		if self.framed {
			self.now
		} else {
			Instant::now()
		}
	}

	pub(super) fn dt(&self) -> Duration {
		self.dt
	}

	/// Records a presentation. Returns whether the next animation frame is
	/// due immediately instead of at `deadline`.
	pub(super) fn presented(&mut self, now: Instant) -> bool {
		if !self.active {
			self.next_at = None;
			return false;
		}
		let due = self.now + self.interval;
		if now >= due {
			self.next_at = None;
			true
		} else {
			self.next_at = Some(due);
			false
		}
	}

	pub(super) fn take_due(&mut self, now: Instant) -> bool {
		if self.next_at.is_some_and(|due| due <= now) {
			self.next_at = None;
			true
		} else {
			false
		}
	}

	pub(super) fn deadline(&self) -> Option<Instant> {
		self.next_at
	}
}

impl Animation for markview_interaction::Scroll {
	fn advance(&mut self, now: Instant) -> bool {
		markview_interaction::Scroll::advance(self, now)
	}
}

impl<P: super::SendEvent> super::App<P> {
	pub(super) fn frame_now(&self) -> Instant {
		self.animations.now()
	}

	pub(super) fn animating(&self) -> bool {
		self.animations.active()
	}

	/// Advances every continuous animation exactly once for the current
	/// presented frame. Returns whether another frame is needed.
	pub(super) fn advance_animations(
		&mut self,
		now: Instant,
		dt: Duration,
	) -> bool {
		let mut active = false;
		active |= self.advance_gestures(now);
		active |= self.advance_scroll(now);
		active |= self.advance_control_scroll(now);
		active |= self.advance_drawers(now);
		active |= self.advance_scrollbars(now);
		active |= self.advance_edge_autoscroll(dt);
		self.refresh_hover();
		active
	}

	/// Follows the tab strip's edge and the selection's viewport edge while a
	/// drag holds them there. Both are speed integrations, not fixed steps.
	fn advance_edge_autoscroll(&mut self, dt: Duration) -> bool {
		let mut active = false;
		if self.tab_strip.drag.is_some_and(|drag| drag.moving) {
			let layout = self.tab_layout();
			let speed =
				layout.edge_scroll_speed(self.interaction.pointer.cursor().0);
			if speed != 0.0 {
				active = true;
				let offset = layout.scroll + speed * dt.as_secs_f32();
				self.tab_strip.scroll.set_max(layout.max_scroll);
				self.tab_strip
					.scroll
					.set(offset.clamp(0.0, layout.max_scroll));
				self.move_tab_drag();
				self.redraw();
			}
		}
		if self.interaction.pointer.drag().is_some()
			&& self.interaction.pointer.dragged()
		{
			let (_, height, _) = self.dimensions();
			let speed = markview_interaction::selection_scroll_speed(
				self.interaction.pointer.cursor().1,
				super::TOP,
				height - self.bottom(),
				self.readers.session.scrolling.offset,
				(self.readers.session.display.snapshot.height
					- self.viewport())
				.max(0.0),
			);
			if speed != 0.0 {
				active = true;
				if dt > Duration::ZERO {
					self.scroll_by(speed * dt.as_secs_f32());
				}
			}
		}
		active
	}

	pub(super) fn advance_scrollbars(&mut self, now: Instant) -> bool {
		let document = self.scrollbar_reveal.advance(now);
		let folder = self.folder.advance_scrollbar(now);
		document || folder
	}

	pub(super) fn sync_refresh_rate(&mut self) {
		let rate = self
			.window
			.as_ref()
			.and_then(|window| window.current_monitor())
			.and_then(|monitor| monitor.refresh_rate_millihertz());
		self.animations.set_refresh_rate(rate);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn driver(millihertz: u32) -> FrameDriver {
		let mut driver = FrameDriver::default();
		driver.set_refresh_rate(Some(millihertz));
		driver
	}

	#[test]
	fn unknown_refresh_rate_falls_back_to_sixty_hertz() {
		let mut driver = FrameDriver::default();
		driver.set_refresh_rate(None);
		assert_eq!(driver.interval, Duration::from_secs_f64(1.0 / 60.0));
	}

	#[test]
	fn an_idle_driver_never_asks_for_another_frame() {
		let mut driver = driver(60_000);
		let now = Instant::now();
		driver.begin(now);
		driver.set_active(false);
		assert!(!driver.presented(now + Duration::from_millis(100)));
		assert_eq!(driver.deadline(), None);
	}

	#[test]
	fn an_active_driver_immediate_or_scheduled_by_refresh() {
		let mut driver = driver(60_000);
		let start = Instant::now();
		driver.begin(start);
		driver.set_active(true);
		assert!(!driver.presented(start + Duration::from_millis(5)));
		assert!(driver.deadline().is_some());
		let due = driver.deadline().unwrap();
		assert!(!driver.take_due(due - Duration::from_millis(1)));
		assert!(driver.take_due(due));

		driver.begin(start + Duration::from_millis(20));
		driver.set_active(true);
		assert!(driver.presented(start + Duration::from_millis(40)));
		assert_eq!(driver.deadline(), None);
	}

	#[test]
	fn dt_is_only_carried_across_active_frames_and_is_clamped() {
		let mut driver = driver(60_000);
		let start = Instant::now();
		driver.begin(start);
		driver.set_active(true);
		assert_eq!(
			driver.begin(start + Duration::from_millis(16)),
			Duration::from_millis(16)
		);
		driver.set_active(false);
		assert_eq!(
			driver.begin(start + Duration::from_millis(32)),
			Duration::ZERO
		);
		driver.set_active(true);
		assert_eq!(driver.begin(start + Duration::from_secs(5)), MAX_FRAME_DT);
	}
}
