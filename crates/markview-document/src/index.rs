//! Semantic indexes built once when a document is sealed.
#[cfg(test)]
mod proptests;

use crate::{
	AnchorId, Block, BlockId, BlockKind, InlineKind, ListItem, OutlineEntry,
	RichText, plain_text,
};
use std::collections::HashMap;

#[cfg(test)]
thread_local! {
	pub(crate) static ITEM_LOOKUP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DocumentIndex {
	pub has_embeds: bool,
	/// Structural child-index path from the document root to each block.
	pub paths: HashMap<BlockId, std::ops::Range<usize>>,
	pub path_data: Vec<usize>,
	pub outline: Vec<OutlineEntry>,
	pub details: HashMap<BlockId, bool>,
	pub list_items: HashMap<AnchorId, ListItemPosition>,
	/// The owning block and disclosures enclosing each anchor, outermost first.
	pub enclosing: HashMap<AnchorId, (BlockId, Vec<BlockId>)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ListItemPosition {
	pub list: BlockId,
	pub index: usize,
	pub number: Option<isize>,
}

pub(crate) fn build(blocks: &[Block]) -> DocumentIndex {
	let mut index = DocumentIndex::default();
	let mut open = Vec::new();
	let mut path = Vec::new();
	for (child, block) in blocks.iter().enumerate() {
		path.push(child);
		walk(block, &mut path, &mut open, &mut index);
		path.pop();
	}
	index.path_data.shrink_to_fit();
	index.outline.shrink_to_fit();
	index
}

fn walk(
	block: &Block,
	path: &mut Vec<usize>,
	open: &mut Vec<BlockId>,
	index: &mut DocumentIndex,
) {
	index.has_embeds |= block.note_embed().is_some();
	let start = index.path_data.len();
	index.path_data.extend_from_slice(path);
	index.paths.insert(block.id, start..index.path_data.len());
	if let BlockKind::Details { default_open, .. }
	| BlockKind::FrontMatter { default_open, .. } = &block.kind
	{
		index.details.insert(block.id, *default_open);
	}
	if let BlockKind::Heading {
		level,
		text,
		anchor,
	} = &block.kind
	{
		index.outline.push(OutlineEntry {
			block: block.id,
			level: *level,
			text: plain_text(text),
			anchor: anchor.clone(),
		});
	}
	for anchor in block.anchors() {
		register(index, block.id, AnchorId::Fragment(anchor.clone()), open);
	}
	if let BlockKind::Heading { anchor, .. } = &block.kind {
		register(index, block.id, AnchorId::Fragment(anchor.clone()), open);
	}
	if let BlockKind::Footnote { id, .. } = &block.kind {
		register(
			index,
			block.id,
			AnchorId::FootnoteDefinition(id.clone()),
			open,
		);
	}
	for text in fields(block) {
		for inline in text {
			match &inline.kind {
				InlineKind::Anchor(anchor) => {
					register(
						index,
						block.id,
						AnchorId::Fragment(anchor.clone()),
						open,
					);
				}
				InlineKind::FootnoteRef(reference) => {
					register(
						index,
						block.id,
						AnchorId::FootnoteReference {
							definition: reference.definition.clone(),
							occurrence: reference.occurrence,
						},
						open,
					);
				}
				_ => {}
			}
		}
	}
	// Front matter is a closure too, so an anchor in its YAML must be able to
	// open it just like a `<details>` body.
	let disclosure = matches!(
		block.kind,
		BlockKind::Details { .. } | BlockKind::FrontMatter { .. }
	);
	if disclosure {
		open.push(block.id);
	}
	if let BlockKind::List {
		start,
		reversed,
		items,
		..
	} = block.kind()
	{
		walk_list(block.id(), *start, *reversed, items, path, open, index);
	} else {
		for (child, block) in children(block).into_iter().enumerate() {
			path.push(child);
			walk(block, path, open, index);
			path.pop();
		}
	}
	if disclosure {
		open.pop();
	}
}

fn walk_list(
	list: BlockId,
	mut number: Option<isize>,
	reversed: bool,
	items: &[ListItem],
	path: &mut Vec<usize>,
	open: &mut Vec<BlockId>,
	index: &mut DocumentIndex,
) {
	let mut child = 0;
	for (item_index, item) in items.iter().enumerate() {
		#[cfg(test)]
		ITEM_LOOKUP_PROBES.with(|probes| probes.set(probes.get() + 1));
		number = number.map(|value| item.number.unwrap_or(value));
		if let Some(first) = item.blocks.first()
			&& let Some(anchor) = first.list_item_anchor()
		{
			index.list_items.insert(
				AnchorId::Fragment(anchor.to_owned()),
				ListItemPosition {
					list,
					index: item_index,
					number,
				},
			);
		}
		for block in &item.blocks {
			path.push(child);
			walk(block, path, open, index);
			path.pop();
			child += 1;
		}
		number = number.map(|value| {
			if reversed {
				value.saturating_sub(1)
			} else {
				value.saturating_add(1)
			}
		});
	}
}

fn register(
	index: &mut DocumentIndex,
	block: BlockId,
	anchor: AnchorId,
	open: &[BlockId],
) {
	index
		.enclosing
		.entry(anchor)
		.or_insert_with(|| (block, open.to_vec()));
}

/// Fields owned by one block, in reading order.
fn fields(block: &Block) -> impl Iterator<Item = &RichText> {
	let single = match &block.kind {
		BlockKind::Paragraph(text) | BlockKind::Heading { text, .. } => {
			Some(text)
		}
		BlockKind::Quote { callout, .. } => {
			callout.as_ref().and_then(|c| c.title.as_ref())
		}
		BlockKind::Details { summary, .. } => Some(summary),
		BlockKind::Table(table) => table.caption.as_ref(),
		BlockKind::Figure { caption, .. } => caption.as_ref(),
		BlockKind::DefinitionList(_)
		| BlockKind::List { .. }
		| BlockKind::Footnote { .. }
		| BlockKind::FrontMatter { .. }
		| BlockKind::Code { .. }
		| BlockKind::Rule => None,
	};
	let terms = match &block.kind {
		BlockKind::DefinitionList(items) => Some(items.as_slice()),
		_ => None,
	};
	single
		.into_iter()
		.chain(terms.into_iter().flatten().flat_map(|item| &item.terms))
}

/// Child blocks in the same order the field traversal visits them.
pub(crate) fn children(block: &Block) -> Vec<&Block> {
	match &block.kind {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. }
		| BlockKind::Figure { blocks, .. } => blocks.iter().collect(),
		BlockKind::List { items, .. } => {
			items.iter().flat_map(|item| item.blocks.iter()).collect()
		}
		BlockKind::Table(table) => table.cell_blocks().collect(),
		BlockKind::DefinitionList(items) => items
			.iter()
			.flat_map(|item| item.definitions.iter())
			.flat_map(|blocks| blocks.iter())
			.collect(),
		BlockKind::Paragraph(_)
		| BlockKind::Heading { .. }
		| BlockKind::Code { .. }
		| BlockKind::Rule => Vec::new(),
	}
}

/// The sibling list and position corresponding to a flattened child index.
pub(crate) fn siblings_at(
	parent: &Block,
	index: usize,
) -> Option<(&[Block], usize)> {
	match parent.kind() {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. }
		| BlockKind::Figure { blocks, .. } => Some((blocks, index)),
		BlockKind::List { items, .. } => sibling_group(
			index,
			items.iter().map(|item| item.blocks.as_slice()),
		),
		BlockKind::Table(table) => sibling_group(
			index,
			table
				.groups
				.iter()
				.flat_map(|group| &group.rows)
				.flat_map(|row| &row.cells)
				.map(|cell| cell.blocks.as_slice()),
		),
		BlockKind::DefinitionList(items) => sibling_group(
			index,
			items
				.iter()
				.flat_map(|item| &item.definitions)
				.map(Vec::as_slice),
		),
		_ => None,
	}
}

fn sibling_group<'a>(
	mut index: usize,
	groups: impl Iterator<Item = &'a [Block]>,
) -> Option<(&'a [Block], usize)> {
	for blocks in groups {
		#[cfg(test)]
		ITEM_LOOKUP_PROBES.with(|probes| probes.set(probes.get() + 1));
		if index < blocks.len() {
			return Some((blocks, index));
		}
		index -= blocks.len();
	}
	None
}

/// Resolve a structural path from the document root.
pub(crate) fn resolve<'a>(
	blocks: &'a [Block],
	path: &[usize],
) -> Option<&'a Block> {
	let (first, rest) = path.split_first()?;
	let mut block = blocks.get(*first)?;
	for child in rest {
		let (siblings, index) = siblings_at(block, *child)?;
		block = siblings.get(index)?;
	}
	Some(block)
}
