use super::scroll_limit;
use crate::scene::Draw;
use crate::scene::LayoutSnapshot;
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
			if old.images.entries != new.images.entries {
				let local_y = scroll - anchor.y;
				let cluster = anchor
					.layout
					.text
					.iter()
					.enumerate()
					.flat_map(|(ni, n)| n.clusters.iter().map(move |c| (ni, c)))
					.filter(|(_, c)| c.rect.y + c.rect.h >= local_y)
					.min_by(|(_, a), (_, b)| {
						let a_image = matches!(
							anchor.layout.draws[a.command],
							Draw::Image { .. }
						);
						let b_image = matches!(
							anchor.layout.draws[b.command],
							Draw::Image { .. }
						);
						a_image.cmp(&b_image).then_with(|| {
							(a.rect.y - local_y)
								.abs()
								.total_cmp(&(b.rect.y - local_y).abs())
						})
					});
				if let Some((ni, c)) = cluster
					&& let Some(next) = b.layout.text.get(ni).and_then(|n| {
						n.clusters
							.iter()
							.find(|n| n.range.contains(&c.range.start))
					}) {
					return (b.y + next.rect.y + (local_y - c.rect.y))
						.clamp(0., max);
				}
			}
			return (b.y + (scroll - anchor.y).min(b.layout.height))
				.clamp(0.0, max);
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
				if let Some(b) = new.blocks.iter().find(|b| b.id == a.id) {
					return (b.y + scroll - a.y).clamp(0.0, max);
				}
			}
		}
	}
	scroll.clamp(0.0, max)
}
