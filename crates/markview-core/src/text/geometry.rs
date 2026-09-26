//! Text hit testing and clipped selection geometry.
use crate::layout::{LayoutSnapshot, Rect};
use std::{collections::HashMap, ops::Range};

use super::{Affinity, TextCluster, TextNode, TextPosition, TextSelection};
impl LayoutSnapshot {
	pub fn hit_test_text(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
		revision: u64,
	) -> Option<TextPosition> {
		let mut best = None;
		let mut distance = f32::INFINITY;
		// Blocks are ordered by y: only the one above the cursor's block can
		// still be nearer, and once a block is farther than the best score the
		// blocks below it can only be farther.
		let start = self
			.blocks
			.partition_point(|b| b.y + b.layout.height < y)
			.saturating_sub(1);
		for (bi, block) in self.blocks.iter().enumerate().skip(start) {
			let dy = (block.y - y)
				.max(0.0)
				.max(y - block.y - block.layout.height);
			if dy * dy * 10000.0 > distance {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				for cluster in &node.clusters {
					let Some(rect) = self.text_rect(bi, cluster, horizontal)
					else {
						continue;
					};
					let dy = (rect.y - y).max(0.0).max(y - rect.y - rect.h);
					let dx = (rect.x - x).max(0.0).max(x - rect.x - rect.w);
					let score = dy * dy * 10000.0 + dx * dx;
					if score < distance {
						distance = score;
						let (offset, _) = block.layout.command_view(
							cluster.command,
							bi,
							horizontal,
						);
						let after = (x
							>= cluster.rect.x - offset + cluster.rect.w * 0.5)
							!= cluster.rtl;
						best = Some(TextPosition {
							revision,
							block: bi,
							node: ni,
							offset: if after {
								cluster.range.end
							} else {
								cluster.range.start
							},
							affinity: if after {
								Affinity::After
							} else {
								Affinity::Before
							},
						});
					}
				}
			}
		}
		best
	}
	/// Returns whether a point is inside the laid-out bounds of a text cluster.
	/// Unlike `hit_test_text`, this does not snap through line spacing to the
	/// nearest cluster.
	pub fn contains_text(
		&self,
		x: f32,
		y: f32,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> bool {
		let start = self
			.blocks
			.partition_point(|block| block.y + block.layout.height < y)
			.saturating_sub(1);
		self.blocks
			.iter()
			.enumerate()
			.skip(start)
			.any(|(bi, block)| {
				if block.y > y {
					return false;
				}
				block.layout.text.iter().any(|node| {
					node.clusters.iter().any(|cluster| {
						self.text_rect(bi, cluster, horizontal)
							.is_some_and(|rect| rect.contains(x, y))
					})
				})
			})
	}
	pub(crate) fn text_rect(
		&self,
		bi: usize,
		cluster: &TextCluster,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<Rect> {
		let block = &self.blocks[bi];
		let mut rect = cluster.rect;
		let (offset, clip) =
			block.layout.command_view(cluster.command, bi, horizontal);
		rect.x -= offset;
		if let Some(clip) = clip {
			rect = rect.intersect(clip)?;
		}
		rect.y += block.y;
		Some(rect)
	}
	pub fn selection_rects(
		&self,
		selection: TextSelection,
		horizontal: &HashMap<(usize, usize), f32>,
		revision: u64,
	) -> Vec<Rect> {
		self.selection_rects_in(
			selection,
			horizontal,
			revision,
			f32::NEG_INFINITY..f32::INFINITY,
		)
	}
	/// Capacity of unique retained reading text and hit-test allocations; excludes allocator overhead.
	pub fn text_index_bytes(&self) -> usize {
		let mut seen = std::collections::HashSet::new();
		self.blocks
			.iter()
			.filter(|b| seen.insert(std::sync::Arc::as_ptr(&b.layout)))
			.map(|b| {
				b.layout.text.capacity() * std::mem::size_of::<TextNode>()
					+ b.layout
						.text
						.iter()
						.map(|n| {
							n.text.capacity()
								+ n.boundaries.capacity()
									* std::mem::size_of::<usize>()
								+ n.clusters.capacity()
									* std::mem::size_of::<TextCluster>()
						})
						.sum::<usize>()
			})
			.sum()
	}
	pub fn selection_rects_in(
		&self,
		selection: TextSelection,
		horizontal: &HashMap<(usize, usize), f32>,
		revision: u64,
		visible_y: Range<f32>,
	) -> Vec<Rect> {
		if selection.is_empty()
			|| selection.anchor.revision != revision
			|| selection.focus.revision != revision
		{
			return Vec::new();
		}
		let (a, b) = selection.ordered();
		let mut rects = Vec::new();
		let start = self
			.blocks
			.partition_point(|b| b.y + b.layout.height < visible_y.start);
		for (bi, block) in self.blocks.iter().enumerate().skip(start) {
			if block.y > visible_y.end {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				for cluster in &node.clusters {
					if (bi, ni, cluster.range.end) > a.key()
						&& (bi, ni, cluster.range.start) < b.key()
						&& let Some(rect) =
							self.text_rect(bi, cluster, horizontal)
					{
						if matches!(
							block.layout.draws.get(cluster.command),
							Some(crate::scene::Draw::Image { .. })
						) {
							rects.extend([
								Rect {
									x: rect.x - 2.,
									y: rect.y - 2.,
									w: rect.w + 4.,
									h: 2.,
								},
								Rect {
									x: rect.x - 2.,
									y: rect.y + rect.h,
									w: rect.w + 4.,
									h: 2.,
								},
								Rect {
									x: rect.x - 2.,
									y: rect.y,
									w: 2.,
									h: rect.h,
								},
								Rect {
									x: rect.x + rect.w,
									y: rect.y,
									w: 2.,
									h: rect.h,
								},
							]);
						} else {
							rects.push(rect);
						}
					}
				}
			}
		}
		rects
	}
}
