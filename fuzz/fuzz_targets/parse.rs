//! G1, Tier 1: the whole parse entry point — extensions, front matter,
//! `<details>`, sourcepos mapping — plus the prefix and incremental entries.
//! Oracle (O1, O2): no panic, abort, or overflow; wall and allocation
//! budgets.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, oracle, pipeline};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

fn check(data: &[u8], dialect: markview_commonmark::Dialect) {
	let budget = budget::Budget::parse().from_env();
	let md = String::from_utf8_lossy(data);
	let len = md.len();
	// Parsing alone is cheap; keep the unit small so the campaign stays fast.
	if len > 512 * 1024 {
		return;
	}
	let guard = budget::InputGuard::new();
	let src: Arc<str> = Arc::from(md.as_ref());
	let doc = markview_commonmark::parse_with(src.clone(), dialect);
	// Every range the parse produced must address this source, so a later
	// `&source[range]` cannot panic on an out-of-bounds or split character.
	oracle::assert_source_ranges(&doc);
	// The outline and image walks re-derive structure the parser built.
	let _ = doc.outline();
	let _ = doc.details_enclosing("x");
	let mut images = Vec::new();
	for block in doc.document.blocks() {
		block.images(&mut images);
	}
	// The incremental entries over several cut and reuse points.
	let n = src.len();
	if n > 1 {
		for cut in [1, n / 4, n / 2, (3 * n) / 4] {
			let _ = markview_commonmark::parse_prefix_with(&src, cut, dialect);
		}
		let _ =
			markview_commonmark::parse_incremental(&doc, src.clone(), dialect);
		let _ = markview_commonmark::reparse_with(&doc, src.clone(), dialect);
	}
	guard.finish(&budget, len);
}

fuzz_target!(|data: &[u8]| {
	for dialect in pipeline::dialects() {
		check(data, dialect);
	}
});
