//! Per-input budgets (O2, O3, B1).
//!
//! Every target wraps one input in an [`InputGuard`]: no panic, abort, or
//! integer overflow may occur, and neither the wall-clock time nor the
//! input's peak live allocation may exceed the budget. `Err` returns and
//! degraded output are legal (O4: the `Limits` degradation contract is not
//! asserted here).
//!
//! The allocation figure is a *window* peak: opening the guard snapshots
//! live bytes and resets the allocator's window peak, so each input is
//! metered on its own transient high-water mark — never on the
//! process-global peak, which earlier larger inputs would otherwise mask.
//! The wall budget detects *slow* inputs, not hung ones; a post-hoc
//! `finish` cannot observe a hang, so libFuzzer's `-timeout` is the hang
//! backstop.
//!
//! The per-stage defaults are calibrated from representative-input
//! measurements: the high percentile times a safety factor, plus a linear
//! term when the peak grows with input length (`cargo run -p mvfuzz --bin
//! calibrate` re-derives them). Long campaigns may override them through the
//! `MARKVIEW_FUZZ_*` environment variables (B1).

use std::time::Instant;

use crate::allocator::GLOBAL;

#[derive(Clone, Copy, Debug)]
pub struct Budget {
	/// Wall-clock allowance for one input, in milliseconds.
	pub time_ms: u64,
	/// Fixed part of the peak-allocation allowance, in bytes.
	pub alloc_base: u64,
	/// Part of the allowance that scales with input length, in bytes per KiB
	/// of input.
	pub alloc_per_kib: u64,
}

impl Budget {
	/// The calibrated parse budget: the parse, reparse, mvss, math, fonts,
	/// shaping, and geometry targets measure against it. The wall figures
	/// carry a factor of two over the calibrated maximum: the wall clock
	/// sees the campaign's own scheduling load and one-time process
	/// warm-up, and a small input descheduled past the calibrated figure is
	/// not a runaway. The allocation figures stay the calibrated ones,
	/// which the load does not inflate.
	pub fn parse() -> Self {
		Self {
			time_ms: 2_000,
			alloc_base: 4_066 * 1024,
			alloc_per_kib: 1_032 * 1024,
		}
	}
	/// The calibrated layout budget: layout, layout_diff, and highlight. The
	/// allocation figure carries a factor of 2.5 over the calibrated
	/// maximum rather than 2: the highlight worker runs detached, so a
	/// large input's tail can still be allocating while the next input's
	/// guard is open, and the overlap lands on that input's account.
	pub fn layout() -> Self {
		Self {
			time_ms: 8_000,
			alloc_base: 32_000 * 1024,
			alloc_per_kib: 1_027 * 1024,
		}
	}
	/// The calibrated PDF budget: the export target.
	pub fn pdf() -> Self {
		Self {
			time_ms: 8_000,
			alloc_base: 26_673 * 1024,
			alloc_per_kib: 1_029 * 1024,
		}
	}
	/// Decoding may allocate the full RGBA output for a legal 16-million-pixel
	/// image (64 MiB) plus decoder working state. This is intentionally its own
	/// stage, not the layout window.
	pub fn image() -> Self {
		Self {
			time_ms: 8_000,
			alloc_base: 160 * 1024 * 1024,
			alloc_per_kib: 1_024 * 1024,
		}
	}
	/// Math formulas may expand to their configured `math_bytes` limit, which
	/// is larger than the parse window for a tiny source formula.
	pub fn math() -> Self {
		Self {
			time_ms: 5_000,
			alloc_base: 32 * 1024 * 1024,
			alloc_per_kib: 1_024 * 1024,
		}
	}
	/// Integration targets do several real layouts, searches, paginations or
	/// state transitions per input. Their fixed work is larger than one layout
	/// pass, so the wall and fixed-allocation windows are wider; libFuzzer's
	/// own timeout remains the hang backstop.
	pub fn integration() -> Self {
		Self {
			time_ms: 30_000,
			alloc_base: 128 * 1024 * 1024,
			alloc_per_kib: 1_024 * 1024,
		}
	}
	/// [`Self::parse`], the lightest stage; keep `Default` on it.
	pub fn calibrated() -> Self {
		Self::parse()
	}
	/// `self` with the `MARKVIEW_FUZZ_TIME_MS`,
	/// `MARKVIEW_FUZZ_ALLOC_BASE_KB`, and `MARKVIEW_FUZZ_ALLOC_KB_PER_KB`
	/// overrides applied, so a long campaign can relax or tighten budgets
	/// without a rebuild.
	pub fn from_env(self) -> Self {
		let mut out = self;
		if let Some(v) = env("MARKVIEW_FUZZ_TIME_MS")
			&& let Ok(v) = v.parse()
		{
			out.time_ms = v;
		}
		if let Some(v) = env("MARKVIEW_FUZZ_ALLOC_BASE_KB")
			&& let Ok(v) = v.parse::<u64>()
		{
			out.alloc_base = v * 1024;
		}
		if let Some(v) = env("MARKVIEW_FUZZ_ALLOC_KB_PER_KB")
			&& let Ok(v) = v.parse::<u64>()
		{
			out.alloc_per_kib = v * 1024;
		}
		out
	}
	/// The peak-allocation allowance for an input of this many bytes.
	pub fn alloc_limit(&self, input_bytes: usize) -> u64 {
		self.alloc_base.saturating_add(
			(input_bytes as u64).div_ceil(1024) * self.alloc_per_kib,
		)
	}
}

impl Default for Budget {
	fn default() -> Self {
		Self::calibrated()
	}
}

fn env(key: &str) -> Option<String> {
	std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// One budgeted input. Wraps the work and fails loudly if either budget is
/// exceeded, which libFuzzer records as a regression test.
pub struct InputGuard {
	start: Instant,
	alloc_base: u64,
}

impl InputGuard {
	pub fn new() -> Self {
		// Opening a window both snapshots the live baseline (what was
		// already resident before this input is not its fault) and resets
		// the window peak, so this input is metered on its own
		// transient high-water mark, not the process-global one.
		Self {
			start: Instant::now(),
			alloc_base: GLOBAL.open_window(),
		}
	}
	pub fn finish(self, budget: &Budget, input_bytes: usize) {
		let elapsed = self.start.elapsed();
		if elapsed.as_millis() > u128::from(budget.time_ms) {
			panic!(
				"wall budget: {elapsed:?} for a {input_bytes}-byte input exceeds {budget:?}"
			);
		}
		let spent = GLOBAL.window_peak().saturating_sub(self.alloc_base);
		let limit = budget.alloc_limit(input_bytes);
		if spent > limit {
			panic!(
				"allocation budget: one {input_bytes}-byte input grew live memory by \
				 {spent} bytes, allowance {limit}"
			);
		}
	}
}

impl Default for InputGuard {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// The two allocation-gate tests share one process-global allocator, so
	// they serialize to keep the window state deterministic. Lock poisoning
	// is expected here (a panicking test holds the guard); exclusion, not
	// the mutex state, is what matters.
	static ALLOC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

	fn alloc_test_lock() -> std::sync::MutexGuard<'static, ()> {
		ALLOC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
	}

	// The reverse test that pins the allocation gate: with a zero allowance
	// a live allocation inside the guard must panic, or the gate is inert.
	#[test]
	#[should_panic(expected = "allocation budget")]
	fn a_zero_alloc_allowance_is_enforced() {
		let _lock = alloc_test_lock();
		let guard = InputGuard::new();
		let chunk = vec![0u8; 64 * 1024];
		guard.finish(
			&Budget {
				time_ms: 60_000,
				alloc_base: 0,
				alloc_per_kib: 0,
			},
			chunk.len(),
		);
	}

	// Regression pin for window metering: a large block allocated and
	// dropped *outside* any guard raises the process-global peak; a later
	// guard's window starts from the live baseline, so its own modest
	// allocation — far below the historical peak but over a zero allowance
	// — must still panic. Under the old global-peak delta this was silent
	// (delta 0).
	#[test]
	#[should_panic(expected = "allocation budget")]
	fn a_window_peak_below_the_global_peak_is_still_metered() {
		let _lock = alloc_test_lock();
		let _big = vec![0u8; 32 * 1024 * 1024];
		drop(_big);
		let guard = InputGuard::new();
		let chunk = vec![0u8; 2 * 1024 * 1024];
		guard.finish(
			&Budget {
				time_ms: 60_000,
				alloc_base: 0,
				alloc_per_kib: 0,
			},
			chunk.len(),
		);
	}
}
