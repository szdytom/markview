//! G3, Tier 3: serial and parallel layout must produce identical reading scenes.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, parallel, pipeline};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

fuzz_target!(|data: &[u8]| {
	let md = String::from_utf8_lossy(data);
	if md.len() > 32 * 1024 {
		return;
	}
	pipeline::warmup();
	// Five paired passes retain both snapshots and exercise independent shapers.
	let baseline = budget::Budget::layout();
	let budget = budget::Budget {
		time_ms: baseline.time_ms * 5,
		alloc_base: baseline.alloc_base * 2,
		..baseline
	}
	.from_env();
	let guard = budget::InputGuard::new();
	parallel::check(&md);
	guard.finish(&budget, md.len());
});
