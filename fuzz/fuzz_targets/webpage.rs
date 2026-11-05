//! G8, Tier 1 + Tier 3: the HTML-to-document lowering.
//!
//! Oracle: a conversion must not panic, must satisfy the document contract
//! (unique anchors, one address per link occurrence, absolute resources, a
//! tiling table grid, normalised text, the title/source preamble), must be
//! deterministic, must agree between `extract` and `extract_with`, and must
//! not turn a success into a failure when the budget is relaxed. The tight
//! profile runs the same limits the reader applies, so a budget path is
//! exercised on every accepted page.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use mvfuzz::{budget, mutators, webpage};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::html(data, size, max_size, seed)
}}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let html = String::from_utf8_lossy(data);
	// Readability is super-linear in the DOM; keep the unit small enough that
	// a campaign iterates, and let the corpus grow the interesting shapes.
	if html.len() > 128 * 1024 {
		return;
	}
	let url = webpage::base_for(data);
	let guard = budget::InputGuard::new();
	let article = markview_webpage::extract(&html, url);
	if let Ok(article) = &article {
		webpage::assert_article(article);
		webpage::assert_input_fingerprint(article, &html);
		// The same page, a second time: DOM iteration order and the shared
		// `Anchors` state must not leak into the result.
		webpage::assert_deterministic(&html, url, article);
		let profile = &markview_webpage::PROFILE_V1;
		let again = markview_webpage::extract_with(&html, url, profile)
			.expect("the same page failed on its explicit-profile conversion");
		assert_eq!(
			webpage::fingerprint(article),
			webpage::fingerprint(&again),
			"extract and extract_with disagree on one page"
		);
	}
	// A tighter budget may reject a page, never turn a rejection into a
	// success or a success into a rejection at the shipped limits. This runs
	// outside the success branch: the violation it looks for is a tight
	// profile succeeding where the shipped one failed.
	let shift = 1 + u32::from(data.last().copied().unwrap_or(0)) % 20;
	webpage::check_profiles(&html, url, &webpage::tight_profile(shift));
	guard.finish(&budget, html.len());
});
