//! D: valid and damaged font archives through the shared extraction core.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, font_archive, mutators};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"ARC1") { 18 } else { 0 };
	mutators::bytes_after(data, size, max_size, seed, keep)
}}
fuzz_target!(|data: &[u8]| {
	let guard = budget::InputGuard::new();
	font_archive::check(data);
	guard.finish(&budget::Budget::parse().from_env(), data.len());
	coverage::tick();
});
