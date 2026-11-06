//! Search-to-copy integration with pinned fonts and bounded reflow histories.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, mutators, pipeline, reading};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"RDP1") || data.starts_with(b"RDB1") { 12 } else { 0 };
	mutators::markdown_after(data, size, max_size, seed, keep)
}}
fuzz_target!(|data: &[u8]| {
	if data.len() > 64 * 1024 {
		return;
	}
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	reading::check(data);
	guard.finish(&budget::Budget::integration().from_env(), data.len());
	coverage::tick();
});
