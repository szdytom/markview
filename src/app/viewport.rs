use super::App;
use crate::layout::{Rect, Scrollbar};
use crate::state::{Animation, Fade};
use std::time::{Duration, Instant};

/// How long the document scrollbar stays on screen after scrolling or pointer
/// motion over the reading area.
pub(super) const SCROLLBAR_LINGER: Duration = Duration::from_millis(1200);
/// How long the document scrollbar takes to fade once its linger expires.
/// VS Code fades a hidden scrollbar over 800ms.
pub(super) const SCROLLBAR_FADE: Duration = Duration::from_millis(800);

/// The document or folder scrollbar's reveal state: hidden, lingering after
/// scrolling or pointer motion, or fading out.
#[derive(Default)]
pub(super) enum ScrollbarReveal {
	#[default]
	Hidden,
	Linger(Instant),
	Fade(Fade),
}

impl ScrollbarReveal {
	pub(super) fn opacity(&self, now: Instant) -> f32 {
		match self {
			Self::Hidden => 0.0,
			Self::Linger(until) => f32::from(now <= *until),
			Self::Fade(fade) => fade.value(now),
		}
	}

	pub(super) fn show(&mut self, now: Instant) {
		*self = Self::Linger(now + SCROLLBAR_LINGER);
	}

	pub(super) fn hide(&mut self, now: Instant) {
		if let Self::Linger(until) = self
			&& *until > now
		{
			*self = Self::Linger(now);
		}
	}

	pub(super) fn deadline(&self, now: Instant) -> Option<Instant> {
		match self {
			Self::Linger(until) if *until > now => Some(*until),
			_ => None,
		}
	}

	/// Expires the linger and starts the fade. Returns whether the paint
	/// changed.
	pub(super) fn tick(&mut self, now: Instant, dragging: bool) -> bool {
		if let Self::Fade(fade) = self
			&& fade.is_finished(now)
		{
			*self = Self::Hidden;
		}
		if dragging {
			self.show(now);
			return false;
		}
		if let Self::Linger(until) = *self
			&& now >= until
		{
			let fade = Fade::new(1.0, 0.0, until, SCROLLBAR_FADE);
			*self = if fade.is_finished(now) {
				Self::Hidden
			} else {
				Self::Fade(fade)
			};
			return true;
		}
		false
	}

	pub(super) fn advance(&mut self, now: Instant) -> bool {
		if let Self::Fade(fade) = self
			&& !fade.advance(now)
		{
			*self = Self::Hidden;
		}
		matches!(self, Self::Fade(_))
	}

	#[cfg(test)]
	pub(super) fn linger_until(&self) -> Option<Instant> {
		match self {
			Self::Linger(until) => Some(*until),
			_ => None,
		}
	}
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn document_interaction(
		&self,
	) -> markview_interaction::DocumentInteraction<'_> {
		markview_interaction::DocumentInteraction {
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
			&& self.scrollbar_opacity_at(self.frame_now()) <= 0.0
		{
			return None;
		}
		self.document_scrollbar_geometry()
	}

	/// The document scrollbar's opacity, from full while lingering to zero at
	/// the end of the fade. A drag keeps it fully opaque.
	pub(super) fn scrollbar_opacity(&self) -> f32 {
		self.scrollbar_opacity_at(self.frame_now())
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
		self.scrollbar_reveal.opacity(now)
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
		self.scrollbar_reveal.show(now);
	}

	/// Starts the document bar's fade now, or leaves it hidden.
	pub(super) fn hide_document_scrollbar(&mut self, now: Instant) {
		self.scrollbar_reveal.hide(now);
	}

	/// The document bar's linger expiry, which starts its fade.
	pub(super) fn scrollbar_deadline(&self, now: Instant) -> Option<Instant> {
		self.scrollbar_reveal.deadline(now)
	}

	/// Starts the document bar's fade when its linger expires. A drag keeps it
	/// fully visible instead.
	pub(super) fn scrollbar_tick(&mut self, now: Instant) {
		if self.scrollbar_reveal.tick(now, self.scrollbar_dragging()) {
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
			markview_interaction::Point::new(x, y),
			self.preferences
				.values
				.stylesheet
				.overflow_scrollbar_metrics(),
		)
	}
}
