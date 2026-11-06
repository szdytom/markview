use super::*;
use std::mem::{size_of, size_of_val};

pub(super) fn vec_bytes<T>(values: &Vec<T>) -> usize {
	values.capacity() * size_of::<T>()
}

impl Draw {
	pub(super) fn heap_bytes(&self) -> usize {
		match self {
			Self::Clipped { draws, .. } => {
				vec_bytes(draws)
					+ draws.iter().map(Self::heap_bytes).sum::<usize>()
			}
			Self::Image { src, title, .. } => src.capacity() + title.capacity(),
			Self::Glyph(g) => size_of_val(g.coords.as_ref()),
			Self::Polygon { points, .. } => size_of_val(points.as_ref()),
			Self::Math { math, .. } => {
				use ratex_types::DisplayItem;
				size_of_val(math.as_ref())
					+ vec_bytes(&math.display.items)
					+ math
						.display
						.items
						.iter()
						.map(|item| match item {
							DisplayItem::GlyphPath { font, .. } => {
								font.capacity()
							}
							DisplayItem::Path { commands, .. } => {
								vec_bytes(commands)
							}
							_ => 0,
						})
						.sum::<usize>()
			}
			_ => 0,
		}
	}
}

impl PlacedBlock {
	/// Estimated retained geometry bytes for cache admission, excluding shared fonts.
	/// Shared geometry and small tree allocations are conservatively charged per block.
	pub fn retained_bytes(&self) -> usize {
		let b = &self.layout;
		size_of::<Self>()
			+ size_of::<BlockLayout>()
			+ size_of::<FlowIndex>()
			+ b.scene.heap_bytes()
			+ self.flow.heap_bytes()
			+ vec_bytes(&b.draws)
			+ b.draws.iter().map(Draw::heap_bytes).sum::<usize>()
			+ vec_bytes(&b.text)
			+ b.text.iter().map(TextNode::heap_bytes).sum::<usize>()
			+ vec_bytes(&b.page_constraints)
			+ vec_bytes(&b.inline_decorations)
			+ vec_bytes(&b.overflow)
			+ vec_bytes(&b.links)
			+ b.links
				.iter()
				.map(|link| link.url.len() * 2 + 64)
				.sum::<usize>()
			+ vec_bytes(&b.link_occurrences)
			+ vec_bytes(&b.anchors)
			+ b.anchors.len() * 128
	}
}
