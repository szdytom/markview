use super::App;
use crate::layout::{Rect, Scrollbar};
use std::time::{Duration, Instant};

/// How long the document scrollbar stays on screen after the pointer moves
/// over the reading area.
pub(super) const SCROLLBAR_LINGER: Duration = Duration::from_millis(1200);
/// How long the document scrollbar takes to fade once its linger expires.
/// VS Code fades a hidden scrollbar over 800ms.
pub(super) const SCROLLBAR_FADE: Duration = Duration::from_millis(800);
/// One fade frame. The fade is an animation, so it asks for the next tick.
pub(super) const SCROLLBAR_FRAME: Duration = Duration::from_millis(16);

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
		if !self.scrollbar_dragging()
			&& self.scrollbar_opacity_at(Instant::now()) <= 0.0
		{
			return None;
		}
		self.document_scrollbar_geometry()
	}

	/// The document scrollbar's opacity, from full while lingering to zero at
	/// the end of the fade. A drag keeps it fully opaque.
	pub(super) fn scrollbar_opacity(&self) -> f32 {
		self.scrollbar_opacity_at(Instant::now())
	}

	fn scrollbar_dragging(&self) -> bool {
		self.interaction.pointer.scrollbar().is_some_and(|drag| {
			drag.target == crate::state::ScrollbarAxis::Document
		})
	}

	pub(super) fn scrollbar_opacity_at(&self, now: Instant) -> f32 {
		if self.scrollbar_dragging() {
			return 1.0;
		}
		let Some(until) = self.scrollbar_visible_until else {
			return 0.0;
		};
		let faded = now.saturating_duration_since(until);
		if faded.is_zero() {
			1.0
		} else {
			(1.0 - faded.as_secs_f32() / SCROLLBAR_FADE.as_secs_f32()).max(0.0)
		}
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
		x >= self.folder_chrome_inset()
			&& x <= width
			&& y >= self.content_top()
			&& y < height - self.bottom()
	}

	pub(super) fn show_document_scrollbar(&mut self, now: Instant) {
		self.scrollbar_visible_until = Some(now + SCROLLBAR_LINGER);
	}

	/// Starts the document bar's fade now, or leaves it hidden.
	pub(super) fn hide_document_scrollbar(&mut self, now: Instant) {
		if self
			.scrollbar_visible_until
			.is_some_and(|until| until > now)
		{
			self.scrollbar_visible_until = Some(now);
		}
	}

	/// The next frame the document bar's linger or fade needs.
	pub(super) fn scrollbar_deadline(&self, now: Instant) -> Option<Instant> {
		let until = self.scrollbar_visible_until?;
		if until > now {
			Some(until)
		} else {
			let end = until + SCROLLBAR_FADE;
			(end > now).then(|| (now + SCROLLBAR_FRAME).min(end))
		}
	}

	/// Expires the document bar's linger, or keeps it alive under a drag.
	pub(super) fn scrollbar_tick(&mut self, now: Instant) {
		if self.scrollbar_dragging() {
			self.show_document_scrollbar(now);
		} else if let Some(until) = self.scrollbar_visible_until {
			let end = until + SCROLLBAR_FADE;
			if now >= end {
				self.scrollbar_visible_until = None;
				self.refresh_hover();
				self.redraw();
			} else if now >= until {
				self.redraw();
			}
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
