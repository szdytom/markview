//! Measurement helpers for hand-written probes over the ratex math path.
//!
//! The fuzz targets judge an input against a budget; a probe has to *report*
//! the numbers instead. Both read the same counting allocator
//! ([`crate::allocator::GLOBAL`]), so a figure printed here is the same
//! quantity [`crate::budget::InputGuard`] meters: the window high-water mark
//! of live bytes.
//!
//! A probe that drives an amplification must be bounded by the OS, not by
//! this process: the only lever ratex 0.1.15 exposes is the input, so run the
//! binary under `ulimit -v` (or `slot.sh`'s RSS cap) and treat an
//! allocation-failure abort as a data point rather than a lost run.

use std::time::{Duration, Instant};

use crate::allocator::GLOBAL;

/// One measured run: what came back, how long, how many bytes.
#[derive(Clone, Debug)]
pub struct Measured {
	/// Bytes of the formula handed to the entry point.
	pub input_bytes: usize,
	/// Live bytes at the window's high-water mark, minus the baseline.
	pub peak_bytes: u64,
	pub elapsed: Duration,
	/// `Err(message)`, empty on success.
	pub error: String,
}

impl Measured {
	/// Peak bytes per input byte. The number the budget compares against
	/// its per-KiB term (`1_032 * 1024` bytes per KiB = 1_032 bytes/byte).
	pub fn bytes_per_input_byte(&self) -> f64 {
		if self.input_bytes == 0 {
			return 0.0;
		}
		self.peak_bytes as f64 / self.input_bytes as f64
	}
}

/// Runs `f` inside a fresh allocation window and times it.
///
/// The window is opened before the call and read after it, so everything `f`
/// allocates and still holds is counted — the counterpart of the target's
/// guard. `T` carries whatever the caller wants to inspect (a node count, a
/// result, a layout box).
pub fn measure<T>(input_bytes: usize, f: impl FnOnce() -> T) -> (T, Measured) {
	let baseline = GLOBAL.open_window();
	let start = Instant::now();
	let value = f();
	let elapsed = start.elapsed();
	let peak = GLOBAL.window_peak().saturating_sub(baseline);
	(
		value,
		Measured {
			input_bytes,
			peak_bytes: peak,
			elapsed,
			error: String::new(),
		},
	)
}

/// [`measure`] with a label attached to the error of a `Result`.
pub fn measure_result<T, E: std::fmt::Display>(
	input_bytes: usize,
	f: impl FnOnce() -> Result<T, E>,
) -> (Result<T, String>, Measured) {
	let (value, mut m) = measure(input_bytes, f);
	let value = value.map_err(|e| {
		let message = e.to_string();
		m.error = message.clone();
		message
	});
	(value, m)
}

/// Human-readable row for a probe's output table.
pub fn row(label: &str, m: &Measured) -> String {
	format!(
		"{label:<24} in={:>8} B  peak={:>12} B  {:>7.1} B/byte  {:>9.3} s  {}",
		m.input_bytes,
		m.peak_bytes,
		m.bytes_per_input_byte(),
		m.elapsed.as_secs_f64(),
		if m.error.is_empty() {
			"Ok".to_string()
		} else {
			format!("Err({})", truncate(&m.error, 60))
		}
	)
}

fn truncate(s: &str, n: usize) -> String {
	if s.len() <= n {
		return s.to_string();
	}
	let mut end = n;
	while !s.is_char_boundary(end) {
		end -= 1;
	}
	format!("{}…", &s[..end])
}
