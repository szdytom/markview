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
			Self::Box {
				decoration: Some(_),
				..
			} => size_of::<BoxDecoration>() + 16,
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

pub(super) fn style_bytes(
	draws: &[Draw],
	seen: &mut std::collections::HashSet<*const GlyphStyle>,
) -> usize {
	draws
		.iter()
		.map(|d| match d {
			Draw::Glyph(g) if seen.insert(Arc::as_ptr(&g.style)) => {
				size_of::<GlyphStyle>() + 16
			}
			Draw::Clipped { draws, .. } => style_bytes(draws, seen),
			_ => 0,
		})
		.sum()
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
			+ b.draws.heap_bytes()
			+ b.text.heap_bytes()
			+ b.text.iter().map(TextNode::heap_bytes).sum::<usize>()
			+ b.page_constraints.heap_bytes()
			+ b.inline_decorations.heap_bytes()
			+ b.overflow.heap_bytes()
			+ b.links.storage_bytes()
			+ b.links
				.iter()
				.map(|link| link.url.len() * 2 + 64)
				.sum::<usize>()
			+ vec_bytes(&b.link_occurrences)
			+ vec_bytes(&b.anchors)
			+ b.anchors.len() * 128
	}
}

/// Known payload allocations, separate from allocator totals and mapped resources.
#[cfg(feature = "fuzz")]
pub type StorageVisitor<'a> =
	dyn FnMut(usize, &'static str, usize, usize, usize) + 'a;
#[cfg(feature = "fuzz")]
pub(crate) fn allocation<T>(
	values: &[T],
	capacity: usize,
	category: &'static str,
	visit: &mut StorageVisitor<'_>,
) {
	if capacity != 0 {
		visit(
			values.as_ptr() as usize,
			category,
			capacity * size_of::<T>(),
			values.len(),
			capacity,
		);
	}
}
#[cfg(feature = "fuzz")]
impl BlockLayout {
	pub fn visit_storage(&self, visit: &mut StorageVisitor<'_>) {
		visit(
			std::ptr::from_ref(self) as usize,
			"block_layouts",
			size_of::<Self>() + 2 * size_of::<usize>(),
			1,
			1,
		);
		self.draws.visit_storage(visit);
		self.text.visit_storage("text_nodes", visit);
		for text in &self.text {
			text.visit_storage(visit);
		}
		self.scene.visit_storage(visit);
		self.page_constraints
			.visit_storage("page_constraints", visit);
		self.inline_decorations.visit_storage("decorations", visit);
		self.overflow.visit_storage("overflow", visit);
		self.links.visit_storage(visit);
		allocation(
			&self.link_occurrences,
			self.link_occurrences.capacity(),
			"occurrences",
			visit,
		);
		allocation(&self.anchors, self.anchors.capacity(), "anchors", visit);
	}
}

#[cfg(feature = "fuzz")]
impl LayoutSnapshot {
	pub fn visit_storage(&self, visit: &mut StorageVisitor<'_>) {
		allocation(&self.blocks, self.blocks.capacity(), "placements", visit);
		for block in &self.blocks {
			block.layout.visit_storage(visit);
			block.flow.visit_storage(visit);
		}
	}
}
