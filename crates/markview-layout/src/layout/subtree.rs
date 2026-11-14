//! Borrowed subtree selection within its original semantic document.
use super::{
	Block, Document, LayoutEngine, LayoutOptions, LayoutSnapshot,
	blocks::{anchor_only, block_role},
};
use crate::{
	document::BlockKind,
	image::ImageSnapshot,
	style::{Condition, TextAppearance},
};

impl LayoutEngine {
	/// Lays out one borrowed node, preserving the source document's identities
	/// and reference context. Ancestor boxes are excluded from the scene.
	///
	/// # Panics
	/// Panics if `node` does not belong to `document`.
	pub fn layout_subtree(
		&mut self,
		document: &Document,
		node: &Block,
		options: &LayoutOptions,
	) -> LayoutSnapshot {
		self.layout_subtree_with_images(
			document,
			node,
			options,
			&ImageSnapshot::default(),
		)
	}

	/// Subtree layout with the same resource snapshot used by whole-document layout.
	pub fn layout_subtree_with_images(
		&mut self,
		document: &Document,
		node: &Block,
		options: &LayoutOptions,
		images: &ImageSnapshot,
	) -> LayoutSnapshot {
		self.layout_subtree_cancellable(document, node, options, images, || {
			false
		})
		.expect("uninterrupted layout")
	}

	/// Borrows a subtree and cooperatively stops at the shared layout checkpoints.
	pub fn layout_subtree_cancellable(
		&mut self,
		document: &Document,
		node: &Block,
		options: &LayoutOptions,
		images: &ImageSnapshot,
		cancelled: impl Fn() -> bool + Sync,
	) -> Option<LayoutSnapshot> {
		assert!(
			document
				.block(node.id())
				.is_some_and(|original| std::ptr::eq(original, node)),
			"the layout root must belong to the source document"
		);
		if cancelled() {
			return None;
		}
		let layout =
			self.begin_layout_root(document, Some(node), options, images);
		self.complete_layout(document, layout, |_| true, cancelled)
	}
}

/// Original ancestor appearance and the selected node's visible sibling position.
pub(super) fn context(
	document: &Document,
	node: &Block,
	base: &TextAppearance,
	options: &LayoutOptions,
) -> (TextAppearance, (usize, usize)) {
	let path = document
		.path_of(node.id())
		.expect("a source node has a structural path");
	let mut parent = base.clone();
	let mut siblings = document.blocks();
	for depth in 0..path.len() {
		let current = document.block_at_path(&path[..=depth]).unwrap();
		let index = siblings
			.iter()
			.position(|b| b.id() == current.id())
			.unwrap();
		let position = (
			siblings[..index].iter().filter(|b| !anchor_only(b)).count(),
			siblings.iter().filter(|b| !anchor_only(b)).count(),
		);
		if depth + 1 == path.len() {
			return (parent, position);
		}
		parent = options.stylesheet.child(&parent, position.0, position.1);
		let role = block_role(current);
		if role.element_path().contains(Condition::Callout) {
			for condition in [Condition::Blockquote, Condition::Callout, role] {
				parent = options.stylesheet.text(&parent, condition);
			}
		} else {
			parent = options.stylesheet.text(&parent, role);
		}
		let child = document.block_at_path(&path[..=depth + 1]).unwrap();
		if let Some((row, count)) = table_position(current, child) {
			parent = options.stylesheet.child(&parent, row, count);
		}
		let (children, conditions) = child_context(current, child);
		siblings = children;
		for condition in conditions {
			parent = options.stylesheet.text(&parent, *condition);
		}
	}
	unreachable!("a block path is nonempty")
}

fn table_position(parent: &Block, child: &Block) -> Option<(usize, usize)> {
	let BlockKind::Table(table) = parent.kind() else {
		return None;
	};
	let count = table.groups.iter().map(|group| group.rows.len()).sum();
	table
		.groups
		.iter()
		.flat_map(|group| &group.rows)
		.position(|row| {
			row.cells.iter().any(|cell| {
				cell.blocks.iter().any(|block| block.id() == child.id())
			})
		})
		.map(|row| (row, count))
}

fn child_context<'a>(
	parent: &'a Block,
	child: &Block,
) -> (&'a [Block], &'static [Condition]) {
	let contains =
		|blocks: &[Block]| blocks.iter().any(|b| b.id() == child.id());
	match parent.kind() {
		BlockKind::Quote { blocks, .. }
		| BlockKind::Footnote { blocks, .. }
		| BlockKind::Details { blocks, .. }
		| BlockKind::FrontMatter { blocks, .. }
		| BlockKind::Figure { blocks, .. } => (blocks, &[]),
		BlockKind::List { items, .. } => (
			&items
				.iter()
				.find(|item| contains(&item.blocks))
				.unwrap()
				.blocks,
			&[Condition::ListItem],
		),
		BlockKind::Table(table) => {
			let cell = table
				.groups
				.iter()
				.flat_map(|g| &g.rows)
				.flat_map(|r| &r.cells)
				.find(|cell| contains(&cell.blocks))
				.unwrap();
			(
				&cell.blocks,
				if cell.kind == crate::document::CellKind::Header {
					&[Condition::Cell, Condition::Header]
				} else {
					&[Condition::Cell]
				},
			)
		}
		BlockKind::DefinitionList(items) => (
			items
				.iter()
				.flat_map(|item| &item.definitions)
				.find(|blocks| contains(blocks))
				.unwrap(),
			&[],
		),
		_ => unreachable!("only containers have structural children"),
	}
}

#[cfg(test)]
mod tests;
