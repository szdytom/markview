//! The panel shell's page state: scroll offsets, the scrollbar drag and the
//! sidebar resize drag.
//!
//! The pages' contents are their own models; this holds what every page of
//! the shell shares. `ExportSettings` is persisted configuration, so its
//! page offset never lives there.
use super::sidebar;
use crate::scroll::ScrollArea;
use std::time::Instant;

#[derive(Default)]
pub(crate) struct Model {
	export_scroll: ScrollArea,
	/// The Styles page's list offset. The export's stylesheet chooser shares
	/// it: no two pages of the panel are ever open at once.
	styles_scroll: ScrollArea,
	/// Offset from the centre of the panel scrollbar thumb while dragging.
	grab: Option<f32>,
	/// The resize drag on a sidebar's edge.
	sidebar: sidebar::State,
}

impl Model {
	pub(crate) fn export_scroll(&self) -> &ScrollArea {
		&self.export_scroll
	}

	pub(crate) fn export_scroll_mut(&mut self) -> &mut ScrollArea {
		&mut self.export_scroll
	}

	pub(crate) fn styles_scroll(&self) -> &ScrollArea {
		&self.styles_scroll
	}

	pub(crate) fn styles_scroll_mut(&mut self) -> &mut ScrollArea {
		&mut self.styles_scroll
	}

	pub(crate) fn grab(&self) -> Option<f32> {
		self.grab
	}

	pub(crate) fn set_grab(&mut self, grab: Option<f32>) {
		self.grab = grab;
	}

	pub(crate) fn sidebar(&self) -> &sidebar::State {
		&self.sidebar
	}

	pub(crate) fn sidebar_mut(&mut self) -> &mut sidebar::State {
		&mut self.sidebar
	}

	/// Advances eased scrolling on both page offsets.
	pub(crate) fn advance_scrolls(&mut self, now: Instant) {
		self.export_scroll.advance(now);
		self.styles_scroll.advance(now);
	}

	pub(crate) fn scroll_deadline(&self, now: Instant) -> Option<Instant> {
		[
			self.export_scroll.deadline(now),
			self.styles_scroll.deadline(now),
		]
		.into_iter()
		.flatten()
		.min()
	}

	pub(crate) fn scroll_animating(&self) -> bool {
		self.export_scroll.animating() || self.styles_scroll.animating()
	}
}
