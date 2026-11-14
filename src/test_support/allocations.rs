//! Process-wide Rust allocator counters for isolated diagnostic tests.
#![allow(unsafe_code)]
use serde::Serialize;
use std::{
	alloc::{GlobalAlloc, Layout, System},
	sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Stats {
	pub calls: usize,
	pub requested_bytes: usize,
	pub peak_extra_live_bytes: usize,
	pub retained_delta_bytes: i64,
}

fn account(bytes: usize) {
	let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
	if ENABLED.load(Relaxed) {
		PEAK.fetch_max(live, Relaxed);
		CALLS.fetch_add(1, Relaxed);
		BYTES.fetch_add(bytes, Relaxed);
	}
}

struct Counting;
// SAFETY: allocation ownership, layouts and pointers are forwarded unchanged
// to `System`; the counters only observe requested sizes.
unsafe impl GlobalAlloc for Counting {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		// SAFETY: the caller supplies a valid allocation `layout`.
		let ptr = unsafe { System.alloc(layout) };
		if !ptr.is_null() {
			account(layout.size());
		}
		ptr
	}
	unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
		// SAFETY: the caller supplies a valid allocation `layout`.
		let ptr = unsafe { System.alloc_zeroed(layout) };
		if !ptr.is_null() {
			account(layout.size());
		}
		ptr
	}
	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		LIVE.fetch_sub(layout.size(), Relaxed);
		// SAFETY: the caller returns a live `System` pointer with its layout.
		unsafe { System.dealloc(ptr, layout) };
	}
	unsafe fn realloc(
		&self,
		ptr: *mut u8,
		layout: Layout,
		size: usize,
	) -> *mut u8 {
		// SAFETY: the caller supplies a live allocation and a valid new size.
		let out = unsafe { System.realloc(ptr, layout, size) };
		if !out.is_null() {
			LIVE.fetch_sub(layout.size(), Relaxed);
			account(size);
		}
		out
	}
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct Guard;
impl Drop for Guard {
	fn drop(&mut self) {
		ENABLED.store(false, Relaxed);
	}
}

/// Includes every Rust thread; run a single test with `--test-threads=1`.
pub(crate) fn measure<T>(run: impl FnOnce() -> T) -> (T, Stats) {
	assert!(!ENABLED.load(Relaxed), "allocation scopes cannot nest");
	let baseline = LIVE.load(Relaxed);
	PEAK.store(baseline, Relaxed);
	CALLS.store(0, Relaxed);
	BYTES.store(0, Relaxed);
	ENABLED.store(true, Relaxed);
	let guard = Guard;
	let result = run();
	drop(guard);
	(
		result,
		Stats {
			calls: CALLS.load(Relaxed),
			requested_bytes: BYTES.load(Relaxed),
			peak_extra_live_bytes: PEAK.load(Relaxed).saturating_sub(baseline),
			retained_delta_bytes: LIVE.load(Relaxed) as i64 - baseline as i64,
		},
	)
}

#[test]
#[ignore = "process-wide allocator counters require an isolated test process"]
fn counters_observe_live_payload_and_reallocation() {
	let (payload, stats) = measure(|| {
		let mut bytes = Vec::with_capacity(std::hint::black_box(64));
		bytes.resize(4096, std::hint::black_box(7_u8));
		std::hint::black_box(bytes)
	});
	assert!(payload.iter().all(|byte| *byte == 7));
	assert!(stats.calls >= 2);
	assert!(stats.requested_bytes >= 4160);
	assert!(stats.peak_extra_live_bytes >= 4096);
	assert!(stats.retained_delta_bytes >= 4096);
}
