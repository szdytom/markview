//! Extraction and direct span grids consumed by the reader and paginator.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, mutators, pipeline, web_layout};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown_after(data, size, max_size, seed, 1)
}}
fuzz_target!(|data: &[u8]| {
	if data.len() > 8192 {
		return;
	}
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	web_layout::check(data);
	guard.finish(&budget::Budget::integration().from_env(), data.len());
	coverage::tick();
});
