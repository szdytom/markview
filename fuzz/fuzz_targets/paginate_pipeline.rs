//! P1: real layout feeding pagination with page heights derived from content.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, mutators, paginate_pipeline, pipeline};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown_after(data, size, max_size, seed, 3)
}}
fuzz_target!(|data: &[u8]| {
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	paginate_pipeline::check(data);
	guard.finish(&budget::Budget::integration().from_env(), data.len());
	coverage::tick();
});
