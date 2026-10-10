//! Reuse leaf spans from the last published container without duplicating geometry.
use super::{BlockContext, CacheKey, LayoutOptions, external_key};
use crate::{
	document::{Block, BlockKind, fingerprint},
	scene::BlockLayout,
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
		let locator = match &block.kind {
			BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
				Some(text.as_ptr() as usize)
			}
			BlockKind::Code { text, .. } => Some(text.as_ptr() as usize),
			BlockKind::Table { rows, .. } => rows
				.iter()
				.flatten()
				.next()
				.map(|text| text.as_ptr() as usize),
			BlockKind::Rule => None,
			_ => return self.block(block, x, y, width, opts, out),
		};
		let Some(cache) = &self.nested else {
			return self.block(block, x, y, width, opts, out);
		};
		if (self.cancelled)() {
			return 0.;
		}
		let field = locator.and_then(|p| self.search_fields.get(&p).copied());
		let mut key = cache.base.clone();
		key.position = 0;
		key.content = block.content_key;
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
		let key = fingerprint(&(&key, self.shaper.appearance.key()));
		if let Some(entry) = cache.entries.get(&key) {
			return out.reuse_leaf(
				&entry.layout,
				entry.leaf,
				position,
				width,
				field,
			);
		}
		let degraded = out.degraded;
		let math_errors = out.math_errors;
		let height = self.block(block, x, y, width, opts, out);
		if !(self.cancelled)() {
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{document, layout::LayoutEngine};

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
			let first = engine.layout(&document::parse(source.as_str()), &opts);
			let old = Arc::downgrade(&first.blocks[0].layout);
			assert_eq!(first.blocks[0].layout.scene.reused_leaves, 0);
			let edited = document::parse(
				source
					.replace("Paragraph 0", "A much longer edited paragraph 0"),
			);
			let next = engine.layout(&edited, &opts);
			assert_eq!(next.blocks[0].layout.scene.reused_leaves, 31);
			drop(first);
			assert!(old.upgrade().is_none());
			let summary = document::parse(
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
