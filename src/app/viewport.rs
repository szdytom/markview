use super::App;
use crate::layout::{Rect, Scrollbar};
use crate::state::scroll_limit;
impl<P: super::SendEvent> App<P> {
	pub(super) fn document_interaction(
		&self,
	) -> markview_selection::DocumentInteraction<'_> {
		markview_selection::DocumentInteraction {
			snapshot: &self.readers.session.snapshot,
			viewport: self.view_geometry(),
			horizontal: &self.readers.session.horizontal,
			revision: self.readers.session.accepted_revision,
		}
	}

	/// The document scrollbar while it is visible. Drawing and pointer
	/// handling share this geometry, so the thumb always agrees with what a
	/// press grabs.
	pub(super) fn document_scrollbar(&self) -> Option<Scrollbar> {
		if self.interaction.panel_open()
			|| self.interaction.overlays.outline.open()
			|| self.readers.session.layout_pending
		{
			return None;
		}
		let (width, height, _) = self.dimensions();
		let mut metrics =
			self.preferences.values.stylesheet.scrollbar_metrics();
		// Keep the document bar's painted width and grab zone fixed.
		metrics.thickness_hover = metrics.thickness;
		let band = metrics.band();
		let inset = if markview_platform::touch_inset() {
			0.0
		} else {
			6.0
		};
		let top = self.content_top();
		let track = Rect {
			x: width - band - inset,
			y: top,
			w: band,
			h: (height - top - self.bottom()).max(0.0),
		};
		let viewport = self.viewport();
		// The bar spans the scrollable range, including the blank kept below
		// the document, so its thumb and the scroll clamp agree.
		let content =
			scroll_limit(self.readers.session.snapshot.height, viewport)
				+ viewport;
		Scrollbar::vertical(
			track,
			self.readers.session.scrolling.offset,
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
		self.document_interaction().overflow_bar(
			block,
			overflow,
			self.preferences
				.values
				.stylesheet
				.overflow_scrollbar_metrics(),
		)
	}

	/// The horizontal scrollbar under a window point, with the block and
	/// overflow index it belongs to.
	pub(super) fn overflow_scrollbar_at(
		&self,
		x: f32,
		y: f32,
	) -> Option<(usize, usize, Scrollbar)> {
		self.document_interaction().overflow_bar_at(
			markview_selection::Point::new(x, y),
			self.preferences
				.values
				.stylesheet
				.overflow_scrollbar_metrics(),
		)
	}
}
