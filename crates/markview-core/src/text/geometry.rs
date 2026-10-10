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
			.partition_point(|b| b.y + b.height() < y)
			.saturating_sub(1);
		for (bi, block) in self.blocks.iter().enumerate().skip(start) {
			let dy = (block.y - y).max(0.0).max(y - block.y - block.height());
			if dy * dy * 10000.0 > distance {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				if !block.text_visible(ni) {
					continue;
				}
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
						let (offset, _) =
							block.command_view(cluster.command, bi, horizontal);
						// The nearest of the offsets the cluster may be split
						// at. A ligature sets several letters as one glyph, so
						// the pointer has to be able to land between them; a
						// base letter and its combining mark are one grapheme,
						// and an atomic formula or image is one box, so those
						// offer only the two edges.
						let width = cluster.rect.w;
						let along = if width > 0.0 {
							((x - (block
								.rect(cluster.command, cluster.rect)
								.unwrap()
								.x - offset)) / width)
								.clamp(0.0, 1.0)
						} else {
							0.5
						};
						let along =
							if cluster.rtl { 1.0 - along } else { along };
						let parts = node.parts(cluster);
						let part = (along * parts as f32).round() as usize;
						best = Some(TextPosition {
							revision,
							block: bi,
							node: ni,
							offset: node.split_at(cluster, part),
							affinity: if part == parts {
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
			.partition_point(|block| block.y + block.height() < y)
			.saturating_sub(1);
		self.blocks
			.iter()
			.enumerate()
			.skip(start)
			.any(|(bi, block)| {
				if block.y > y {
					return false;
				}
				block.layout.text.iter().enumerate().any(|(ni, node)| {
					block.text_visible(ni)
						&& node.clusters.iter().any(|cluster| {
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
		self.view_rect(bi, cluster.command, cluster.rect, horizontal)
	}
	/// Applies a block's scroll offset and overflow clip to a cluster-local
	/// rect, then moves it into block coordinates.
	fn view_rect(
		&self,
		bi: usize,
		command: usize,
		mut rect: Rect,
		horizontal: &HashMap<(usize, usize), f32>,
	) -> Option<Rect> {
		let block = &self.blocks[bi];
		let (offset, clip) = block.command_view(command, bi, horizontal);
		rect = block.rect(command, rect)?;
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
								+ n.source_images.capacity()
									* std::mem::size_of::<(usize, TextCluster)>(
									)
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
			.partition_point(|b| b.y + b.height() < visible_y.start);
		for (bi, block) in self.blocks.iter().enumerate().skip(start) {
			if block.y > visible_y.end {
				break;
			}
			for (ni, node) in block.layout.text.iter().enumerate() {
				if !block.text_visible(ni) {
					continue;
				}
				for cluster in &node.clusters {
					// A selection that stops inside a cluster covers only the
					// letters up to where it stops, so a ligature can be
					// highlighted in part. A cluster of one grapheme has no
					// part but all of it, which is the whole of a plain letter,
					// and an atomic formula or image is never split.
					let parts =
						node.covered_parts(cluster, (bi, ni), a.key(), b.key());
					if parts.is_empty() {
						continue;
					}
					// The selected share of the glyph's own advance is taken
					// before the viewport moves and clips it, so a partially
					// selected ligature keeps the letter it names even when
					// that letter sits outside the visible range.
					let mut rect = node.part_rect(cluster, parts);
					// Automatic spacing belongs between selected characters,
					// but outside the selection at either endpoint.
					let at = |offset| (bi, ni, offset);
					let starts = at(cluster.range.start) == a.key();
					let ends = at(cluster.range.end) == b.key();
					let (left, right) = if cluster.rtl {
						(ends, starts)
					} else {
						(starts, ends)
					};
					if left {
						rect.x += cluster.mixed_spacing.0;
						rect.w -= cluster.mixed_spacing.0;
					}
					if right {
						rect.w -= cluster.mixed_spacing.1;
					}
					let Some(rect) =
						self.view_rect(bi, cluster.command, rect, horizontal)
					else {
						continue;
					};
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
		rects
	}
}
