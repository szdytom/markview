use super::App;
use crate::layout::{Rect, Scrollbar};
use std::time::{Duration, Instant};

/// How long the document scrollbar stays on screen after the pointer moves
/// over the reading area.
const SCROLLBAR_LINGER: Duration = Duration::from_millis(1200);

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
		let dragging =
			self.interaction.pointer.scrollbar().is_some_and(|drag| {
				drag.target == crate::state::ScrollbarAxis::Document
			});
		let visible = self
			.scrollbar_visible_until
			.is_some_and(|until| until > Instant::now());
		if !dragging && !visible {
			return None;
		}
		self.document_scrollbar_geometry()
	}

	/// The document scrollbar wherever it can exist, regardless of whether it
	/// is currently shown. Pointer motion uses this to decide what may reveal
	/// it.
	pub(super) fn document_scrollbar_geometry(&self) -> Option<Scrollbar> {
		if self.interaction.panel_open()
			|| self.interaction.overlays.outline.open()
			|| self.readers.session.layout_pending
		{
			return None;
		}
		let (width, height, _) = self.dimensions();
		let metrics = self.preferences.values.stylesheet.scrollbar_metrics();
		let top = self.content_top();
		let track = Rect {
			x: width - metrics.thickness,
			y: top,
			w: metrics.thickness,
			h: (height - top - self.bottom()).max(0.0),
		};
		let viewport = self.viewport();
		// The bar spans the scrollable range, including the blank kept below
		// the document, so its thumb and the scroll clamp agree.
		let content =
			self.readers.session.snapshot.scroll_limit(viewport) + viewport;
		Scrollbar::vertical(
			track,
			self.readers.session.scrolling.offset,
			content,
			viewport,
			metrics,
		)
	}

	/// Whether a window point lies in the reading area the document bar
	/// belongs to.
	pub(super) fn reading_area_contains(&self, x: f32, y: f32) -> bool {
		let (width, height, _) = self.dimensions();
		x >= 0.0
			&& x <= width
			&& y >= self.content_top()
			&& y < height - self.bottom()
	}

	pub(super) fn show_document_scrollbar(&mut self, now: Instant) {
		self.scrollbar_visible_until = Some(now + SCROLLBAR_LINGER);
	}

	pub(super) fn hide_document_scrollbar(&mut self) {
		self.scrollbar_visible_until = None;
	}

	/// Expires the document bar's linger, or keeps it alive under a drag.
	pub(super) fn scrollbar_tick(&mut self, now: Instant) {
		let dragging =
			self.interaction.pointer.scrollbar().is_some_and(|drag| {
				drag.target == crate::state::ScrollbarAxis::Document
			});
		if dragging {
			self.show_document_scrollbar(now);
		} else if self
			.scrollbar_visible_until
			.is_some_and(|until| until <= now)
		{
			self.scrollbar_visible_until = None;
			self.refresh_hover();
			self.redraw();
		}
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
