//! Reuse leaf spans from the last published container without duplicating geometry.
#[cfg(test)]
mod proptests;

use super::{BlockContext, CacheKey, LayoutOptions, external_key};
use crate::{
	document::{Block, BlockKind, fingerprint},
	scene::{BlockLayout, LinkId},
};
use std::{collections::HashMap, sync::Arc};

pub(super) struct NestedEntry {
	pub layout: Arc<BlockLayout>,
	pub leaf: usize,
	pub pass: u64,
	pub completed: Option<u64>,
}

pub(super) struct NestedCache<'a> {
	pub entries: &'a HashMap<u64, NestedEntry>,
	pub base: &'a CacheKey,
}

impl BlockContext<'_> {
	pub(super) fn cached_child(
		&mut self,
		block: &Block,
		position: [f32; 2],
		width: f32,
		opts: &LayoutOptions,
		out: &mut BlockLayout,
	) -> f32 {
		let [x, y] = position;
		match &block.kind() {
			BlockKind::Paragraph(_)
			| BlockKind::Heading { .. }
			| BlockKind::Code { .. }
			| BlockKind::Rule => {}
			// A table's scene is a subtree: its framed cells open scenes of
			// their own, so the block owns several spans and leaves, while a
			// cache entry retains exactly one leaf span of one node. Reusing a
			// table would need a subtree entry that copies every node, span and
			// leaf below it, re-translates their local coordinates, rebinds the
			// link occurrences to the current block, and lets
			// `retain_children` record a leaf list rather than one index. Until
			// that exists a progressive pass re-shapes every table.
			BlockKind::Table(_) => {
				return self.block(block, x, y, width, opts, out);
			}
			_ => return self.block(block, x, y, width, opts, out),
		}
		let Some(cache) = &self.nested else {
			return self.block(block, x, y, width, opts, out);
		};
		if (self.cancelled)() {
			return 0.;
		}
		let mut key = cache.base.clone();
		key.position = 0;
		key.content = block.content_key();
		key.width = width.to_bits();
		key.paragraph_indent = opts.paragraph_indent.to_bits();
		let theme = opts.codeblock_theme_override.as_deref().or_else(|| {
			opts.stylesheet
				.rule(crate::style::Condition::CodeBlock)
				.theme
				.as_deref()
		});
		key.external =
			external_key(block, self.images, self.highlight_cache, theme, opts);
		let key = fingerprint(&(
			&key,
			self.shaper.appearance.key(),
			self.footnote_column,
		));
		if let Some(entry) = cache.entries.get(&key) {
			let field = match block.kind() {
				BlockKind::Paragraph(_)
				| BlockKind::Heading { .. }
				| BlockKind::Code { .. } => self.fields.next(block.id()),
				BlockKind::Rule => None,
				_ => unreachable!("only cached leaf kinds reach reuse"),
			};
			let origins = block_link_origins(block, &opts.limits);
			return out.reuse_leaf(
				&entry.layout,
				entry.leaf,
				position,
				width,
				field,
				&origins,
			);
		}
		let degraded = out.degraded;
		let math_errors = out.math_errors;
		let first_text = out.text.len();
		let height = self.block(block, x, y, width, opts, out);
		if !(self.cancelled)() {
			let field =
				out.text.get(first_text).and_then(|node| node.search_field);
			out.retain_leaf(
				key,
				height,
				out.degraded - degraded,
				out.math_errors - math_errors,
				field,
			);
		}
		height
	}
}

impl super::LayoutEngine {
	pub(super) fn retain_children(
		&mut self,
		layout: &Arc<BlockLayout>,
		pass: u64,
	) {
		for (leaf, key) in layout.leaf_keys().enumerate() {
			let completed =
				self.nested_cache.get(&key).and_then(|e| e.completed);
			self.nested_cache.insert(
				key,
				NestedEntry {
					layout: layout.clone(),
					leaf,
					pass,
					completed,
				},
			);
		}
	}
}

/// Rebind cached link occurrences to the ones `block` carries now. A reused
/// block keeps the ids it was laid out with, but an edit before it shifts every
/// source origin, so the current block is the authority on their values.
pub(super) fn remap_link_ids(
	geometry: Arc<BlockLayout>,
	block: &Block,
	limits: &crate::limits::Limits,
) -> Arc<BlockLayout> {
	let cached = &geometry.link_occurrences;
	if cached.is_empty() {
		return geometry;
	}
	let current = block_link_origins(block, limits);
	// The two lists mirror what layout prepares, including runs line breaking
	// discarded, so equal lengths pair logical occurrences exactly even when
	// RTL paints them in the opposite order.
	if cached.len() != current.len() || cached == &current {
		return geometry;
	}
	let remap: HashMap<_, _> = cached
		.iter()
		.copied()
		.zip(current.iter().copied())
		.collect();
	let mut layout = (*geometry).clone();
	for link in &mut layout.links {
		if let LinkId::Inline(origin) = link.id
			&& let Some(&origin) = remap.get(&origin)
		{
			link.id = LinkId::Inline(origin);
		}
	}
	for anchor in &mut layout.anchors {
		if let markview_document::AnchorId::FootnoteReference {
			occurrence, ..
		} = &mut anchor.anchor
			&& let Some(&origin) = remap.get(occurrence)
		{
			*occurrence = origin;
		}
	}
	layout.link_occurrences = current;
	Arc::new(layout)
}

/// The inline link occurrences `block` prepares, in layout order, including
/// runs whose content line breaking later discards.
pub(super) fn block_link_origins(
	block: &Block,
	limits: &crate::limits::Limits,
) -> Vec<markview_document::LinkOccurrenceId> {
	fn quote(
		callout: &Option<crate::document::Callout>,
		blocks: &[Block],
		limits: &crate::limits::Limits,
		out: &mut Vec<markview_document::LinkOccurrenceId>,
	) {
		// A foldable title is already the disclosed summary.
		if let Some(title) =
			callout.as_ref().and_then(|callout| callout.title.as_ref())
		{
			out.extend(crate::document::inline_link_origins(title));
		}
		for block in blocks {
			walk(block, limits, out);
		}
	}
	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the exhaustive block-kind link walk in one place so cached links cannot miss a container."
		)
	)]
	fn walk(
		block: &Block,
		limits: &crate::limits::Limits,
		out: &mut Vec<markview_document::LinkOccurrenceId>,
	) {
		match &block.kind() {
			BlockKind::Paragraph(text) => {
				out.extend(crate::document::inline_link_origins(text));
			}
			BlockKind::Heading { text, .. } => {
				out.extend(crate::document::inline_link_origins(text));
			}
			BlockKind::Code { .. } | BlockKind::Rule => {}
			BlockKind::Quote { callout, blocks } => {
				quote(callout, blocks, limits, out);
			}
			BlockKind::Footnote { blocks, .. }
			| BlockKind::FrontMatter { blocks, .. } => {
				for block in blocks {
					walk(block, limits, out);
				}
			}
			BlockKind::List { items, .. } => {
				for item in items {
					for block in &item.blocks {
						walk(block, limits, out);
					}
				}
			}
			// Tables never enter the single-leaf reuse path, so collecting
			// their origins would describe geometry that cannot be reused.
			BlockKind::Table(_) => {}
			BlockKind::DefinitionList(items) => {
				for item in items {
					for terms in &item.terms {
						out.extend(crate::document::inline_link_origins(terms));
					}
					for blocks in &item.definitions {
						for block in blocks {
							walk(block, limits, out);
						}
					}
				}
			}
			BlockKind::Figure { blocks, caption } => {
				for block in blocks {
					walk(block, limits, out);
				}
				if let Some(caption) = caption {
					out.extend(crate::document::inline_link_origins(caption));
				}
			}
			BlockKind::Details {
				summary, blocks, ..
			} => {
				out.extend(crate::document::inline_link_origins(summary));
				for block in blocks {
					walk(block, limits, out);
				}
			}
		}
	}
	let mut out = Vec::new();
	walk(block, limits, &mut out);
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::layout::LayoutEngine;

	#[test]
	fn callout_title_links_are_counted_once() {
		let limits = crate::limits::Limits::default();
		for source in [
			"> [!note] [title](t)\n> [body](b)\n",
			"> [!note]- [title](t)\n> [body](b)\n",
		] {
			let doc = markview_commonmark::parse_with(
				source,
				markview_commonmark::Dialect {
					ofm: true,
					..Default::default()
				},
			);
			assert_eq!(
				block_link_origins(&doc.blocks()[0], &limits).len(),
				2,
				"{source}"
			);
		}
	}

	#[test]
	fn an_edit_remeasures_one_leaf_and_releases_superseded_geometry() {
		let body = (0..32)
			.map(|i| format!("Paragraph {i} with several words.\n\n"))
			.collect::<String>();
		for open in ["", " open"] {
			let source = format!(
				"<details{open}>\n<summary>Summary</summary>\n\n{body}</details>\n"
			);
			let opts = LayoutOptions::default();
			let mut engine = LayoutEngine::new();
			let first = engine
				.layout(&markview_commonmark::parse(source.as_str()), &opts);
			let old = Arc::downgrade(&first.blocks[0].layout);
			assert_eq!(first.blocks[0].layout.scene.reused_leaves, 0);
			let edited = markview_commonmark::parse(
				source
					.replace("Paragraph 0", "A much longer edited paragraph 0"),
			);
			let next = engine.layout(&edited, &opts);
			assert_eq!(next.blocks[0].layout.scene.reused_leaves, 31);
			drop(first);
			assert!(old.upgrade().is_none());
			let summary = markview_commonmark::parse(
				edited.source.replace("Summary", "New summary"),
			);
			assert_eq!(
				engine.layout(&summary, &opts).blocks[0]
					.layout
					.scene
					.reused_leaves,
				32
			);
			assert_eq!(
				engine
					.layout(
						&summary,
						&LayoutOptions {
							width: 240.,
							..opts.clone()
						}
					)
					.blocks[0]
					.layout
					.scene
					.reused_leaves,
				0
			);
			engine.release_document();
			assert!(engine.nested_cache.is_empty());
		}
	}
}
