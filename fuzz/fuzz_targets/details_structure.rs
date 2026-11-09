//! G1, Tier 3 semantic oracle: the `<details>` scanner must agree with the
//! source it read.
//!
//! `html::details`, `html::close_tag` and `document::details_enclosing` turn
//! source bytes into a disclosure tree; a byte-level check can only ask
//! whether they stayed in bounds, not whether the tree means what the source
//! wrote. This target checks the meaning instead: the element the tree built
//! must enclose the tags the source wrote, and must not swallow a sibling.
//!
//! Oracle (O1, O2, O5), source-derived (plan item O7):
//!
//! - an element's range must fit the source, and any two element ranges must
//!   either nest or be disjoint, so two disclosures never share the identity a
//!   summary toggle addresses;
//! - a summary range must sit inside its element;
//! - the tree's declared `open` state must equal the source's `<details open>`
//!   whenever the element's range starts at its own opener;
//! - the summary text the tree reports must equal the source's `<summary>` inner
//!   text, modulo the Markdown the scanner is allowed to interpret;
//! - `details_enclosing` must return exactly the chain of elements the layout
//!   walk actually descends through.
#![no_main]

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_commonmark::MarkdownParse;
use markview_document::{AnchorId, BlockId};
use mvfuzz::{budget, mutators, oracle, seam};
use std::sync::Arc;

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

/// The `<details>` element chain each anchor the tree declares lives under,
/// as the document's own jump semantics define it: the *first* declaration
/// wins, and a declaration may be a reference occurrence rather than the
/// heading that spells the same name.
///
/// [`seam::fragment_targets`] walks the tree for exactly the names layout
/// turns into anchors and frames each with its disclosure chain, so this
/// expectation is independent of `Document`'s index while agreeing with the
/// rule that index applies.
fn enclosing_by_id(doc: &MarkdownParse) -> Vec<(AnchorId, Vec<BlockId>)> {
	let mut out: Vec<(AnchorId, Vec<BlockId>)> = Vec::new();
	for target in seam::fragment_targets(doc) {
		if out.iter().any(|(anchor, _)| *anchor == target.anchor) {
			continue;
		}
		out.push((target.anchor, target.chain));
	}
	out
}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let md = String::from_utf8_lossy(data).into_owned();
	let len = md.len();
	if !(2..=256 * 1024).contains(&len) {
		return;
	}
	// The scanner only runs on raw HTML, so steer the corpus: a document with
	// no `<details` at all still exercises the general parse, but the deep
	// oracle below is only meaningful when one is present.
	let guard = budget::InputGuard::new();
	let source: Arc<str> = Arc::from(md.as_str());
	let doc = markview_commonmark::parse(source.clone());
	oracle::assert_source_ranges(&doc);
	seam::assert_details_structure(&doc, &md);

	let elements = seam::details_elements(&doc);
	// `details_enclosing` must agree with a walk of the tree for every anchor
	// the tree registers. A disagreement means a jump would expand the wrong
	// disclosure chain and land on a block that is still hidden.
	for (anchor, expected) in enclosing_by_id(&doc) {
		let got = doc.details_enclosing(&anchor);
		assert_eq!(
			got, expected,
			"details_enclosing({anchor:?}) returned {got:?}, the tree nests \
			 it under {expected:?}"
		);
	}
	// Every element's declared state must be the one its **own** opener
	// carries.
	//
	// Both sides are bound to the same element, which an earlier version got
	// wrong in two ways: it read the first `<details` anywhere in the document
	// rather than the tag belonging to `elements[i]`, and it ended the tag at
	// the next `>` in the whole source rather than at the tag's own, so on a
	// malformed opener it swallowed a later well-formed `<details open>` and
	// reported that element's attribute as this one's. `opener_at` now
	// requires the tag to start exactly at the element's own range and to be
	// well formed, and returns `None` otherwise — the honest precondition
	// this assertion always claimed.
	for (id, range) in &elements {
		let Some(opener) = seam::opener_at(&doc, range) else {
			continue;
		};
		let Some(declared) = seam::declared_open(opener) else {
			continue;
		};
		let Some(built) = doc.details_declared(*id) else {
			continue;
		};
		assert_eq!(
			built, declared,
			"the element at {range:?} has the opener {opener:?} declaring \
			 open={declared}, the tree says open={built}"
		);
	}
	guard.finish(&budget, len);
});
