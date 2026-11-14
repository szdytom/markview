//! G1, Tier 3 differential (O5.3): an edit re-parsed incrementally must
//! equal a full parse of the same source, field by field. When the fast path
//! declines (`None`), `reparse` falls back to the full parse, which is equal
//! by construction; the interesting inputs are the ones where it succeeds.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, oracle, pipeline};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::reparse(data, size, max_size, seed)
}}

fn check(data: &[u8], dialect: markview_commonmark::Dialect) {
	let budget = budget::Budget::parse().from_env();
	let md0 = String::from_utf8_lossy(data).into_owned();
	if md0.len() > 256 * 1024 {
		return;
	}
	// One deterministic edit: insert/delete/replace/duplicate/truncate.
	let edit = mvfuzz::edit::apply_deterministic_edit(data);
	let md1 = edit.md1;

	let guard = budget::InputGuard::new();
	let doc0 = markview_commonmark::parse_with(md0.clone(), dialect);
	let len = md1.len();
	let full = markview_commonmark::parse_with(md1.clone(), dialect);
	let incr = markview_commonmark::reparse_with(
		&doc0,
		Arc::from(md1.as_str()),
		dialect,
	);
	assert_eq!(
		oracle::document(&full),
		oracle::document(&incr),
		"incremental reparse diverged from the full parse"
	);
	guard.finish(&budget, len);
}

fuzz_target!(|data: &[u8]| {
	if data.starts_with(b"RPS1") {
		mvfuzz::reparse_sequence::check(data);
		return;
	}
	for dialect in pipeline::dialects() {
		check(data, dialect);
	}
});
