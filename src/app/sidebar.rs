//! Sidebar navigation, width preferences and desktop pointer capture.
use super::{App, chrome};
use crate::{
	layout::Rect,
	settings::{Setting, Sidebar},
	state::{PanelPage, Selection, SidebarTab},
};

#[derive(Clone, Copy)]
pub(crate) struct Drag {
	sidebar: Sidebar,
	start_x: f32,
	start_width: f32,
}

/// The resize drag a sidebar's edge owns while the pointer holds it.
#[derive(Default)]
pub(crate) struct State {
	drag: Option<Drag>,
}

impl State {
	pub(crate) fn drag(&self) -> Option<Drag> {
		self.drag
	}

	pub(crate) fn begin(&mut self, drag: Drag) {
		self.drag = Some(drag);
	}

	pub(crate) fn take(&mut self) -> Option<Drag> {
		self.drag.take()
	}

	pub(crate) fn clear(&mut self) {
		self.drag = None;
	}
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn phone_folder_open(&self) -> bool {
		self.tab_strip.phone
			&& self.interaction.panel == PanelPage::Tabs
			&& self.interaction.sidebar_tab == SidebarTab::Folder
	}

	pub(super) fn switch_sidebar_tab(&mut self, tab: SidebarTab) {
		if !self.tab_strip.phone {
			return;
		}
		self.interaction.sidebar_tab = tab;
		self.folder.open = false;
		self.folder.stop_animation();
		self.show_panel(PanelPage::Tabs);
		self.interaction.widgets.focus =
			Some(crate::state::Command::SidebarTab(tab).widget_id());
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn toggle_phone_sidebar(&mut self) {
		if !self.tab_strip.phone {
			return;
		}
		self.readers.session.cancel_scroll_animation();
		self.tab_strip.cancel_drag();
		if self.interaction.panel == PanelPage::Tabs {
			self.show_panel(PanelPage::Closed);
		} else {
			self.switch_sidebar_tab(self.interaction.sidebar_tab);
			if self.interaction.sidebar_tab == SidebarTab::Tabs
				&& self.readers.session.path.is_some()
			{
				let (width, height, _) = self.dimensions();
				let list = chrome::tab_drawer::list(
					width,
					height,
					self.readers.entries().len(),
					self.tab_strip.drawer_scroll.offset(),
					self.preferences.values.tabs_width,
				);
				let row = list.row_rect(self.readers.active());
				self.tab_strip.drawer_scroll.set_max(list.max_scroll());
				self.tab_strip.drawer_scroll.set(
					(list.scroll
						+ (row.y - list.viewport.y).min(0.)
						+ (row.y + row.h - list.viewport.y - list.viewport.h)
							.max(0.))
					.clamp(0., list.max_scroll()),
				);
			}
		}
		self.refresh_hover();
		self.redraw();
	}

	fn sidebar_rect(&self, sidebar: Sidebar) -> Rect {
		match sidebar {
			Sidebar::Folder => self.folder_surface(),
			Sidebar::Outline => self.outline_drawer(),
			Sidebar::Tabs => {
				let (width, height, _) = self.dimensions();
				chrome::tab_drawer::animated_rect(
					width,
					height,
					self.tabs_reveal(),
					self.preferences.values.tabs_width,
				)
			}
		}
	}

	pub(super) fn sidebar_edge(&self) -> Option<Sidebar> {
		if markview_platform::touch_layout()
			|| self.interaction.overlays.modal_open()
			|| self.interaction.viewer.is_some()
			|| self.interaction.dropdown.is_some()
		{
			return None;
		}
		let sidebar = if self.interaction.panel == PanelPage::Tabs {
			(self.tabs_reveal() == 1.0).then_some(Sidebar::Tabs)
		} else if self.interaction.panel_open() {
			None
		} else {
			self.reading_sidebar_edge()
		}?;
		self.on_sidebar_edge(sidebar).then_some(sidebar)
	}

	fn reading_sidebar_edge(&self) -> Option<Sidebar> {
		if self.interaction.overlays.outline.open()
			&& self.interaction.overlays.outline.reveal() == 1.0
			&& self.on_sidebar_edge(Sidebar::Outline)
			&& !self.pointer_in_folder()
		{
			Some(Sidebar::Outline)
		} else if self.folder.open
			&& self.folder.tree.is_some()
			&& self.folder.reveal() == 1.0
		{
			Some(Sidebar::Folder)
		} else {
			None
		}
	}

	fn on_sidebar_edge(&self, sidebar: Sidebar) -> bool {
		let rect = self.sidebar_rect(sidebar);
		let edge = if sidebar == Sidebar::Outline {
			rect.x
		} else {
			rect.x + rect.w
		};
		let (x, y) = self.interaction.pointer.cursor();
		(x - edge).abs() <= 4.0 && y >= rect.y && y < rect.y + rect.h
	}

	pub(super) fn begin_sidebar_resize(&mut self) -> bool {
		let Some(sidebar) = self.sidebar_edge() else {
			return false;
		};
		self.interaction.reset_clicks();
		self.interaction.pointer.release();
		self.interaction.widgets.focus = None;
		let drag = Drag {
			sidebar,
			start_x: self.interaction.pointer.cursor().0,
			start_width: self.sidebar_rect(sidebar).w,
		};
		self.interaction.panels.sidebar_mut().begin(drag);
		self.refresh_hover();
		true
	}

	pub(super) fn drag_sidebar(&mut self) {
		let Some(drag) = self.interaction.panels.sidebar().drag() else {
			return;
		};
		let delta = self.interaction.pointer.cursor().0 - drag.start_x;
		let direction = if drag.sidebar == Sidebar::Outline {
			-1.0
		} else {
			1.0
		};
		let window_width = self.dimensions().0;
		let maximum = match drag.sidebar {
			Sidebar::Folder => {
				chrome::folder::rect(
					window_width,
					0.,
					0.,
					0.,
					crate::settings::SIDEBAR_WIDTH_MAX,
				)
				.w
			}
			Sidebar::Outline => window_width - self.folder_inset(),
			Sidebar::Tabs => window_width - 48.,
		};
		let width = (drag.start_width + direction * delta)
			.min(maximum.max(crate::settings::SIDEBAR_WIDTH_MIN));
		let old = self.preferences.values.sidebar_width(drag.sidebar);
		self.preferences
			.values
			.set_sidebar_width(drag.sidebar, width.round());
		if self.preferences.values.sidebar_width(drag.sidebar) != old {
			self.setting_changed(Some(Setting::SidebarWidth(drag.sidebar)));
			self.sidebar_resized(drag.sidebar);
			self.redraw();
		}
	}

	pub(super) fn sidebar_resized(&mut self, sidebar: Sidebar) {
		if sidebar == Sidebar::Folder && self.folder_inset() > 0.0 {
			self.request(false);
		}
	}
}
