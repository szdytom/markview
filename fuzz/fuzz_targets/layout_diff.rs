//! G3, Tier 3 differentials (O5.1, O5.2): cached layout must equal a fresh
//! engine's, and a progressive prefix snapshot must equal the final
//! snapshot for the blocks it contains.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, coverage, history, mutators, oracle, pipeline};
fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	let keep = if data.starts_with(b"LDH1") { 5 } else { 0 };
	mutators::bytes_after(data, size, max_size, seed, keep)
}}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::integration().from_env();
	let md = String::from_utf8_lossy(data);
	if md.len() > 64 * 1024 {
		return;
	}
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	if data.starts_with(b"LDH1") {
		history::check(data);
		guard.finish(&budget, data.len());
		coverage::tick();
		return;
	}
	let doc = markview_commonmark::parse(md.to_string());
	let options = pipeline::options_for(&md);
	let images = markview_layout::image::ImageSnapshot::default();

	// O5.1: the second pass over a warm cache and a cold engine must agree.
	let mut warm = pipeline::differential_engine();
	let a = warm.layout(&doc, &options);
	let b = warm.layout(&doc, &options);
	let fresh = pipeline::differential_engine().layout(&doc, &options);
	assert_eq!(
		oracle::layout(&a),
		oracle::layout(&b),
		"cached layout diverged from the first pass"
	);
	assert_eq!(
		oracle::layout(&fresh),
		oracle::layout(&b),
		"cached layout diverged from a fresh engine"
	);

	// O5.2: a prefix snapshot equals the final one block for block.
	let mut progressive = pipeline::differential_engine();
	let keep = (oracle::derive(data) as usize % 3).max(1);
	let mut prefix = None;
	let final_progressive = progressive
		.layout_progressive(&doc, &options, &images, |p| {
			if p.blocks.len() == keep && prefix.is_none() {
				prefix = Some(p.clone());
			}
			true
		})
		.expect("uninterrupted layout");
	assert_eq!(
		oracle::layout(&final_progressive),
		oracle::layout(&fresh),
		"progressive final diverged from the direct layout"
	);
	if let Some(p) = prefix {
		assert!(p.blocks.len() <= fresh.blocks.len());
		for (a, b) in p.blocks.iter().zip(&fresh.blocks) {
			assert_eq!(a.id, b.id, "prefix block id diverged");
			assert_eq!(a.y, b.y, "prefix block position diverged");
			let mut h = oracle::hashers();
			oracle::block_layout(&a.layout, &mut h);
			let pa = oracle::finish(h);
			let mut h = oracle::hashers();
			oracle::block_layout(&b.layout, &mut h);
			assert_eq!(pa, oracle::finish(h), "prefix block geometry diverged");
		}
	}
	guard.finish(&budget, md.len());
});
