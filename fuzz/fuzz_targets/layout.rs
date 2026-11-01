//! G3, Tier 1: the whole layout pipeline — line breaking (optimal and
//! greedy), paragraphs, microtype, tables, scene production — with every
//! `Limits` budget derived toward its floor. Oracle (O1, O2, I7): no panic
//! within budgets, and every produced geometry value finite and bounded.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mvfuzz::{budget, oracle, pipeline};

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::layout().from_env();
	let md = String::from_utf8_lossy(data);
	if md.len() > 128 * 1024 {
		return;
	}
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	let (_doc, _options, snapshot) = pipeline::parse_layout(&md);
	oracle::assert_snapshot_finite(&snapshot);
	guard.finish(&budget, md.len());
});
