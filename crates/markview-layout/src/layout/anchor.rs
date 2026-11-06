use crate::scene::Draw;
use crate::scene::LayoutSnapshot;
use std::collections::HashMap;

/// Maps an offset inside `anchor` onto the matching text in `next`.
///
/// Prefers text under the offset over images, so text below a resized picture
/// keeps its place when the block above it grows.
pub fn intra_block_anchor(
	anchor: &crate::scene::PlacedBlock,
	next: &crate::scene::PlacedBlock,
	local_y: f32,
) -> Option<f32> {
	let (ni, c, rect) = anchor
		.clusters()
		.filter(|(_, _, rect)| rect.y + rect.h >= local_y)
		.min_by(|(_, a, ar), (_, c, cr)| {
			let image = |c: &crate::text::TextCluster| {
				matches!(anchor.layout.draws.at(c.command), Draw::Image { .. })
			};
			image(a).cmp(&image(c)).then_with(|| {
				(ar.y - local_y).abs().total_cmp(&(cr.y - local_y).abs())
			})
		})?;
	let cluster = next
		.layout
		.text
		.get(ni)?
		.clusters
		.iter()
		.find(|n| n.range.contains(&c.range.start))?;
	next.rect(cluster.command, cluster.rect)
		.map(|r| r.y + local_y - rect.y)
		.or_else(|| next.visible_ancestor_y(cluster.command))
}

pub fn anchored_scroll(
	old: &LayoutSnapshot,
	new: &LayoutSnapshot,
	scroll: f32,
	viewport: f32,
	follow: bool,
) -> f32 {
	let max = new.scroll_limit(viewport);
	if follow && scroll >= old.scroll_limit(viewport) - 3.0 {
		return max;
	}
	let index = old
		.blocks
		.partition_point(|b| b.y <= scroll)
		.saturating_sub(1);
	if let Some(anchor) = old.blocks.get(index) {
		let same = |a: &crate::scene::PlacedBlock,
		            b: &crate::scene::PlacedBlock| {
			if old.revision == new.revision {
				a.id == b.id
			} else {
				a.content == b.content
			}
		};
		let occurrence = old.blocks[..index]
			.iter()
			.filter(|b| same(b, anchor))
			.count();
		if let Some(b) = new
			.blocks
			.iter()
			.filter(|b| same(b, anchor))
			.nth(occurrence)
		{
			let relaid = old.images.entries.iter().any(|(key, info)| {
				new.images.entries.get(key).map(|next| next.size)
					!= Some(info.size)
			}) || old.presentation_key != new.presentation_key;
			if relaid
				&& let Some(y) =
					intra_block_anchor(anchor, b, scroll - anchor.y)
			{
				return (b.y + y).clamp(0., max);
			}
			return (b.y + (scroll - anchor.y).min(b.height())).clamp(0.0, max);
		}
		// Index each identity's first occurrence once for all fallback neighbors.
		let mut first = HashMap::new();
		let key = |block: &crate::scene::PlacedBlock| {
			if old.revision == new.revision {
				block.id.get() as u64
			} else {
				block.content.get()
			}
		};
		for block in &new.blocks {
			first.entry(key(block)).or_insert(block);
		}
		for delta in 1..=old.blocks.len() {
			for neighbor in [
				index.checked_sub(delta),
				index.checked_add(delta).filter(|&n| n < old.blocks.len()),
			]
			.into_iter()
			.flatten()
			{
				let a = &old.blocks[neighbor];
				if let Some(b) = first.get(&key(a)).filter(|b| same(b, a)) {
					return (b.y + scroll - a.y).clamp(0.0, max);
				}
			}
		}
	}
	scroll.clamp(0.0, max)
}
