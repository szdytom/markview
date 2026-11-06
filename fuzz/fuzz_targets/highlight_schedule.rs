//! G4, Tier 3: controlled background highlighting converges to a direct engine.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, highlight_schedule, mutators, pipeline};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"HLP1") { 12 } else { 0 };
	mutators::bytes_after(data, size, max_size, seed, keep)
}}
fuzz_target!(|data: &[u8]| {
	if data.len() > 16 * 1024 {
		return;
	}
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	highlight_schedule::check(data);
	guard.finish(&budget::Budget::integration().from_env(), data.len());
	coverage::tick();
});
