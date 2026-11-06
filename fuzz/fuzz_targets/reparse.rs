//! G1, Tier 3 differential (O5.3): an edit re-parsed incrementally must
//! equal a full parse of the same source, field by field. When the fast path
//! declines (`None`), `reparse` falls back to the full parse, which is equal
//! by construction; the interesting inputs are the ones where it succeeds.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, oracle};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	if data.starts_with(b"RPS1") {
		mvfuzz::reparse_sequence::check(data);
		return;
	}
	let md0 = String::from_utf8_lossy(data).into_owned();
	if md0.len() > 256 * 1024 {
		return;
	}
	// One deterministic edit: insert/delete/replace/duplicate/truncate.
	let edit = mvfuzz::edit::apply_deterministic_edit(data);
	let md1 = edit.md1;

	let guard = budget::InputGuard::new();
	let doc0 = markview_commonmark::parse(md0);
	let len = md1.len();
	let full = markview_commonmark::parse(md1.clone());
	let incr = markview_commonmark::reparse(&doc0, Arc::from(md1));
	assert_eq!(
		oracle::document(&full),
		oracle::document(&incr),
		"incremental reparse diverged from the full parse"
	);
	guard.finish(&budget, len);
});
