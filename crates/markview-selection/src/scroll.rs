//! Time-driven document scrolling; the host supplies bounds and a clock.
use std::time::Duration;
use web_time::Instant;

pub const SCROLL_MIN: Duration = Duration::from_millis(120);
pub const SCROLL_MAX: Duration = Duration::from_millis(400);
const SCROLL_FULL: f32 = 2400.0;
/// How often a running animation asks the event loop for a frame.
const SCROLL_FRAME: Duration = Duration::from_millis(8);

/// How long a high-resolution wheel stream's momentum takes to die away, in
/// seconds. It is also how far ahead of the packets that momentum may carry
/// the page, because at a fixed speed distance and time say the same thing.
const COAST: f32 = 0.25;
/// The shortest gap a packet's own speed is read over, so a timer that never
/// advanced cannot claim an unbounded rate.
const PACKET_MIN: f32 = 0.008;
/// A pause this long ends the run of packets whose spacing sets the speed: a
/// packet after it belongs to a new gesture and says nothing about the hand.
const PACKET_GAP: f32 = 0.15;
/// How much of a packet's own speed one reading takes in.
const PACKET_BLEND: f32 = 0.05;

/// Ease-out cubic: fast away from the start and settling into the target.
/// Both ends are exact and the curve is strictly increasing between them.
pub fn ease_out_cubic(t: f32) -> f32 {
	let remaining = 1.0 - t.clamp(0.0, 1.0);
	1.0 - remaining * remaining * remaining
}

/// A time-driven scroll from one offset to another.
///
/// The offset depends only on elapsed time, so the motion is identical at any
/// frame rate; the duration grows with the distance between a floor and a
/// ceiling, which keeps a one-line step responsive and a whole-page jump calm.
#[derive(Clone, Copy, Debug)]
pub struct ScrollAnimation {
	from: f32,
	to: f32,
	started: Instant,
	pub duration: Duration,
}

impl ScrollAnimation {
	/// Starts a move to `to`, scaling the duration from `from`.
	pub fn new(from: f32, to: f32, now: Instant) -> Self {
		let ratio = ((to - from).abs() / SCROLL_FULL).clamp(0.0, 1.0);
		// Integer nanoseconds keep both bounds exact at the ends.
		let span = (SCROLL_MAX - SCROLL_MIN).as_nanos() as f64;
		let nanos = SCROLL_MIN.as_nanos() as f64 + span * f64::from(ratio);
		Self {
			from,
			to,
			started: now,
			duration: Duration::from_nanos(nanos as u64),
		}
	}

	/// The eased offset at `now`, clamped to the two ends.
	pub fn offset_at(&self, now: Instant) -> f32 {
		let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
		let progress = (elapsed / self.duration.as_secs_f32()).clamp(0.0, 1.0);
		self.from + (self.to - self.from) * ease_out_cubic(progress)
	}

	/// When the last frame is due.
	pub fn end(&self) -> Instant {
		self.started + self.duration
	}

	pub fn finished(&self, now: Instant) -> bool {
		now >= self.end()
	}
}

/// Scroll geometry currently available to a host.
#[derive(Clone, Copy, Debug)]
pub struct ScrollBounds {
	pub max: f32,
	pub complete: bool,
}

/// The momentum of a high-resolution wheel stream, in logical pixels and
/// seconds.
///
/// A touchpad on Windows hands its motion to a reader that has not opted into
/// Direct Manipulation as a few large wheel packets, each landing a quarter of
/// a second after the motion it describes. Easing every packet from a
/// standstill is what makes a fast two-finger scroll crawl and then lurch, so
/// the stream keeps a speed of its own instead: the page rides that speed
/// across the gaps between packets, and a packet settles the distance the page
/// has already run ahead by.
#[derive(Clone, Copy, Debug)]
struct Momentum {
	/// Logical pixels per second, signed like the scroll offset.
	velocity: f32,
	/// The frame the offset was last advanced to.
	at: Instant,
	/// When the last packet arrived, so a pause can stop reading speeds.
	packet: Instant,
}

/// Scroll offset, deferred destination and animation for one document.
#[derive(Clone, Debug, Default)]
pub struct ScrollState {
	/// Absolute requests may exceed incomplete bounds; `visible` clamps them.
	pub offset: f32,
	pub target: Option<f32>,
	pub animation: Option<ScrollAnimation>,
	momentum: Option<Momentum>,
}
impl ScrollState {
	pub fn visible(&self, bounds: ScrollBounds) -> f32 {
		self.offset.clamp(0.0, bounds.max)
	}
	/// Whether an eased step or a stream's momentum owns the displayed offset.
	pub fn animating(&self) -> bool {
		self.animation.is_some() || self.momentum.is_some()
	}
	pub fn resolve(&mut self, bounds: ScrollBounds) {
		if self.animating() {
			return;
		}
		if let Some(target) = self.target
			&& (bounds.complete || target <= bounds.max)
		{
			self.offset = target.clamp(0.0, bounds.max);
			self.target = None;
		}
		if bounds.complete {
			self.offset = self.offset.clamp(0.0, bounds.max);
		}
	}
	pub fn cancel(&mut self) {
		self.momentum = None;
		if let Some(animation) = self.animation.take()
			&& self.target == Some(animation.to)
		{
			self.target = None;
		}
	}
	pub fn set(&mut self, offset: f32, bounds: ScrollBounds) {
		self.cancel();
		self.target = Some(offset.max(0.0));
		// Keep the absolute request while `visible` clamps it to each prefix.
		self.offset = offset.max(0.0);
		self.resolve(bounds);
	}
	pub fn by(&mut self, delta: f32, bounds: ScrollBounds) {
		self.cancel();
		if delta == 0.0 {
			self.target.get_or_insert(self.offset);
			self.resolve(bounds);
			return;
		}
		let base = self.target.filter(|v| v.is_finite()).unwrap_or(self.offset);
		self.target = Some((base + delta).max(0.0));
		self.resolve(bounds);
	}
	pub fn animate_to(&mut self, target: f32, now: Instant) {
		self.momentum = None;
		self.target = Some(target.max(0.0));
		self.animation =
			Some(ScrollAnimation::new(self.offset, target.max(0.0), now));
	}
	pub fn animate_by(&mut self, delta: f32, now: Instant) {
		if delta == 0.0 {
			return;
		}
		let base = self.target.filter(|v| v.is_finite()).unwrap_or(self.offset);
		self.animate_to(
			(f64::from(base) + f64::from(delta)).clamp(0.0, f64::from(f32::MAX))
				as f32,
			now,
		);
	}
	pub fn wheel_by(&mut self, delta: f32, now: Instant) {
		if (self.target.unwrap_or(self.offset) - self.offset) * delta < 0.0 {
			self.target = None;
		}
		self.animate_by(delta, now);
	}
	/// A wheel travel from a high-resolution device, which Windows delivers in
	/// large packets rather than as a stream.
	///
	/// The packet is distance the hand has already travelled, so the page takes
	/// its speed from how long the packet took to arrive and keeps it for the
	/// packets still to come. Nothing here changes a wheel that reports whole
	/// detents: those keep the eased step in `wheel_by`.
	pub fn coast_wheel_by(&mut self, delta: f32, now: Instant) {
		if delta == 0.0 {
			return;
		}
		// The stream owns the displayed offset; an eased step in flight is
		// distance the packets have already accounted for.
		self.animation = None;
		let base = self
			.target
			.filter(|value| value.is_finite())
			.unwrap_or(self.offset);
		self.target = Some((base + delta).max(0.0));
		let gap = self.momentum.as_ref().map(|momentum| {
			now.saturating_duration_since(momentum.packet).as_secs_f32()
		});
		let momentum = self.momentum.get_or_insert(Momentum {
			velocity: 0.0,
			at: now,
			packet: now,
		});
		// Only a packet that follows another packet says how fast the hand is
		// moving; the first of a gesture is a distance to travel. A packet
		// after a pause begins a new gesture too, and the speed the last one
		// left behind is spent: kept, it would carry the page on the old
		// way while the new gesture's packets answer nothing, which is how
		// a reversal came to be ignored.
		if let Some(gap) = gap.filter(|gap| *gap <= PACKET_GAP) {
			// A packet against the stream's own speed is the hand reversing:
			// the speed it reverses is spent, and the packet's own rate, not
			// a blend with what it undoes, says how fast the page now moves.
			if delta * momentum.velocity < 0.0 {
				momentum.velocity = 0.0;
			}
			let rate = delta / gap.max(PACKET_MIN);
			let weight = 1.0 - (-gap / PACKET_BLEND).exp();
			momentum.velocity += (rate - momentum.velocity) * weight;
		} else {
			momentum.velocity = 0.0;
		}
		momentum.packet = now;
	}
	/// Advances a high-resolution stream's momentum.
	///
	/// The page runs at the stream's speed until the packets have paid for it,
	/// and may lead them by the distance that speed predicts; a packet whose
	/// distance the page has already covered is simply spent as it arrives.
	/// Returns whether another frame is due.
	fn advance_momentum(&mut self, now: Instant, bounds: ScrollBounds) -> bool {
		let Some(momentum) = self.momentum.as_mut() else {
			return false;
		};
		let dt = now.saturating_duration_since(momentum.at).as_secs_f32();
		momentum.at = now;
		let received = self
			.target
			.filter(|value| value.is_finite())
			.unwrap_or(self.offset);
		let owed = received - self.offset;
		let forward = if momentum.velocity == 0.0 {
			owed >= 0.0
		} else {
			momentum.velocity > 0.0
		};
		// Only a debt still to pay sets a speed; distance the page has already
		// run past is settled by the packets arriving, never by reversing.
		let debt = if forward {
			owed.max(0.0)
		} else {
			owed.min(0.0)
		};
		let chase = debt / COAST;
		let used = if forward {
			momentum.velocity.max(chase)
		} else {
			momentum.velocity.min(chase)
		};
		let limit = received + momentum.velocity * COAST;
		let (low, high) = if forward {
			(self.offset, limit.max(self.offset))
		} else {
			(limit.min(self.offset), self.offset)
		};
		self.offset = (self.offset + used * dt)
			.clamp(low, high)
			.clamp(0.0, bounds.max);
		momentum.velocity *= (-dt / COAST).exp();
		// Spent once the speed has died with no distance still owed in the
		// direction of travel: the lead the packets never paid is the
		// momentum's own, and it stays where it put the page. The chase
		// alone keeps a stream alive until the page has caught up, and a
		// page pinned at the bound the packets ran past while the layout
		// was growing is spent as well.
		if momentum.velocity.abs() <= 4.0
			&& (debt == 0.0
				|| (self.offset - received).abs() < 0.5
				|| (forward && self.offset >= bounds.max))
		{
			self.target = None;
			self.momentum = None;
			return false;
		}
		true
	}
	pub fn advance(&mut self, now: Instant, bounds: ScrollBounds) -> bool {
		if self.momentum.is_some() {
			return self.advance_momentum(now, bounds);
		}
		let Some(animation) = self.animation else {
			return false;
		};
		self.offset = animation.offset_at(now).clamp(0.0, bounds.max);
		let at_end = bounds.complete
			&& animation.to >= bounds.max - 0.5
			&& self.offset >= bounds.max - 0.5;
		if !animation.finished(now) && !at_end {
			return true;
		}
		self.animation = None;
		self.resolve(bounds);
		false
	}
	pub fn deadline(&self, now: Instant) -> Option<Instant> {
		if let Some(animation) = &self.animation {
			return Some((now + SCROLL_FRAME).min(animation.end()));
		}
		self.momentum.is_some().then_some(now + SCROLL_FRAME)
	}
}

/// A selection near a viewport edge asks for a fixed scroll step.
pub fn selection_scroll(
	y: f32,
	top: f32,
	bottom: f32,
	offset: f32,
	max: f32,
) -> f32 {
	if y < top + 24.0 && offset > 0.0 {
		-14.0
	} else if y > bottom - 24.0 && offset < max {
		14.0
	} else {
		0.0
	}
}
