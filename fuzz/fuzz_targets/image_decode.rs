//! P1: real image bytes through the production decoder and a PDF consumer.
#![no_main]
use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, image_decode, mutators};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"IDC1") { 16 } else { 0 };
	mutators::bytes_after(data, size, max_size, seed, keep)
}}
fuzz_target!(|data: &[u8]| {
	if data.len() > 256 * 1024 {
		return;
	}
	let guard = budget::InputGuard::new();
	image_decode::check(data);
	guard.finish(&budget::Budget::image().from_env(), data.len());
	coverage::tick();
});
