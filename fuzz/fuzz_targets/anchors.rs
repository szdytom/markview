//! G1, Tier 3 semantic differential: a fragment target must be reachable, and
//! land where its source declared it.
//!
//! A `#name` jump is resolved by two walks that never meet: `Document::
//! details_enclosing` names the disclosures a jump must expand, and
//! `LayoutSnapshot::anchor_y` says where the jump lands. The HTML-target
//! feature (`c09dfd7`) added a second way for a document to declare a target —
//! `Block::anchors` and `InlineKind::Anchor` — to a mechanism that already had
//! three (heading slugs, footnote definitions, footnote references). This
//! target holds all of them to one reading: `seam::fragment_targets` rebuilds
//! the registry from the tree, and every anchor the layout produces must be
//! one of its names, reached through the chain it records, in the block it
//! names.
//!
//! Oracle (O1, O2, O5): no panic within budgets, plus the properties of any
//! correct fragment registry:
//!
//! 1. **The chain is the one the source nests the target under.**
//!    `details_enclosing` must return the first declaration's disclosure
//!    chain, outermost first.
//! 2. **Reachability.** Expanding exactly those chains makes every declared
//!    target resolvable, so a jump can always reach the target it names.
//! 3. **Position.** With the chains expanded, the block a jump reaches is the
//!    block that declares the name first; with the reader's own toggles
//!    (none), it is the first declaration whose chain the source left open.
//! 4. **No phantom anchors.** The layout may resolve no name the tree did not
//!    declare.
//! 5. **In-block position.** An anchor sits inside its block's box and the
//!    block's anchors are ordered by y.
//! 6. **Pagination keeps up.** Every anchor the layout exposes reaches the
//!    page map, and the map invents nothing.
#![no_main]

use std::{collections::BTreeMap, sync::Arc};

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_core::{
	layout::LayoutSnapshot,
	paginate::{PageGeometry, paginate},
};
use mvfuzz::{budget, mutators, oracle, pipeline, ratex, seam};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

/// The first block that resolves each name, so a name's position can be read
/// off a layout.
fn layout_targets(snapshot: &LayoutSnapshot) -> BTreeMap<String, usize> {
	let mut out = BTreeMap::new();
	for (index, block) in snapshot.blocks.iter().enumerate() {
		for (anchor, _) in block.anchor_positions() {
			out.entry(anchor.to_owned()).or_insert(index);
		}
	}
	out
}

fn page_geometry() -> PageGeometry {
	PageGeometry {
		width_pt: 595.,
		height_pt: 842.,
		margin_pt: [40.; 4],
	}
}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::layout().from_env();
	let md = String::from_utf8_lossy(data);
	if md.len() > 64 * 1024 {
		return;
	}
	// A formula in the document reaches ratex; see `mvfuzz::ratex`.
	ratex::allow_char_overflow();
	pipeline::warmup();
	let guard = budget::InputGuard::new();
	let doc = markview_core::document::parse(md.to_string());
	oracle::assert_source_ranges(&doc);

	let targets = seam::fragment_targets(&doc);
	let mut by_name: BTreeMap<&str, Vec<&seam::FragmentTarget>> =
		BTreeMap::new();
	for target in &targets {
		by_name.entry(&target.name).or_default().push(target);
	}

	// The reader starts with no `<details>` toggled, so a fresh document shows
	// the state its source declared; `force_open` would erase the disclosure
	// tracking this target exists to check.
	let mut options = pipeline::options_for(&md);
	options.force_open = false;
	let mut expanded = options.clone();
	expanded.details_open = Arc::new(
		by_name
			.values()
			.filter_map(|list| list.first())
			.flat_map(|target| target.chain.iter().map(|id| (*id, true)))
			.collect(),
	);
	let base = pipeline::differential_engine().layout(&doc, &options);
	let opened = pipeline::differential_engine().layout(&doc, &expanded);

	// 1. The disclosures a jump expands must be the ones framing the target.
	for (name, list) in &by_name {
		let declared = doc.details_enclosing(name);
		assert_eq!(
			declared, list[0].chain,
			"details_enclosing({name:?}) names {declared:?}, the source nests \
			 the declaration in {:?}",
			list[0].chain
		);
	}

	// 2. Expanding exactly those chains must make every target resolvable.
	for name in by_name.keys() {
		assert!(
			opened.anchor_y(name).is_some(),
			"{name:?} is declared but unreachable with its disclosure chain \
			 expanded"
		);
	}

	// A block index means the same thing on both sides only while the tree and
	// the layout agree block for block. The bundled stylesheet (the only one
	// these options select) hides the front-matter label, a nested block, so a
	// top-level block is never dropped.
	assert_eq!(
		doc.blocks.len(),
		base.blocks.len(),
		"the layout placed {} of the tree's {} top-level blocks",
		base.blocks.len(),
		doc.blocks.len()
	);
	for (index, (tree, placed)) in
		doc.blocks.iter().zip(&base.blocks).enumerate()
	{
		assert_eq!(
			tree.id, placed.id,
			"top-level block {index} is not the tree's ({:#x} vs {:#x})",
			tree.id, placed.id
		);
	}

	let base_at = layout_targets(&base);
	let opened_at = layout_targets(&opened);

	// 3. Position: expanded, the first declaration is what a jump finds;
	//    untouched, it is the first declaration whose chain is open.
	for (name, list) in &by_name {
		assert_eq!(
			opened_at.get(*name).copied(),
			Some(list[0].block),
			"{name:?} reaches block {:?} with everything expanded, but the \
			 tree declares it first in block {}",
			opened_at.get(*name),
			list[0].block
		);
		let visible = list.iter().find(|target| {
			target
				.chain
				.iter()
				.all(|id| doc.details_declared(*id) == Some(true))
		});
		assert_eq!(
			base_at.get(*name).copied(),
			visible.map(|target| target.block),
			"{name:?} reaches block {:?} untouched, the source leaves the \
			 first open declaration in block {:?}",
			base_at.get(*name),
			visible.map(|target| target.block)
		);
	}

	// 4. No anchor may come from anywhere but the tree.
	for name in base_at.keys() {
		assert!(
			by_name.contains_key(name.as_str()),
			"the layout resolves {name:?}, which the tree never declares"
		);
	}

	// 5. An anchor sits inside the block that registers it, in reading order.
	for (index, block) in base.blocks.iter().enumerate() {
		let mut last = f32::NEG_INFINITY;
		for (anchor, y) in block.anchor_positions() {
			assert!(
				y.is_finite() && y >= -0.01 && y <= block.height() + 0.01,
				"block {index} anchors {:?} at y={} outside its {}px box",
				anchor,
				y,
				block.height()
			);
			assert!(
				y >= last,
				"block {index} anchors {:?} at y={} after {}",
				anchor,
				y,
				last
			);
			last = y;
		}
	}

	// 6. Pagination must keep every anchor a jump can use, and invent none. A
	//    document that paginates to no drawn line at all is the one escape:
	//    there is no page to point at, and the reader has nothing to scroll.
	let pages = paginate(&doc, &base, &page_geometry());
	if pages.pages.iter().any(|page| !page.is_empty()) {
		for name in base_at.keys() {
			assert!(
				pages.anchors.contains_key(name),
				"{name:?} has a layout position but no page"
			);
		}
	}
	for name in pages.anchors.keys() {
		assert!(
			base_at.contains_key(name),
			"the page map resolves {name:?}, which no block anchors"
		);
	}

	guard.finish(&budget, md.len());
});
