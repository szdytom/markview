use super::{App, BOTTOM};
use crate::layout::{Rect, Scrollbar};
use crate::state::scroll_limit;
impl App {
	/// The document scrollbar while it is visible. Drawing and pointer
	/// handling share this geometry, so the thumb always agrees with what a
	/// press grabs.
	pub(super) fn document_scrollbar(&self) -> Option<Scrollbar> {
		if self.interaction.panel_open || self.readers.session.layout_pending {
			return None;
		}
		let (width, height, _) = self.dimensions();
		let metrics = self.preferences.values.stylesheet.scrollbar_metrics();
		let band = metrics.band();
		let top = self.content_top();
		let track = Rect {
			x: width - band - 2.0,
			y: top,
			w: band,
			h: (height - top - BOTTOM).max(0.0),
		};
		let viewport = self.viewport();
		// The bar spans the scrollable range, including the blank kept below
		// the document, so its thumb and the scroll clamp agree.
		let content =
			scroll_limit(self.readers.session.snapshot.height, viewport)
				+ viewport;
		Scrollbar::vertical(
			track,
			self.readers.session.scroll,
			content,
			viewport,
			metrics,
		)
	}

	/// The horizontal scrollbar of one overflowing block, in window
	/// coordinates. The bar sits in the gutter the layout reserved below the
	/// block's content.
	pub(super) fn overflow_scrollbar(
		&self,
		block: usize,
		overflow: usize,
	) -> Option<Scrollbar> {
		let geometry = self.view_geometry();
		let placed = self.readers.session.snapshot.blocks.get(block)?;
		let o = placed.layout.overflow.get(overflow)?;
		let metrics = self
			.preferences
			.values
			.stylesheet
			.overflow_scrollbar_metrics();
		let track = Rect {
			x: geometry.left + o.rect.x,
			y: geometry.top - geometry.scroll + placed.y + o.rect.y + o.rect.h,
			w: o.rect.w,
			h: metrics.overflow_band(o.gutter),
		};
		Scrollbar::horizontal(
			track,
			self.readers
				.session
				.horizontal
				.get(&(block, overflow))
				.copied()
				.unwrap_or(0.0),
			o.content_width,
			o.rect.w,
			metrics,
		)
	}

	/// The horizontal scrollbar under a window point, with the block and
	/// overflow index it belongs to.
	pub(super) fn overflow_scrollbar_at(
		&self,
		x: f32,
		y: f32,
	) -> Option<(usize, usize, Scrollbar)> {
		let geometry = self.view_geometry();
		if !geometry.clip().contains(x, y) {
			return None;
		}
		let (_, dy) = geometry.document_point(x, y);
		for (block, placed) in
			self.readers.session.snapshot.blocks.iter().enumerate()
		{
			let local = dy - placed.y;
			if local < 0.0 || local > placed.layout.height {
				continue;
			}
			for overflow in 0..placed.layout.overflow.len() {
				if let Some(bar) = self.overflow_scrollbar(block, overflow)
					&& bar.hit(x, y)
				{
					return Some((block, overflow, bar));
				}
			}
		}
		None
	}
}
