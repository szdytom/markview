//! G1, Tier 3 semantic differential: a reference definition must resolve the
//! same way whoever reads it.
//!
//! `incremental::definitions` re-derives a document's definitions for prefix
//! parses and for `<details>` bodies, and the coarse block-equality
//! differentials cannot say whether that scan agrees with the parser. This
//! target attacks the predicate itself: an independent column-zero scan of the
//! same bytes derives the definition table, and the tree's resolution is
//! compared against it.
//!
//! Oracle (O1, O2, O5): no panic within budgets, plus two source-derived
//! properties of any correct parse:
//!
//! 1. **Definition scope.** A `[x]: url` line that begins a top-level block is
//!    a definition for the whole document, so the same body text parsed
//!    inside a `<details>` element must resolve to the same destination. The
//!    tree is checked against `seam::definition_lines`, an independent
//!    column-zero scan.
//! 2. **Inert suffix.** Appending a fresh definition that nothing references
//!    cannot change any block before it, because a definition is not rendered
//!    and cannot interrupt a paragraph that already closed. The first blocks
//!    of the two parses must match exactly.
#![no_main]

use std::sync::Arc;

use libfuzzer_sys::{fuzz_mutator, fuzz_target};
use markview_commonmark::MarkdownParse;
use markview_document::{Block, BlockKind, InlineKind, RichText};
use mvfuzz::{budget, mutators, oracle, seam};

fuzz_mutator! { |data: &mut [u8], size: usize, max_size: usize, seed: u32| {
	mutators::markdown(data, size, max_size, seed)
}}

/// Whether the appended definition's text became visible anywhere in the
/// extended document.
///
/// A link reference definition renders no node, so a *successful* inert append
/// leaves no trace of its text in the tree at all. Any trace therefore means
/// the parser read the appended bytes as something else, and the
/// block-for-block comparison is meaningless for this input.
///
/// Two arms, because one is not enough — the second was added after a real
/// false positive (`crash-d1dc1aac…`): the appended line was absorbed into an
/// open paragraph as a lazy continuation, which left the block count
/// unchanged and split the marker's text across three inlines
/// (`"e.com/d"`, `"fuzz-inert-529"`, `": /inert-529 "`), so no single inline
/// `contains` the needle. A per-inline text search therefore misses the case
/// the count guard also misses, and the assertion fires on a paragraph that
/// merely continued.
///
/// - **region**: any node whose range reaches into the appended bytes. This is
///   the direct statement — the append is visible if the tree points at it.
/// - **text**: the reading text of each rich run with all whitespace removed,
///   searched for the marker. Concatenating first is what makes a marker split
///   across inlines detectable.
fn marker_visible(doc: &MarkdownParse, len: usize) -> bool {
	// The search key is the marker's *label stem*, not the whole marker: a
	// crash file minimized from an earlier run already contains that run's
	// marker text, and matching on the full spelling — which embeds the input
	// length — would miss it. `fuzz-inert-` is unique to this target's own
	// appends, so the stem is a safe key, and it is what actually appeared in
	// the paragraph-continuation false positive.
	const NEEDLE: &str = "fuzz-inert-";
	fn rich_has(text: &RichText, needle: &str) -> bool {
		let joined: String = text
			.iter()
			.filter_map(|inline| match &inline.kind {
				InlineKind::Text(t) => Some(t.as_str()),
				_ => None,
			})
			.collect::<String>()
			.chars()
			.filter(|c| !c.is_whitespace())
			.collect();
		joined.contains(needle)
	}
	fn walk(
		doc: &MarkdownParse,
		blocks: &[Block],
		needle: &str,
		len: usize,
	) -> bool {
		for block in blocks {
			if doc.block_source(block).is_some_and(|range| range.end > len) {
				return true;
			}
			let hit = match &block.kind() {
				BlockKind::Paragraph(text)
				| BlockKind::Heading { text, .. } => rich_has(text, needle),
				BlockKind::Code { text, .. } => text.contains(needle),
				BlockKind::Details {
					summary, blocks, ..
				} => rich_has(summary, needle) || walk(doc, blocks, needle, len),
				BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. }
				| BlockKind::Figure { blocks, .. } => walk(doc, blocks, needle, len),
				BlockKind::List { items, .. } => items
					.iter()
					.any(|item| walk(doc, &item.blocks, needle, len)),
				BlockKind::Table(table) => table.cell_blocks().any(|child| {
					walk(doc, std::slice::from_ref(child), needle, len)
				}),
				BlockKind::DefinitionList(items) => items.iter().any(|item| {
					item.terms.iter().any(|terms| rich_has(terms, needle))
						|| item
							.definitions
							.iter()
							.any(|blocks| walk(doc, blocks, needle, len))
				}),
				BlockKind::Rule => false,
			};
			if hit {
				return true;
			}
		}
		false
	}
	walk(doc, doc.document.blocks(), NEEDLE, len)
}

fuzz_target!(|data: &[u8]| {
	let budget = budget::Budget::parse().from_env();
	let md = String::from_utf8_lossy(data).into_owned();
	let len = md.len();
	if !(2..=256 * 1024).contains(&len) {
		return;
	}
	let guard = budget::InputGuard::new();
	let source: Arc<str> = Arc::from(md.as_str());
	let doc = markview_commonmark::parse(source.clone());
	oracle::assert_source_ranges(&doc);

	// 1. Every definition the independent scan is sure of must resolve to its
	//    own destination wherever the tree used it, and no link may carry a
	//    destination no definition declares.
	seam::assert_reference_resolution(&doc);

	// 2. The inert-suffix differential. A link reference definition is never
	//    rendered: comrak consumes the `[x]: url` line and emits no node for
	//    it, so appending an unreferenced one must leave the document's block
	//    list *exactly* as it was — same count, same blocks, same ranges.
	//
	//    The hard part is the precondition, and it is stricter than it looks.
	//    An append is inert only when the base document has *finished* every
	//    construct at EOF. All of these defeat a textual check and were found
	//    the hard way while building this target:
	//
	//    - `1.` alone is a paragraph, while `1.` with a following line is a
	//      list marker;
	//    - `a\n  ` (trailing spaces, no terminator) leaves a paragraph open,
	//      and the appended line continues it;
	//    - `<foo bar\n` is an unterminated raw HTML block, which swallows
	//      whatever follows;
	//    - a **list or quote item stays open across a blank line**, so the
	//      appended line is absorbed into its last item's paragraph — the
	//      common case in this corpus.
	//
	//    So the precondition is checked structurally, and the append is
	//    verified inert *before* it is trusted: the definition line must be
	//    The trigger is narrow and structural. The append is only a suffix
	//    when the parser had already finished every construct at EOF, and
	//    *predicting* that from the bytes was wrong five times over while
	//    building this target:
	//
	//    - `1.` / `0)` alone is a paragraph, but a bare list marker with a
	//      line after it is an empty list item;
	//    - `a\n  ` (trailing spaces, no terminator) leaves the paragraph open
	//      and the appended line continues it;
	//    - `<foo bar\n` is an unterminated raw HTML block, which swallows
	//      whatever follows;
	//    - a list or quote item stays open across a blank line, so the
	//      appended line joins its last item's paragraph;
	//    - `[fo\x05o]: <…>` is ordinary paragraph text on its own, but a
	//      blank line before it lets the *appended* line close that
	//      paragraph, after which the line reads as a definition and the
	//      paragraph disappears.
	//
	//    So the base document's last block must be a leaf the append cannot
	//    reopen. That is a property of the parse, not of the text, which is
	//    why every textual test failed.
	//
	//    The last block is therefore required to be a paragraph or heading
	//    whose range ends *before* the end of the source, with at least one
	//    blank line of slack. That excludes each shape above at once: the
	//    bare marker and the open HTML block both own the end of the source,
	//    a container is excluded by kind, and the `[fo\x05o]` case is
	//    excluded because its paragraph also owns the source end.
	let Some(last) = doc.document.blocks().last() else {
		guard.finish(&budget, len);
		return;
	};
	let leaf = matches!(
		last.kind(),
		BlockKind::Paragraph(_) | BlockKind::Heading { .. }
	);
	// The range is a real one for this source: `assert_source_ranges` above
	// holds every block range in bounds and on a character boundary, so the
	// tail can be sliced as it stands.
	let Some(range) = doc.block_source(last) else {
		guard.finish(&budget, len);
		return;
	};
	let end = range.end.min(len);
	if !leaf || range.is_empty() {
		guard.finish(&budget, len);
		return;
	}
	let tail = &md[end..];
	// At least one line ending of slack: the appended line must not be the
	// first thing that can close the last block.
	let slack = tail.matches(['\n', '\r']).count() >= 2
		|| (tail.matches(['\n', '\r']).count() == 1
			&& tail.ends_with(['\n', '\r']));
	// A document opening with `---` is never in scope: comrak decides whether
	// that is front matter only once a *closing* `---` arrives, so appending a
	// line can retroactively turn the whole prefix into YAML. Measured on
	// `---\r\n--\n---\r`, which is a thematic break plus a setext heading on
	// its own and one front-matter block once anything follows it.
	if md.starts_with("---") || !slack {
		guard.finish(&budget, len);
		return;
	}
	let marker = format!("\n[fuzz-inert-{len}]: /inert-{len}\n");
	let extended = format!("{md}{marker}");
	let with_suffix = markview_commonmark::parse(Arc::from(extended.as_str()));
	oracle::assert_source_ranges(&with_suffix);
	// The append was absorbed if its bytes show up anywhere in the extended
	// tree: a list or quote item still open at EOF takes the line into its
	// last paragraph, an unterminated raw HTML block keeps it as literal
	// source, and an empty footnote definition at EOF (`[^1]: `) swallows the
	// next line into a footnote block. All are properties of the append, not
	// defects.
	//
	// The test is empirical on purpose. Trying to *predict* closedness from
	// the bytes was wrong five times over while building this target (bare
	// marker, trailing spaces, open HTML, container continuation, front
	// matter). "The parser has finished here" is a property of the parse, not
	// of the text, so the tree is asked instead.
	//
	// One arm is not enough, and the block count is *not* a sufficient guard
	// on its own — that cost a real false positive (`crash-d1dc1aac…`, 529
	// bytes). There the appended line was swallowed by an open paragraph as a
	// lazy continuation: the count stayed equal, but the paragraph's inlines
	// were rewritten in place, so the per-block comparison fired on a
	// paragraph that had merely continued. `marker_visible` catches that arm
	// only because it searches the *joined* reading text — the marker arrived
	// split across three inlines (`"e.com/d"`, `"fuzz-inert-529"`,
	// `": /inert-529 "`), so a per-inline search would have missed it too.
	if marker_visible(&with_suffix, len) {
		guard.finish(&budget, len);
		return;
	}
	// A count change is itself proof that the appended line was read as
	// something other than an invisible definition, so it is not a finding —
	// it means this input's precondition failed after all. Only a change that
	// keeps the count while altering an existing block is the defect this
	// differential looks for: the definition rendered no node, yet an earlier
	// block's resolution moved.
	if with_suffix.document.blocks().len() != doc.document.blocks().len() {
		guard.finish(&budget, len);
		return;
	}
	// An unterminated construct at EOF takes the following line into its own
	// literal without changing its range, so neither the region arm of
	// `marker_visible` nor the count guard sees it: the append moved text, but
	// not in a way this differential can attribute. A code block whose text
	// moved therefore takes the differential out of scope rather than
	// reporting a change the append did not cause.
	if code_text_changed(doc.document.blocks(), with_suffix.document.blocks()) {
		guard.finish(&budget, len);
		return;
	}
	// Heading anchors are a document-global uniqueness counter, not content:
	// appending anything that declares a heading with the same slug renumbers
	// the earlier one (`deeper` -> `deeper-1`) without changing a byte of the
	// text it labels. `Block::content_key` embeds the anchor, so comparing it
	// would report that renumbering as a moved block. Measured on
	// `crash-39870c40…`, whose marker text carried a `#`.
	//
	// Anchors are therefore excluded from the comparison, and an input whose
	// anchors actually moved stands down: the differential is about a block's
	// text and range, neither of which an anchor counter can legitimately
	// change.
	if anchors_of(doc.document.blocks())
		!= anchors_of(with_suffix.document.blocks())
	{
		guard.finish(&budget, len);
		return;
	}
	for i in 0..doc.document.blocks().len() {
		assert_eq!(
			without_anchors(&doc.document.blocks()[i]),
			without_anchors(&with_suffix.document.blocks()[i]),
			"block {i} changed when an unreferenced definition was appended \
			 after a closed block"
		);
	}
	guard.finish(&budget, len);
});

/// Whether any code block's literal differs between two parses of the same
/// prefix. An unterminated raw HTML or indented code block keeps absorbing
/// what follows it, which is the recorded class this differential defers to.
fn code_text_changed(before: &[Block], after: &[Block]) -> bool {
	fn collect(blocks: &[Block], out: &mut Vec<String>) {
		for block in blocks {
			match &block.kind() {
				BlockKind::Code { text, .. } => out.push(text.clone()),
				BlockKind::Details { blocks, .. }
				| BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => collect(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						collect(&item.blocks, out);
					}
				}
				_ => {}
			}
		}
	}
	let (mut a, mut b) = (Vec::new(), Vec::new());
	collect(before, &mut a);
	collect(after, &mut b);
	a != b
}

/// Every heading anchor in the tree, in document order, so a caller can tell
/// whether a global uniqueness counter moved.
fn anchors_of(blocks: &[Block]) -> Vec<String> {
	fn walk(blocks: &[Block], out: &mut Vec<String>) {
		for block in blocks {
			match &block.kind() {
				BlockKind::Heading { anchor, .. } => out.push(anchor.clone()),
				BlockKind::Details { blocks, .. }
				| BlockKind::Quote { blocks, .. }
				| BlockKind::Footnote { blocks, .. }
				| BlockKind::FrontMatter { blocks, .. } => walk(blocks, out),
				BlockKind::List { items, .. } => {
					for item in items {
						walk(&item.blocks, out);
					}
				}
				_ => {}
			}
		}
	}
	let mut out = Vec::new();
	walk(blocks, &mut out);
	out
}

/// A block with its heading anchor — and the content key derived from it —
/// blanked, so a comparison can ignore the global anchor counter.
fn without_anchors(block: &Block) -> String {
	let mut clone = block.clone();
	blank_anchors(&mut clone);
	format!("{:?}", clone.kind())
}

fn blank_anchors(block: &mut Block) {
	match block.kind_mut() {
		BlockKind::Heading { anchor, .. } => anchor.clear(),
		BlockKind::Details { blocks, .. }
		| BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. } => {
			for block in blocks {
				blank_anchors(block);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for block in &mut item.blocks {
					blank_anchors(block);
				}
			}
		}
		_ => {}
	}
}
