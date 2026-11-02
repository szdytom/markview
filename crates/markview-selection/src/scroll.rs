//! Time-driven scrolling shared by the document and the chrome; the host
//! supplies an extent and a clock.
use std::time::Duration;
use web_time::Instant;

pub const SCROLL_MIN: Duration = Duration::from_millis(120);
pub const SCROLL_MAX: Duration = Duration::from_millis(400);
const SCROLL_FULL: f32 = 2400.0;
/// How often a running animation asks the event loop for a frame.
const SCROLL_FRAME: Duration = Duration::from_millis(8);

/// How long a high-resolution wheel stream's momentum takes to die away while
/// its packets still arrive, in seconds.
const COAST: f32 = 0.25;
/// How fast it dies once they stop: a hand that halts on the pad stills the
/// page within a beat, while the flush packets a lift produces, which arrive
/// after a longer silence, still find some speed left to continue.
const QUIET_DECAY: f32 = 0.1;
/// Silence this long makes a stream quiet. Nothing announces that a hand has
/// stopped or lifted, and on a batchy touchpad the gaps inside one gesture run
/// to half a second, so this only switches the decay — it never ends the
/// stream outright. The spacing is bimodal — packets land within tens of
/// milliseconds, or a batch gap opens past two hundred — so this sits between
/// the two and touches neither.
const QUIET: f32 = 0.06;
/// The fastest the page may chase distance the packets have named, so one
/// large flush packet lands as a glide instead of a jump.
const CHASE_MAX: f32 = 8000.0;
/// The share of the recent packet spacing the lead is worth, bounded either
/// way: a dense stream keeps the page on the packets' heels, a sparse one
/// bridges its gaps.
const LEAD_GAP: f32 = 1.5;
const LEAD_MIN: f32 = 0.03;
const LEAD_MAX: f32 = 0.25;
/// The shortest gap a packet's own speed is read over, so a timer that never
/// advanced cannot claim an unbounded rate.
const PACKET_MIN: f32 = 0.008;
/// How much of a packet's own speed one reading takes in.
const PACKET_BLEND: f32 = 0.05;
/// A silence this long breaks the stream: the accounting restarts from the
/// displayed offset, the carried speed is believed no faster than the
/// silence's own average, and no rate is read across it.
const PACKET_GAP: f32 = 0.15;
/// How long after its last packet a stream still adopts whole-detent events,
/// which the OS rounds out of a fractional stream mid-gesture.
const STREAM_WINDOW: f32 = 0.5;

/// Ease-out cubic: fast away from the start and settling into the target.
/// Both ends are exact and the curve is strictly increasing between them.
pub fn ease_out_cubic(t: f32) -> f32 {
	let remaining = 1.0 - t.clamp(0.0, 1.0);
	1.0 - remaining * remaining * remaining
}

/// How one wheel event should move the offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WheelMode {
	/// A whole-detent step, eased to its destination.
	#[default]
	Detent,
	/// A packet from a high-resolution stream, which carries its own momentum.
	Packet,
	/// Input that already carries the whole motion, such as a native stream or
	/// a pointer pan. It is applied without easing or a coast.
	Direct,
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

impl Default for ScrollBounds {
	fn default() -> Self {
		Self {
			max: 0.0,
			complete: true,
		}
	}
}

/// The momentum of a high-resolution wheel stream, in logical pixels and
/// seconds.
///
/// A touchpad on Windows hands its motion to a reader that has not opted into
/// Direct Manipulation in batches: dense packets while the hand cruises, and
/// after any silence a flush carrying what the OS accumulated meanwhile. The
/// stream therefore keeps a speed of its own, read from the spacing of its
/// packets, and the page rides it across the gaps; a packet after a silence
/// continues the stream rather than restarting it.
#[derive(Clone, Copy, Debug)]
struct Momentum {
	/// Logical pixels per second, signed like the scroll offset.
	velocity: f32,
	/// The frame the offset was last advanced to.
	at: Instant,
	/// When the last packet arrived, so a quiet stream can decay faster.
	packet: Instant,
	/// The recent spacing of packets, in seconds, which sizes the lead.
	gap_ema: f32,
}

/// One scrollable surface: offset, extent, deferred destination and animation.
///
/// The extent is stored rather than passed on every call, so every surface —
/// the document, a drawer or a list — advances through one clock, one wheel
/// path and one set of bounds.
#[derive(Clone, Debug, Default)]
pub struct Scroll {
	/// Absolute requests may exceed incomplete bounds; `offset` clamps them.
	pub offset: f32,
	pub target: Option<f32>,
	pub animation: Option<ScrollAnimation>,
	bounds: ScrollBounds,
	momentum: Option<Momentum>,
}

impl Scroll {
	/// Replaces the available geometry, clamping a resting offset to it.
	pub fn fit(&mut self, bounds: ScrollBounds) {
		self.bounds = ScrollBounds {
			max: bounds.max.max(0.0),
			complete: bounds.complete,
		};
		self.resolve();
	}
	/// Replaces the bound of a surface whose whole geometry is known.
	pub fn set_max(&mut self, max: f32) {
		self.fit(ScrollBounds {
			max,
			complete: true,
		});
	}
	/// The offset actually on screen.
	pub fn offset(&self) -> f32 {
		self.offset.clamp(0.0, self.bounds.max)
	}
	/// Whether an eased step or a stream's momentum owns the displayed offset.
	pub fn animating(&self) -> bool {
		self.animation.is_some() || self.momentum.is_some()
	}
	/// Resolves a deferred destination against the stored extent.
	pub fn resolve(&mut self) {
		if self.animating() {
			return;
		}
		if let Some(target) = self.target
			&& (self.bounds.complete || target <= self.bounds.max)
		{
			self.offset = target.clamp(0.0, self.bounds.max);
			self.target = None;
		}
		if self.bounds.complete {
			self.offset = self.offset.clamp(0.0, self.bounds.max);
		}
	}
	pub fn cancel(&mut self) {
		if self.momentum.take().is_some() {
			self.target = None;
		}
		if let Some(animation) = self.animation.take()
			&& self.target == Some(animation.to)
		{
			self.target = None;
		}
	}
	pub fn reset(&mut self) {
		self.cancel();
		self.offset = 0.0;
		self.target = None;
	}
	pub fn set(&mut self, offset: f32) {
		self.cancel();
		self.target = Some(offset.max(0.0));
		// Keep the absolute request while `offset` clamps it to each prefix.
		self.offset = offset.max(0.0);
		self.resolve();
	}
	pub fn by(&mut self, delta: f32) {
		self.cancel();
		if delta == 0.0 {
			self.target.get_or_insert(self.offset);
			self.resolve();
			return;
		}
		let base = self.target.filter(|v| v.is_finite()).unwrap_or(self.offset);
		self.target = Some((base + delta).max(0.0));
		self.resolve();
	}
	pub fn animate_to(&mut self, target: f32, now: Instant) {
		self.momentum = None;
		let target = target.max(0.0);
		if target == self.offset {
			self.animation = None;
			self.target = None;
			return;
		}
		self.target = Some(target);
		self.animation = Some(ScrollAnimation::new(self.offset, target, now));
	}
	pub fn animate_by(&mut self, delta: f32, now: Instant) {
		if delta == 0.0 {
			return;
		}
		if self.momentum.is_some() {
			self.cancel();
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
	/// One wheel event, in the mode the host read from the device.
	pub fn wheel(&mut self, delta: f32, now: Instant, mode: WheelMode) {
		let mode = match mode {
			WheelMode::Detent if self.wheel_stream_alive(now) => {
				WheelMode::Packet
			}
			mode => mode,
		};
		match mode {
			WheelMode::Detent => self.wheel_by(delta, now),
			WheelMode::Packet => self.coast_wheel_by(delta, now),
			WheelMode::Direct => self.by(delta),
		}
	}
	/// A wheel travel from a high-resolution device, which Windows delivers in
	/// batches rather than as a stream.
	///
	/// The packet is distance the hand has already travelled, so the page takes
	/// its speed from how long the packet took to arrive and keeps it for the
	/// packets still to come. A packet that follows a silence continues the
	/// stream: the accounting restarts from the displayed offset, the decaying
	/// speed is bridged across the silence, and the silence's own distance
	/// names the fastest speed the hand can be believed to have had. Only a
	/// packet against the stream's own speed is a new gesture: the speed it
	/// reverses is spent, and its own rate, not a blend with what it undoes,
	/// moves the page. Nothing here changes a wheel that reports whole detents
	/// on its own: those keep the eased step in `wheel_by`.
	pub fn coast_wheel_by(&mut self, delta: f32, now: Instant) {
		if delta == 0.0 {
			return;
		}
		let gap = self.momentum.as_ref().map(|momentum| {
			now.saturating_duration_since(momentum.packet).as_secs_f32()
		});
		let reversing = self.momentum.as_ref().is_some_and(|momentum| {
			let direction = if momentum.velocity == 0.0 {
				self.target.unwrap_or(self.offset) - self.offset
			} else {
				momentum.velocity
			};
			delta * direction < 0.0
		});
		// A reversal starts from the displayed offset, dropping both the
		// stream's speed and the target its earlier packets left behind. A
		// pause drops the target too — new input is followed from where the
		// page is — but keeps the decaying speed, which bridges the silences
		// a batchy stream leaves between its bursts.
		if self.momentum.is_some() && reversing {
			self.cancel();
		} else if let Some(gap) = gap.filter(|gap| *gap > PACKET_GAP) {
			self.target = None;
			if let Some(momentum) = self.momentum.as_mut() {
				// Spend unrendered quiet time before bridging the speed. New
				// travel starts at its arrival, not at the preceding frame.
				let dt =
					now.saturating_duration_since(momentum.at).as_secs_f32();
				momentum.velocity *= (-dt / QUIET_DECAY).exp();
				momentum.at = now;
				// The silence's own distance is the fastest the hand can be
				// believed to have gone, and nothing more.
				let average = (delta / gap).abs();
				momentum.velocity = momentum.velocity.clamp(-average, average);
			}
		}
		// The stream owns the displayed offset; an eased step in flight is
		// distance the packets have already accounted for.
		self.animation = None;
		let base = self
			.target
			.filter(|value| value.is_finite())
			.unwrap_or(self.offset);
		self.target = Some((base + delta).max(0.0));
		let momentum = self.momentum.get_or_insert(Momentum {
			velocity: 0.0,
			at: now,
			packet: now,
			// A stream that has just begun has one packet's spacing and
			// nothing larger; the lead grows only if the packets space out.
			gap_ema: LEAD_MIN,
		});
		// Only nearby packets supply a rate — though no faster than the
		// reader is willing to chase, for the OS hands a flush over as a
		// dense burst of large packets whose distance-over-gap names speeds
		// no finger ever reached.
		if let Some(gap) = gap.filter(|gap| *gap <= PACKET_GAP) {
			let rate =
				(delta / gap.max(PACKET_MIN)).clamp(-CHASE_MAX, CHASE_MAX);
			let weight = 1.0 - (-gap / PACKET_BLEND).exp();
			momentum.velocity += (rate - momentum.velocity) * weight;
		}
		if let Some(gap) = gap {
			momentum.gap_ema += (gap.min(LEAD_MAX) - momentum.gap_ema) * 0.3;
		}
		momentum.packet = now;
	}
	/// Advances a high-resolution stream's momentum.
	///
	/// While packets arrive, the page runs at the stream's speed and may lead
	/// them by the distance that speed predicts over their own spacing. Once
	/// they go quiet the lead is lifted and the speed dies at the faster quiet
	/// rate, so a hand that halts on the pad stills the page within a beat
	/// while a lift's flush, arriving later, finds speed left to continue.
	/// Returns whether another frame is due.
	fn advance_momentum(&mut self, now: Instant) -> bool {
		let Some(momentum) = self.momentum.as_mut() else {
			return false;
		};
		let dt = now.saturating_duration_since(momentum.at).as_secs_f32();
		momentum.at = now;
		let quiet =
			now.saturating_duration_since(momentum.packet).as_secs_f32()
				> QUIET;
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
		let chase = (debt / COAST).clamp(-CHASE_MAX, CHASE_MAX);
		let used = if forward {
			momentum.velocity.max(chase)
		} else {
			momentum.velocity.min(chase)
		};
		// A quiet stream has nothing left to lead; its page travels on speed
		// alone, either way the debt or the momentum points, bounded by a
		// tail of the scale the speed decays over — so a stalled frame
		// cannot fling the page past the packets by seconds of chase. A
		// live one is bounded by the lead its own packet spacing pays.
		let (low, high) = if quiet {
			let limit = received + momentum.velocity * QUIET_DECAY;
			if forward {
				(self.offset, limit.max(self.offset))
			} else {
				(limit.min(self.offset), self.offset)
			}
		} else {
			let lead = (momentum.gap_ema * LEAD_GAP).clamp(LEAD_MIN, LEAD_MAX);
			let limit = received + momentum.velocity * lead;
			if forward {
				(self.offset, limit.max(self.offset))
			} else {
				(limit.min(self.offset), self.offset)
			}
		};
		self.offset = (self.offset + used * dt)
			.clamp(low, high)
			.clamp(0.0, self.bounds.max);
		let decay = if quiet { QUIET_DECAY } else { COAST };
		momentum.velocity *= (-dt / decay).exp();
		// Spent once the speed has died with nothing still owed in the
		// direction of travel — the lead and the quiet tail the packets never
		// paid are the momentum's own, and they stay where they put the page.
		// The chase alone keeps a stream alive until the page has caught up,
		// and a page pinned at the bound the packets ran past while the
		// layout was growing is spent as well.
		let settled = debt == 0.0
			|| (self.offset - received).abs() < 0.5
			|| (forward && self.offset >= self.bounds.max);
		if momentum.velocity.abs() <= 4.0 && settled {
			self.target = None;
			self.momentum = None;
			return false;
		}
		true
	}
	/// Whether a wheel packet stream is live enough that a whole-detent event
	/// belongs to it: the OS rounds a fractional stream's odd packets to whole
	/// lines mid-gesture, and easing one of those from a standstill would kill
	/// the stream's momentum.
	pub fn wheel_stream_alive(&self, now: Instant) -> bool {
		self.momentum.as_ref().is_some_and(|momentum| {
			now.saturating_duration_since(momentum.packet).as_secs_f32()
				<= STREAM_WINDOW
		})
	}
	pub fn advance(&mut self, now: Instant) -> bool {
		if self.momentum.is_some() {
			return self.advance_momentum(now);
		}
		let Some(animation) = self.animation else {
			return false;
		};
		self.offset = animation.offset_at(now).clamp(0.0, self.bounds.max);
		let at_end = self.bounds.complete
			&& animation.to >= self.bounds.max - 0.5
			&& self.offset >= self.bounds.max - 0.5;
		if !animation.finished(now) && !at_end {
			return true;
		}
		self.animation = None;
		self.resolve();
		false
	}
	pub fn deadline(&self, now: Instant) -> Option<Instant> {
		if let Some(animation) = &self.animation {
			return Some((now + SCROLL_FRAME).min(animation.end()));
		}
		self.momentum.is_some().then_some(now + SCROLL_FRAME)
	}
}

impl From<f32> for Scroll {
	fn from(offset: f32) -> Self {
		let mut scroll = Self::default();
		scroll.fit(ScrollBounds {
			max: f32::MAX,
			complete: false,
		});
		scroll.offset = offset.max(0.0);
		scroll
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
