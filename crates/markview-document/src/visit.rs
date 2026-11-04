//! The single semantic traversal over a document's reading fields.
//!
//! Search, source mapping, selection and layout all consume this walk so a new
//! block variant cannot be handled by one and silently skipped by another.
use crate::{Block, BlockId, BlockKind, FieldId, RichText, plain_text_len};

/// One semantic text field.
#[derive(Clone, Copy)]
pub enum FieldText<'a> {
	Rich(&'a RichText),
	Code(&'a str),
}

impl FieldText<'_> {
	/// The field's reading projection, used for field offsets.
	pub fn len(&self) -> usize {
		match self {
			Self::Rich(text) => plain_text_len(text),
			Self::Code(text) => text.len(),
		}
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
}

/// Visits every field in one top-level block's subtree in reading order,
/// naming the block that owns it.
///
/// `enclosing` collects the [`BlockId`]s of the disclosures the current field
/// is nested in, outermost first. A field hidden in a collapsed body is still
/// visited: this is the complete semantic text, not the visible projection.
pub fn visit_fields(
	block: &Block,
	enclosing: &mut Vec<BlockId>,
	cancelled: &impl Fn() -> bool,
	visit: &mut impl FnMut(BlockId, FieldId, FieldText<'_>, &[BlockId]),
) {
	let mut ordinal = 0;
	walk(
		block,
		enclosing,
		cancelled,
		&mut |owner, text, enclosing| {
			visit(owner, FieldId::from_index(ordinal), text, enclosing);
			ordinal += 1;
		},
	);
}

#[cfg_attr(
	any(),
	markview::allow_complexity(
		reason = "Keep the exhaustive block-kind traversal together, so a new variant missing from one arm is visible where the others are."
	)
)]
fn walk(
	block: &Block,
	enclosing: &mut Vec<BlockId>,
	cancelled: &impl Fn() -> bool,
	visit: &mut impl FnMut(BlockId, FieldText<'_>, &[BlockId]),
) {
	if cancelled() {
		return;
	}
	match block.kind() {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			visit(block.id(), FieldText::Rich(text), enclosing);
		}
		BlockKind::Code { text, .. } => {
			visit(block.id(), FieldText::Code(text), enclosing);
		}
		BlockKind::Table(table) => {
			for child in table.cell_blocks() {
				walk(child, enclosing, cancelled, visit);
			}
			if let Some(caption) = &table.caption {
				visit(block.id(), FieldText::Rich(caption), enclosing);
			}
		}
		BlockKind::DefinitionList(items) => {
			for item in items {
				for terms in &item.terms {
					visit(block.id(), FieldText::Rich(terms), enclosing);
				}
				for blocks in &item.definitions {
					for child in blocks {
						walk(child, enclosing, cancelled, visit);
					}
				}
			}
		}
		BlockKind::Figure { blocks, caption } => {
			for child in blocks {
				walk(child, enclosing, cancelled, visit);
			}
			if let Some(caption) = caption {
				visit(block.id(), FieldText::Rich(caption), enclosing);
			}
		}
		BlockKind::Details {
			summary, blocks, ..
		} => {
			visit(block.id(), FieldText::Rich(summary), enclosing);
			enclosing.push(block.id());
			for child in blocks {
				walk(child, enclosing, cancelled, visit);
			}
			enclosing.pop();
		}
		BlockKind::FrontMatter { blocks, .. } => {
			enclosing.push(block.id());
			for child in blocks {
				walk(child, enclosing, cancelled, visit);
			}
			enclosing.pop();
		}
		BlockKind::Quote {
			callout: Some(callout),
			blocks,
		} => {
			// A folded callout's title is the nested disclosure's summary,
			// which the child walk below visits once.
			if let Some(title) = &callout.title {
				visit(block.id(), FieldText::Rich(title), enclosing);
			}
			for child in blocks {
				walk(child, enclosing, cancelled, visit);
			}
		}
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. } => {
			for child in blocks {
				walk(child, enclosing, cancelled, visit);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for child in &item.blocks {
					walk(child, enclosing, cancelled, visit);
				}
			}
		}
		BlockKind::Rule => {}
	}
}

/// Visits every [`BlockId`] in depth-first reading order.
pub fn visit_blocks(block: &Block, visit: &mut impl FnMut(&Block)) {
	visit(block);
	match block.kind() {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. } => {
			for child in blocks {
				visit_blocks(child, visit);
			}
		}
		BlockKind::List { items, .. } => {
			for item in items {
				for child in &item.blocks {
					visit_blocks(child, visit);
				}
			}
		}
		BlockKind::Table(table) => {
			for child in table.cell_blocks() {
				visit_blocks(child, visit);
			}
		}
		BlockKind::DefinitionList(items) => {
			for item in items {
				for blocks in &item.definitions {
					for child in blocks {
						visit_blocks(child, visit);
					}
				}
			}
		}
		BlockKind::Figure { blocks, .. } => {
			for child in blocks {
				visit_blocks(child, visit);
			}
		}
		BlockKind::Paragraph(_)
		| BlockKind::Heading { .. }
		| BlockKind::Code { .. }
		| BlockKind::Rule => {}
	}
}
