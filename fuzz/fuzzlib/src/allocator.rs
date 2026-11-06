//! A counting global allocator backing the peak-allocation oracle.
//!
//! Peak here means the high-water mark of live bytes (`allocated - freed`),
//! measured by the harness itself rather than reading system memory (O3).
//! [`CountingAllocator::open_window`] starts a fresh window: it returns the
//! live-byte baseline and resets the *window* peak, so
//! [`CountingAllocator::window_peak`] then tracks the high-water mark of
//! live bytes **within that window** — per input, not per process. The
//! process-global `peak` stays as a historical figure.
//!
//! Resetting the window is racy: an allocation straddling the reset may be
//! folded in just before it is overwritten and so missed. That is acceptable
//! for a budget guard — the fuzzer target thread is the only party that
//! matters, and the detached highlight worker's spillover is deliberately
//! tolerated via the 2.5x layout factor. The fuzzer runs single-threaded;
//! the background worker may still inflate a window slightly. That
//! conservatism is fine: the budget is a guard, not a measurement.

use std::{
	alloc::{GlobalAlloc, Layout, System},
	sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
pub struct CountingAllocator {
	current: AtomicUsize,
	peak: AtomicUsize,
	window_peak: AtomicUsize,
	allocations: AtomicUsize,
	live_allocations: AtomicUsize,
}

impl CountingAllocator {
	pub const fn new() -> Self {
		Self {
			current: AtomicUsize::new(0),
			peak: AtomicUsize::new(0),
			window_peak: AtomicUsize::new(0),
			allocations: AtomicUsize::new(0),
			live_allocations: AtomicUsize::new(0),
		}
	}
	pub fn allocations(&self) -> usize {
		self.allocations.load(Ordering::Relaxed)
	}
	pub fn live_allocations(&self) -> usize {
		self.live_allocations.load(Ordering::Relaxed)
	}
	/// Live bytes right now.
	pub fn current(&self) -> u64 {
		self.current.load(Ordering::Relaxed) as u64
	}
	/// The highest live-byte count since process start.
	pub fn peak(&self) -> u64 {
		self.peak.load(Ordering::Relaxed) as u64
	}
	/// Starts a metering window: resets the window peak to the live total
	/// and returns that total as the baseline. A guard's spend is then
	/// `window_peak - baseline`.
	///
	/// Racy against concurrent allocation (see module docs), acceptable for
	/// a budget guard.
	pub fn open_window(&self) -> u64 {
		let base = self.current();
		self.window_peak.store(base as usize, Ordering::Relaxed);
		base
	}
	/// The high-water mark of live bytes since the last [`Self::open_window`].
	pub fn window_peak(&self) -> u64 {
		self.window_peak.load(Ordering::Relaxed) as u64
	}
}
// SAFETY: Requests preserve the system allocator's pointer/layout contract; counters only observe sizes.
unsafe impl GlobalAlloc for CountingAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		// SAFETY: `layout` is forwarded unchanged to the system allocator.
		let ptr = unsafe { System.alloc(layout) };
		if !ptr.is_null() {
			self.allocations.fetch_add(1, Ordering::Relaxed);
			self.live_allocations.fetch_add(1, Ordering::Relaxed);
			let live = self.current.fetch_add(layout.size(), Ordering::Relaxed);
			self.peak.fetch_max(live + layout.size(), Ordering::Relaxed);
			self.window_peak
				.fetch_max(live + layout.size(), Ordering::Relaxed);
		}
		ptr
	}
	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		self.live_allocations.fetch_sub(1, Ordering::Relaxed);
		self.current.fetch_sub(layout.size(), Ordering::Relaxed);
		// SAFETY: `ptr` and `layout` satisfy the caller's `GlobalAlloc` contract.
		unsafe { System.dealloc(ptr, layout) }
	}
}

/// The process-wide allocator. Declared here so every target and tool in
/// this crate counts into the same instance the [`crate::budget::InputGuard`]
/// reads; a per-binary `#[global_allocator]` would be a *different* counter
/// that never gets observed.
#[global_allocator]
pub static GLOBAL: CountingAllocator = CountingAllocator::new();
