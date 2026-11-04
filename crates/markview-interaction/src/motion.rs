//! Gesture velocity and release inertia.
type Point = (f32, f32);
/// Finger velocity and exponential coast in logical pixels and seconds.
pub struct Motion {
	pub velocity: Point,
	at: web_time::Instant,
	last_motion: web_time::Instant,
}

impl Motion {
	pub fn new(now: web_time::Instant) -> Self {
		Self {
			velocity: (0.0, 0.0),
			at: now,
			last_motion: now,
		}
	}

	pub fn sample(&mut self, delta: Point, now: web_time::Instant) {
		let elapsed = now.duration_since(self.at).as_secs_f32();
		self.at = now;
		if delta == (0.0, 0.0) || elapsed <= 0.0 {
			return;
		}
		let weight = 1.0 - (-elapsed / 0.04).exp();
		self.velocity.0 += (delta.0 / elapsed - self.velocity.0) * weight;
		self.velocity.1 += (delta.1 / elapsed - self.velocity.1) * weight;
		self.velocity.0 = self.velocity.0.clamp(-3000.0, 3000.0);
		self.velocity.1 = self.velocity.1.clamp(-3000.0, 3000.0);
		self.last_motion = now;
	}

	pub fn release(&mut self, now: web_time::Instant) -> bool {
		self.at = now;
		now.duration_since(self.last_motion).as_millis() < 80
			&& self.velocity.0.hypot(self.velocity.1) >= 60.0
	}

	pub fn advance(&mut self, now: web_time::Instant) -> Option<Point> {
		let elapsed = now.duration_since(self.at).as_secs_f32();
		self.at = now;
		if elapsed > 0.1 || self.velocity.0.hypot(self.velocity.1) < 10.0 {
			return None;
		}
		let decay = (-elapsed / 0.22).exp();
		let distance = 0.22 * (1.0 - decay);
		let delta = (self.velocity.0 * distance, self.velocity.1 * distance);
		self.velocity.0 *= decay;
		self.velocity.1 *= decay;
		Some(delta)
	}
}
