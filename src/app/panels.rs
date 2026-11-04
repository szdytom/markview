//! The panel shell's page state: scroll offsets, the scrollbar drag and the
//! sidebar resize drag.
//!
//! The pages' contents are their own models; this holds what every page of
//! the shell shares. `ExportSettings` is persisted configuration, so its
//! page offset never lives there.
use super::sidebar;
use markview_interaction::Scroll;

#[derive(Default)]
pub(crate) struct Model {
	export_scroll: Scroll,
	/// The Styles page's list offset. The export's stylesheet chooser shares
	/// it: no two pages of the panel are ever open at once.
	styles_scroll: Scroll,
	/// Offset from the centre of the panel scrollbar thumb while dragging.
	grab: Option<f32>,
	/// The resize drag on a sidebar's edge.
	sidebar: sidebar::State,
}

impl Model {
	pub(crate) fn export_scroll(&self) -> &Scroll {
		&self.export_scroll
	}

	pub(crate) fn export_scroll_mut(&mut self) -> &mut Scroll {
		&mut self.export_scroll
	}

	pub(crate) fn styles_scroll(&self) -> &Scroll {
		&self.styles_scroll
	}

	pub(crate) fn styles_scroll_mut(&mut self) -> &mut Scroll {
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
}
