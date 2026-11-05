//! Construction of finished [`Document`]s.
//!
//! Producers submit semantic blocks and never derive identities or cache keys
//! themselves. [`DocumentBuilder::finish`] assigns cache keys bottom-up and
//! verifies that every occurrence identity in the subtree is unique.
use crate::{
	Block, BlockId, BlockKind, ContentKey, Document, DocumentRevision,
	content_identity, fingerprint, index, plain_text_len, semantic_key,
};
use std::collections::HashSet;

/// Construction limits for one document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentLimits {
	pub max_blocks: usize,
	pub max_depth: usize,
	pub max_text_bytes: usize,
	pub max_table_cells: usize,
}

impl Default for DocumentLimits {
	fn default() -> Self {
		Self {
			max_blocks: 100_000,
			max_depth: 256,
			max_text_bytes: 32 * 1024 * 1024,
			max_table_cells: 131_072,
		}
	}
}

/// Why a document exceeds its construction budget, with the reached value and
/// the limit that rejected it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentLimit {
	Blocks { blocks: usize, limit: usize },
	Depth { depth: usize, limit: usize },
	Text { bytes: usize, limit: usize },
	Table { cells: usize, limit: usize },
}

/// Allocates node identities for one document being built.
#[derive(Debug)]
pub struct DocumentBuilder {
	revision: DocumentRevision,
	next_block: u32,
	limits: DocumentLimits,
}

impl DocumentBuilder {
	/// A builder for a document whose every occurrence is new.
	pub fn new(revision: DocumentRevision) -> Self {
		Self {
			revision,
			next_block: 1,
			limits: DocumentLimits::default(),
		}
	}

	/// A builder whose identities continue above `after`.
	///
	/// An incremental parse reuses untouched blocks and must allocate ids that
	/// cannot collide with them.
	pub fn continuing(
		revision: DocumentRevision,
		after: Option<BlockId>,
	) -> Self {
		Self {
			revision,
			next_block: after.map_or(1, |id| id.get().saturating_add(1)),
			limits: DocumentLimits::default(),
		}
	}

	pub fn revision(&self) -> DocumentRevision {
		self.revision
	}

	/// Replaces the construction limits before blocks are submitted.
	pub fn with_limits(mut self, limits: DocumentLimits) -> Self {
		self.limits = limits;
		self
	}

	/// Builds one block, taking the next identity.
	pub fn block(&mut self, anchors: Vec<String>, kind: BlockKind) -> Block {
		let id = BlockId::new(self.next_block)
			.expect("block identity space exhausted");
		self.next_block = self.next_block.saturating_add(1);
		Block {
			anchors,
			id,
			content_key: ContentKey(0),
			kind,
		}
	}

	/// Seals the tree after checking its construction budget.
	pub fn try_finish(
		self,
		mut blocks: Vec<Block>,
	) -> Result<Document, DocumentLimit> {
		check_budget(&blocks, self.limits)?;
		let mut seen = HashSet::new();
		for block in &mut blocks {
			seal(block, &mut seen);
		}
		let index = index::build(&blocks);
		Ok(Document {
			revision: self.revision,
			content: content_identity(&blocks),
			blocks,
			index,
		})
	}

	/// Seals a trusted tree without enforcing the optional budget.
	///
	/// Front ends bound their own input; callers that submit untrusted trees
	/// use [`Self::try_finish`] instead of panicking in layout.
	pub fn finish(self, mut blocks: Vec<Block>) -> Document {
		let mut seen = HashSet::new();
		for block in &mut blocks {
			seal(block, &mut seen);
		}
		let index = index::build(&blocks);
		Document {
			revision: self.revision,
			content: content_identity(&blocks),
			blocks,
			index,
		}
	}
}

#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep one iterative, depth-aware budget walk beside the builder so every container is accounted for before sealing."
	)
)]
fn check_budget(
	blocks: &[Block],
	limits: DocumentLimits,
) -> Result<(), DocumentLimit> {
	let mut stack: Vec<(&Block, usize)> =
		blocks.iter().map(|block| (block, 1)).collect();
	let mut count = 0;
	let mut text = 0;
	let mut cells = 0;
	while let Some((block, depth)) = stack.pop() {
		if depth > limits.max_depth.saturating_add(1) {
			return Err(DocumentLimit::Depth {
				depth,
				limit: limits.max_depth,
			});
		}
		count += 1;
		if count > limits.max_blocks {
			return Err(DocumentLimit::Blocks {
				blocks: count,
				limit: limits.max_blocks,
			});
		}
		match &block.kind {
			BlockKind::Paragraph(t) | BlockKind::Heading { text: t, .. } => {
				text += plain_text_len(t);
			}
			BlockKind::Code { text: t, .. } => {
				text += t.len();
			}
			BlockKind::Table(table) => {
				cells += table
					.groups
					.iter()
					.flat_map(|group| &group.rows)
					.map(|row| row.cells.len())
					.sum::<usize>();
				if cells > limits.max_table_cells {
					return Err(DocumentLimit::Table {
						cells,
						limit: limits.max_table_cells,
					});
				}
				if let Some(caption) = &table.caption {
					text += plain_text_len(caption);
				}
				for child in table.cell_blocks() {
					stack.push((child, depth + 1));
				}
			}
			BlockKind::Quote { callout, blocks } => {
				if let Some(title) =
					callout.as_ref().and_then(|callout| callout.title.as_ref())
				{
					text += plain_text_len(title);
				}
				for child in blocks {
					stack.push((child, depth + 1));
				}
			}
			BlockKind::Details {
				summary, blocks, ..
			} => {
				text += plain_text_len(summary);
				for child in blocks {
					stack.push((child, depth + 1));
				}
			}
			BlockKind::Figure { blocks, caption } => {
				if let Some(caption) = caption {
					text += plain_text_len(caption);
				}
				for child in blocks {
					stack.push((child, depth + 1));
				}
			}
			BlockKind::Footnote { blocks, .. }
			| BlockKind::FrontMatter { blocks, .. } => {
				for child in blocks {
					stack.push((child, depth + 1));
				}
			}
			BlockKind::List { items, .. } => {
				for child in items.iter().flat_map(|item| &item.blocks) {
					stack.push((child, depth + 1));
				}
			}
			BlockKind::DefinitionList(items) => {
				for term in items.iter().flat_map(|item| &item.terms) {
					text += plain_text_len(term);
				}
				for child in
					items.iter().flat_map(|item| &item.definitions).flatten()
				{
					stack.push((child, depth + 1));
				}
			}
			BlockKind::Rule => {}
		}
		if text > limits.max_text_bytes {
			return Err(DocumentLimit::Text {
				bytes: text,
				limit: limits.max_text_bytes,
			});
		}
	}
	Ok(())
}

fn seal(block: &mut Block, seen: &mut HashSet<BlockId>) {
	assert!(
		seen.insert(block.id),
		"duplicate block identity {}",
		block.id
	);
	match &mut block.kind {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. } => {
			for child in blocks {
				seal(child, seen);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for child in &mut item.blocks {
					seal(child, seen);
				}
			}
		}
		BlockKind::Table(table) => {
			for child in table.cell_blocks_mut() {
				seal(child, seen);
			}
		}
		BlockKind::DefinitionList(items) => {
			for item in items {
				for blocks in &mut item.definitions {
					for child in blocks {
						seal(child, seen);
					}
				}
			}
		}
		BlockKind::Figure { blocks, .. } => {
			for child in blocks {
				seal(child, seen);
			}
		}
		BlockKind::Paragraph(_)
		| BlockKind::Heading { .. }
		| BlockKind::Code { .. }
		| BlockKind::Rule => {}
	}
	let key = semantic_key(&block.kind);
	block.content_key = if block.anchors.is_empty() {
		key
	} else {
		ContentKey(fingerprint(&(key, &block.anchors)))
	};
}
#[cfg(test)]
mod proptests;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{BlockKind, Inline, InlineKind, TextStyle};

	fn text(value: &str) -> Vec<Inline> {
		vec![Inline {
			link: None,
			kind: InlineKind::Text(value.to_owned()),
			style: TextStyle::default(),
		}]
	}

	#[test]
	fn an_over_budget_tree_is_rejected_before_sealing() {
		let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
		let mut block =
			builder.block(Vec::new(), BlockKind::Paragraph(text("deep")));
		for _ in 0..300 {
			block = builder.block(
				Vec::new(),
				BlockKind::Quote {
					callout: None,
					blocks: vec![block],
				},
			);
		}
		assert!(matches!(
			builder.try_finish(vec![block]),
			Err(DocumentLimit::Depth { .. })
		));
	}
}
