use super::scroll_limit;
use crate::scene::Draw;
use crate::scene::LayoutSnapshot;
use std::collections::HashMap;
pub fn anchored_scroll(
	old: &LayoutSnapshot,
	new: &LayoutSnapshot,
	scroll: f32,
	viewport: f32,
	follow: bool,
) -> f32 {
	let max = scroll_limit(new.height, viewport);
	if follow && scroll >= scroll_limit(old.height, viewport) - 3.0 {
		return max;
	}
	let index = old
		.blocks
		.partition_point(|b| b.y <= scroll)
		.saturating_sub(1);
	if let Some(anchor) = old.blocks.get(index) {
		let occurrence = old.blocks[..index]
			.iter()
			.filter(|b| b.id == anchor.id)
			.count();
		if let Some(b) = new
			.blocks
			.iter()
			.filter(|b| b.id == anchor.id)
			.nth(occurrence)
		{
			if old.images.entries != new.images.entries
				|| old.presentation_key != new.presentation_key
			{
				let local_y = scroll - anchor.y;
				let cluster = anchor
					.clusters()
					.filter(|(_, _, rect)| rect.y + rect.h >= local_y)
					.min_by(|(_, a, ar), (_, c, cr)| {
						let image = |c: &crate::text::TextCluster| {
							matches!(
								anchor.layout.draws[c.command],
								Draw::Image { .. }
							)
						};
						image(a).cmp(&image(c)).then_with(|| {
							(ar.y - local_y)
								.abs()
								.total_cmp(&(cr.y - local_y).abs())
						})
					});
				if let Some((ni, c, rect)) = cluster
					&& let Some(next) = b.layout.text.get(ni).and_then(|n| {
						n.clusters
							.iter()
							.find(|n| n.range.contains(&c.range.start))
					}) {
					if let Some(next_rect) = b.rect(next.command, next.rect) {
						return (b.y + next_rect.y + local_y - rect.y)
							.clamp(0., max);
					}
					if let Some(y) = b.visible_ancestor_y(next.command) {
						return (b.y + y).clamp(0., max);
					}
				}
			}
			return (b.y + (scroll - anchor.y).min(b.height())).clamp(0.0, max);
		}
		// Index each identity's first occurrence once for all fallback neighbors.
		let mut first = HashMap::new();
		for block in &new.blocks {
			first.entry(block.id).or_insert(block);
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
				if let Some(b) = first.get(&a.id) {
					return (b.y + scroll - a.y).clamp(0.0, max);
				}
			}
		}
	}
	scroll.clamp(0.0, max)
}
