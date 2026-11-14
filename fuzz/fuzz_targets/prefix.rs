//! G1, Tier 3 differential: a prefix parse must be the opening of the full
//! parse. `parse_prefix` exists so a reader can show the first blocks of a
//! large file without waiting for the whole parse, and the full parse then
//! reuses those blocks by identity; if the prefix disagreed with the full
//! parse the reader would show one document and settle on another.
//!
//! Oracle (O1, O2, O5): no panic within budgets; every block the prefix
//! returns but the last equals the full parse's block at that index; and
//! every source range addresses the document it is stored in.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, oracle, pipeline};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

fn check(data: &[u8], dialect: markview_commonmark::Dialect) {
	let budget = budget::Budget::parse().from_env();
	let md = String::from_utf8_lossy(data).into_owned();
	let len = md.len();
	if !(2..=256 * 1024).contains(&len) {
		return;
	}
	let guard = budget::InputGuard::new();
	let source: Arc<str> = Arc::from(md.as_str());
	let full = markview_commonmark::parse_with(source.clone(), dialect);
	oracle::assert_source_ranges(&full);
	// Several cuts, fixed and derived: the fixed ones straddle block
	// boundaries in ordinary documents, the derived one moves with the input.
	let seed = oracle::derive(data);
	let mut cuts = vec![
		1,
		len / 4,
		len / 2,
		(3 * len) / 4,
		len - 1,
		(seed as usize) % len,
	];
	cuts.retain(|cut| *cut > 0 && *cut < len);
	cuts.sort_unstable();
	cuts.dedup();
	for cut in cuts {
		if let Some(prefix) =
			markview_commonmark::parse_prefix_with(&source, cut, dialect)
		{
			oracle::assert_prefix_consistent(&full, &prefix);
			oracle::assert_source_ranges(&prefix);
		}
	}
	guard.finish(&budget, len);
}

fuzz_target!(|data: &[u8]| {
	for dialect in pipeline::dialects() {
		check(data, dialect);
	}
});
