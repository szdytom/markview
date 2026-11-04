//! G1, Tier 3 semantic oracle: mapped source ranges must be real places in the
//! source the front end read, and nested nodes must stay inside their parent.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_commonmark::MarkdownParse;
use markview_document::{Block, BlockKind};
use mvfuzz::{budget, mutators, oracle, seam};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

fn walk(
	doc: &MarkdownParse,
	blocks: &[Block],
	outer: Option<&std::ops::Range<usize>>,
) {
	for block in blocks {
		let range = doc.block_source(block);
		if let Some(outer) = outer
			&& let Some(range) = range.as_ref()
		{
			assert!(
				outer.start <= range.start && range.end <= outer.end,
				"block range {range:?} escapes its container {outer:?} in {:?}",
				doc.source.get(outer.clone()).unwrap_or("")
			);
		}
		match block.kind() {
			BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
				check_rich(doc, block, text, "inline", outer.is_some())
			}
			BlockKind::Details {
				summary, blocks, ..
			} => {
				check_rich(doc, block, summary, "summary", false);
				// The body is parsed from a snippet and remapped onto document
				// offsets, so its blocks stay inside the element's range.
				walk(doc, blocks, range.as_ref().or(outer));
			}
			BlockKind::Quote { blocks, .. }
			| BlockKind::Footnote { blocks, .. }
			| BlockKind::FrontMatter { blocks, .. }
			| BlockKind::Figure { blocks, .. } => {
				walk(doc, blocks, range.as_ref().or(outer))
			}
			BlockKind::List { items, .. } => {
				// A list's range stops at its last content byte while an item's
				// block may take the trailing line ending, so containment is
				// not assertable here.
				for item in items {
					walk(doc, &item.blocks, None);
				}
			}
			BlockKind::Table(table) => {
				if let Some(caption) = &table.caption {
					check_rich(doc, block, caption, "caption", true);
				}
				for child in table.cell_blocks() {
					walk(
						doc,
						std::slice::from_ref(child),
						range.as_ref().or(outer),
					);
				}
			}
			BlockKind::DefinitionList(items) => {
				for item in items {
					for terms in &item.terms {
						check_rich(doc, block, terms, "term", true);
					}
					for blocks in &item.definitions {
						walk(doc, blocks, range.as_ref().or(outer));
					}
				}
			}
			BlockKind::Code { .. } | BlockKind::Rule => {}
		}
	}
}

fn check_rich(
	doc: &MarkdownParse,
	block: &Block,
	text: &[markview_document::Inline],
	what: &str,
	assert_containment: bool,
) {
	if !assert_containment {
		return;
	}
	let Some(range) = doc.block_source(block) else {
		return;
	};
	for inline in text {
		let inline = oracle::inline_source(doc, block, inline);
		assert!(
			range.start <= inline.start && inline.end <= range.end,
			"{what} range {inline:?} escapes its block {range:?} ({:?})",
			doc.source.get(range.clone()).unwrap_or("")
		);
	}
}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let md = String::from_utf8_lossy(data).into_owned();
	let len = md.len();
	if !(1..=256 * 1024).contains(&len) {
		return;
	}
	let guard = budget::InputGuard::new();
	let source: Arc<str> = Arc::from(md.as_str());
	let doc = markview_commonmark::parse(source.clone());
	oracle::assert_source_ranges(&doc);
	seam::assert_rich_text_covered(&doc);
	walk(&doc, doc.document.blocks(), None);
	guard.finish(&budget, len);
});
