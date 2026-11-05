//! Pins the webpage oracles against known-good inputs.
//!
//! A target is only useful once its oracles accept a correct implementation,
//! and this suite is the fast reading of that: the committed web fixtures and
//! a deterministic sweep of generated articles must pass every invariant. A
//! failure here is an oracle regression, not a campaign finding.
use mvfuzz::{webgen, webpage};

/// A spread of unit shapes: empty, one byte, powers of two, and a long run.
fn units() -> Vec<Vec<u8>> {
	let mut units = vec![Vec::new(), vec![0], vec![0xff]];
	for shift in 0..9 {
		let len = 1usize << shift;
		let mut unit = Vec::with_capacity(len);
		let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ len as u64;
		for _ in 0..len {
			state ^= state << 13;
			state ^= state >> 7;
			state ^= state << 17;
			unit.push((state >> 24) as u8);
		}
		units.push(unit);
	}
	units
}

fn check_html(html: &str) {
	let url = webpage::base_for(html.as_bytes());
	let Ok(article) = markview_webpage::extract(html, url) else {
		return;
	};
	webpage::assert_article(&article);
	webpage::assert_input_fingerprint(&article, html);
	webpage::assert_deterministic(html, url, &article);
	webpage::check_profiles(html, url, &webpage::tight_profile(4));
}

#[test]
fn a_metadata_edge_may_keep_a_non_breaking_space() {
	// `collapse` folds ASCII whitespace and keeps non-breaking spaces, so an
	// excerpt that starts with one is content, not an untrimmed edge.
	let html = "<!doctype html><html><head><title>Spaces</title>\
		<meta name=\"description\" content=\"&nbsp;Excerpt&nbsp;\"></head>\
		<body><article><h1>Spaces</h1><p>Enough prose for the extractor to\
		keep this article and its excerpt in the reading document. More prose\
		after that sentence keeps the candidate score high enough.</p>\
		</article></body></html>";
	let url = webpage::base_for(html.as_bytes());
	let article = markview_webpage::extract(html, url).expect("a valid page");
	webpage::assert_article(&article);
	assert!(
		article
			.metadata
			.excerpt
			.as_deref()
			.is_some_and(|excerpt| excerpt.contains("Excerpt")),
		"{:?}",
		article.metadata
	);
}

#[test]
fn signed_list_numbers_fingerprint_without_overflow() {
	// `dev` keeps overflow checks on, so the fingerprint's encoding of a
	// negative `value` must not wrap.
	let html = "<!doctype html><html><head><title>Signed</title></head>\
		<body><article><h1>Signed</h1>\
		<p>Enough prose for the extractor to keep this list and the article.\
		More prose after that sentence keeps the candidate score high.</p>\
		<ol start=\"-3\"><li value=\"-1\">minus one</li>\
		<li value=\"-7\">minus seven</li></ol></article></body></html>";
	let url = webpage::base_for(html.as_bytes());
	let article = markview_webpage::extract(html, url).expect("a valid page");
	webpage::assert_article(&article);
	webpage::assert_deterministic(html, url, &article);
}

#[test]
fn the_committed_web_fixtures_pass_every_invariant() {
	let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
	let mut fixtures: Vec<_> = std::fs::read_dir(root)
		.unwrap()
		.filter_map(|entry| {
			let path = entry.ok()?.path();
			let name = path.file_name()?.to_str()?;
			(name.starts_with("web-") && name.ends_with(".html"))
				.then_some(path)
		})
		.collect();
	fixtures.sort();
	assert_eq!(fixtures.len(), 7, "the web fixtures moved");
	for path in fixtures {
		let html = std::fs::read_to_string(&path).unwrap();
		check_html(&html);
	}
}

#[test]
fn generated_articles_match_their_own_record() {
	for unit in units() {
		let generated = webgen::generate(&unit);
		webgen::assert_markers(&generated);
		let url = webpage::base_for(&unit);
		let article = markview_webpage::extract(&generated.html, url)
			.unwrap_or_else(|error| {
				panic!(
					"a generated article was rejected ({error}):\n{}",
					generated.html
				)
			});
		webpage::assert_article(&article);
		webgen::assert_generated(&article, &generated);
	}
}

#[test]
fn arbitrary_html_passes_every_invariant() {
	for unit in units() {
		let html = String::from_utf8_lossy(&unit);
		check_html(&html);
	}
}
