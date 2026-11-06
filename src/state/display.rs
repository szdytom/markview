//! The displayed geometry, overflow offsets and retained preview slice.
use crate::{
	layout::{Draw, LayoutSnapshot, Rect},
	worker::ReaderSnapshot,
};
use std::collections::{HashMap, HashSet};

#[derive(Default, Clone)]
pub(crate) struct DisplayState {
	pub(crate) snapshot: LayoutSnapshot,
	pub(crate) horizontal: HashMap<(usize, usize), f32>,
	pub(crate) viewport: f32,
	pub(crate) cached_slice: Option<crate::app::preview::CachedSlice>,
}

impl DisplayState {
	pub(crate) fn can_display(
		&self,
		reader: &ReaderSnapshot,
		scrolling: &markview_interaction::Scroll,
		viewport: f32,
	) -> bool {
		if let Some(cached) = &self.cached_slice {
			let mut retained = HashSet::new();
			for (index, block) in self.snapshot.blocks.iter().enumerate() {
				if block.y >= cached.range.end
					|| block.y + block.height() <= cached.range.start
				{
					continue;
				}
				let viewport = Rect {
					x: 0.,
					y: cached.range.start - block.y,
					w: self.snapshot.width.max(1.),
					h: cached.range.end - cached.range.start,
				};
				for (command, draw, _) in block.draws() {
					let Draw::Image { src, rect, .. } = draw else {
						continue;
					};
					let Some(mut rect) = block.rect(command, *rect) else {
						continue;
					};
					let (offset, clip) =
						block.command_view(command, index, &self.horizontal);
					rect.x -= offset;
					let clip = clip.map_or(Some(viewport), |clip| {
						viewport.intersect(clip)
					});
					if clip.and_then(|clip| rect.intersect(clip)).is_none() {
						continue;
					}
					retained.insert(src.as_str());
				}
			}
			// Only a picture in the retained slice can be blanked by the
			// replacement; an offscreen one must not hold the update back.
			let images_pending =
				self.snapshot.images.entries.iter().any(|(key, old)| {
					retained.contains(key.as_str())
						&& old.size.is_some()
						&& reader.layout.images.entries.get(key).is_some_and(
							|info| {
								(info.size.is_none() || info.loading)
									&& info.error.is_none()
							},
						)
				});
			if images_pending
				&& cached.covers(
					scrolling.offset,
					viewport,
					self.snapshot.height,
				) {
				return false;
			}
			return reader.complete
				|| cached
					.target(
						&self.snapshot,
						&reader.layout,
						scrolling.target.unwrap_or(scrolling.offset),
						viewport,
					)
					.is_some_and(|target| {
						reader.layout.height >= target + viewport
					});
		}
		if reader.complete {
			return true;
		}
		if self.snapshot.blocks.is_empty() {
			return reader.layout.height >= scrolling.offset + viewport;
		}
		let index = self
			.snapshot
			.blocks
			.partition_point(|b| b.y <= scrolling.offset)
			.saturating_sub(1);
		let anchor = &self.snapshot.blocks[index];
		let occurrence = self.snapshot.blocks[..index]
			.iter()
			.filter(|b| b.id == anchor.id)
			.count();
		reader
			.layout
			.blocks
			.iter()
			.filter(|b| b.id == anchor.id)
			.nth(occurrence)
			.is_some_and(|b| {
				b.y + (scrolling.offset - anchor.y).min(b.height()) + viewport
					<= reader.layout.height
			})
	}

	/// Restores overflow indices from the retained slice to the full document.
	pub(crate) fn take_cached_slice(
		&mut self,
	) -> Option<crate::app::preview::CachedSlice> {
		let cached = self.cached_slice.take();
		if let Some(cached) = &cached {
			self.horizontal = self
				.horizontal
				.drain()
				.map(|((block, overflow), offset)| {
					((block + cached.first, overflow), offset)
				})
				.collect();
		}
		cached
	}

	/// Releases geometry while keeping the viewport and overflow positions.
	pub(crate) fn release(&mut self) {
		self.cached_slice = None;
		self.snapshot = LayoutSnapshot::default();
	}
}
