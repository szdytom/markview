//! D: real layout snapshots driven through selection, scroll, pan and overflow.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, interaction_sequence, mutators, pipeline};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"INT1") { 16 } else { 0 };
	mutators::markdown_after(data, size, max_size, seed, keep)
}}
fuzz_target!(|data: &[u8]| {
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	interaction_sequence::check(data);
	guard.finish(&budget::Budget::integration().from_env(), data.len());
	coverage::tick();
});
