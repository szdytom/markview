//! Heading bookmarks using the same page destinations as fragment links.
#[cfg(test)]
mod proptests;

use krilla::{
	destination::XyzDestination,
	geom::Point,
	outline::{Outline, OutlineNode},
};
use markview_layout::{
	document::OutlineEntry,
	paginate::{PT_PER_PX, PageGeometry, Pagination},
};
use std::iter::Peekable;

pub(crate) fn build(
	entries: &[OutlineEntry],
	pagination: &Pagination,
	geometry: &PageGeometry,
) -> Option<Outline> {
	let [left, top, _, _] = geometry.text_pt();
	let mut nodes = entries
		.iter()
		.filter_map(|entry| {
			let &(page, y) = pagination.anchors.get(&entry.anchor)?;
			Some((
				entry.level,
				OutlineNode::new(
					entry.text.clone(),
					XyzDestination::new(
						page,
						Point::from_xy(left, top + y * PT_PER_PX),
					),
				),
			))
		})
		.peekable();
	nodes.peek()?;
	let mut outline = Outline::new();
	for node in children(&mut nodes, 0) {
		outline.push_child(node);
	}
	Some(outline)
}

/// A heading belongs to the nearest preceding heading with a lower level.
fn children(
	nodes: &mut Peekable<impl Iterator<Item = (u8, OutlineNode)>>,
	parent_level: u8,
) -> Vec<OutlineNode> {
	let mut result = Vec::new();
	while nodes.peek().is_some_and(|(level, _)| *level > parent_level) {
		let (level, mut node) = nodes.next().unwrap();
		for child in children(nodes, level) {
			node.push_child(child);
		}
		result.push(node);
	}
	result
}
