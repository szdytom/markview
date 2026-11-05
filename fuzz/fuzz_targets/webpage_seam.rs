//! G8, Tier 3: a generated article against the lowering.
//!
//! Oracle: every marker the generator wrote survives exactly once and in
//! order, and each generated element becomes the block the generator
//! described — heading level and explicit id, list items, tightness and
//! start, quote, code language and text, formula source and display flag,
//! table groups, spans, alignments and cell text, definition terms,
//! disclosure state and body, and a figure's caption and image. The page also
//! has to convert deterministically and agree between `extract` and
//! `extract_with`. The expectation comes from the generator, not from the
//! lowering, so a lost or misplaced element is a finding rather than a
//! self-consistent tree.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, webgen, webpage};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::html(data, size, max_size, seed)
}}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let url = webpage::base_for(data);
	let generated = webgen::generate(data);
	if generated.html.len() > 128 * 1024 {
		return;
	}
	let guard = budget::InputGuard::new();
	webgen::assert_markers(&generated);
	let article = markview_webpage::extract(&generated.html, url)
		.expect("a generated article was rejected");
	webpage::assert_article(&article);
	webpage::assert_input_fingerprint(&article, &generated.html);
	webgen::assert_generated(&article, &generated);
	// The same page again, and through the explicit-profile entry point.
	webpage::assert_deterministic(&generated.html, url, &article);
	let again = markview_webpage::extract_with(
		&generated.html,
		url,
		&markview_webpage::PROFILE_V1,
	)
	.expect(
		"the same generated page failed on its explicit-profile conversion",
	);
	assert_eq!(
		webpage::fingerprint(&article),
		webpage::fingerprint(&again),
		"extract and extract_with disagree on one generated page"
	);
	guard.finish(&budget, generated.html.len());
});
