//! Adapt application state to borrowed chrome inputs.
use super::{
	App, Button,
	chrome::{self, Chrome},
};
use crate::layout::{Draw, Scrollbar};
impl App {
	/// The remote-image deferral count while its banner is worth showing.
	pub(super) fn remote_notice(&self) -> Option<usize> {
		self.readers.session.remote_notice()
	}

	/// Top of the document area, below the toolbar and any notice strip.
	pub(super) fn content_top(&self) -> f32 {
		super::chrome::content_top(self.remote_notice().is_some())
	}

	fn chrome(&mut self) -> Chrome<'_> {
		let (width, height, _) = self.dimensions();
		let scrollbar = self.document_scrollbar();
		let remote_notice = self.remote_notice();
		// An internal footnote jump has no external target to name, so the
		// footer stays empty while the pointer is over one.
		let hover_hint =
			self.interaction.hover_image.as_deref().or_else(|| {
				self.interaction
					.hover
					.as_deref()
					.filter(|url| !super::anchor::footnote_link(url))
			});
		Chrome {
			backend: self.renderer.as_ref().map(|renderer| renderer.backend),
			ui: &mut self.ui,
			session: &self.readers.session,
			tabs: self.readers.entries(),
			active_tab: self.readers.active(),
			tab_strip: &self.tab_strip,
			tab_widths: &self.tab_metrics.widths,
			settings: &self.preferences.values,
			export: &self.preferences.export,
			interaction: &self.interaction,
			style_entries: &self.preferences.style_entries,
			style_scroll: self.interaction.styles_scroll,
			fonts: self.font_panel.view(),
			width,
			height,
			scrollbar,
			warning: self
				.preferences
				.style_warning
				.as_deref()
				.or(self.preferences.settings_warning.as_deref()),
			status: &self.status,
			status_until: self.status_until,
			error: self.error,
			hover_hint,
			remote_notice,
			watching: self.watch_export.is_some(),
		}
	}
	pub(super) fn panel_form(&mut self) -> Option<chrome::components::Form> {
		self.chrome().form()
	}
	pub(super) fn focus_buttons(&mut self) -> Vec<Button> {
		if let Some(form) = self.panel_form() {
			// The settings header is not part of the scrolling form, but its
			// tabs are still reachable by keyboard: they lead the order.
			let mut buttons: Vec<Button> = self
				.buttons()
				.into_iter()
				.filter(|b| {
					matches!(b.action, crate::state::Command::SettingsTab(_))
				})
				.collect();
			buttons.extend(form.buttons.into_iter().filter(|b| b.enabled));
			buttons
		} else {
			let mut buttons: Vec<Button> = Vec::new();
			for button in self.buttons() {
				if !buttons.iter().any(|b| b.action == button.action) {
					buttons.push(button);
				}
			}
			buttons
		}
	}
	/// The scrolling list of whichever panel page shows one, if any.
	///
	/// The Styles page and the export's stylesheet chooser share their offset:
	/// no two pages of the panel are ever open at once.
	fn panel_list(&self) -> Option<chrome::list::List> {
		let (width, height, _) = self.dimensions();
		if self.interaction.styles_open()
			|| self.interaction.export_styles_open()
		{
			Some(chrome::styles::list(
				width,
				height,
				self.preferences.style_entries.len(),
				self.interaction.styles_scroll,
			))
		} else if self.interaction.fonts_open() {
			let fonts = self.font_panel.view();
			Some(super::font_panel::view::list(
				width,
				height,
				fonts.shown.len(),
				fonts.scroll,
			))
		} else {
			None
		}
	}
	/// The offset and limit of whichever panel page scrolls, already clamped.
	pub(super) fn panel_scroll_range(&mut self) -> Option<(f32, f32)> {
		if let Some(form) = self.panel_form() {
			return Some((form.scroll, form.max_scroll));
		}
		self.panel_list()
			.map(|list| (list.scroll, list.max_scroll()))
	}
	/// The scrollbar of whichever panel page scrolls.
	fn panel_bar(&mut self) -> Option<Scrollbar> {
		if let Some(form) = self.panel_form() {
			return form.scrollbar(&self.ui);
		}
		self.panel_list()?.scrollbar(&self.ui)
	}
	pub(super) fn set_panel_scroll(&mut self, scroll: f32) {
		if self.interaction.export_open()
			&& !self.interaction.export_styles_open()
		{
			self.interaction.export_scroll = scroll;
		} else if self.interaction.fonts_open() {
			self.font_panel.set_scroll(scroll);
		} else if self.interaction.styles_open()
			|| self.interaction.export_styles_open()
		{
			self.interaction.styles_scroll = scroll;
		} else {
			self.interaction.settings_scroll = scroll;
		}
	}
	pub(super) fn scroll_panel(&mut self, delta: f32) {
		let Some((scroll, max)) = self.panel_scroll_range() else {
			return;
		};
		self.set_panel_scroll((scroll + delta).clamp(0.0, max));
		self.interaction.pressed = None;
		self.redraw();
	}
	pub(super) fn reveal_panel_focus(&mut self) {
		if let Some(form) = self.panel_form() {
			let scroll = self
				.interaction
				.focus
				.map_or(form.scroll, |action| form.reveal(action));
			self.set_panel_scroll(scroll);
		}
	}
	pub(super) fn begin_panel_drag(&mut self) -> bool {
		let Some(bar) = self.panel_bar() else {
			return false;
		};
		let (x, y) = self.interaction.cursor;
		if !bar.hit(x, y) {
			return false;
		}
		let grab = if bar.on_thumb(x, y) {
			bar.grab(x, y)
		} else {
			self.set_panel_scroll(bar.scroll_for(x, y, 0.0));
			0.0
		};
		self.interaction.panel_grab = Some(grab);
		true
	}
	pub(super) fn drag_panel(&mut self) {
		if let Some(grab) = self.interaction.panel_grab
			&& let Some(bar) = self.panel_bar()
		{
			let (x, y) = self.interaction.cursor;
			self.set_panel_scroll(bar.scroll_for(x, y, grab));
			self.redraw();
		}
	}

	pub(super) fn buttons(&mut self) -> Vec<Button> {
		self.ensure_outline();
		self.chrome().buttons()
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		self.ensure_outline();
		self.normalize_tab_scroll();
		// The wheel, the scrollbar and the page all read one clamped offset.
		if let Some((scroll, _)) = self.panel_scroll_range() {
			self.set_panel_scroll(scroll);
		}
		let session = &self.readers.session;
		let selection = self.interaction.selection.filter(|s| {
			!s.is_empty()
				&& s.anchor.revision == session.accepted_revision
				&& s.focus.revision == session.accepted_revision
		});
		if self.interaction.selection_counts.map(|(s, _)| s) != selection {
			self.interaction.selection_counts = selection.map(|s| {
				(
					s,
					markview_core::text::TextCounts::of(
						&session
							.snapshot
							.extract_text(s, session.accepted_revision),
					),
				)
			});
		}
		self.chrome().overlay()
	}
	pub(super) fn tab_layout(&mut self) -> super::tab_strip::TabLayout {
		self.tab_metrics.sync(&mut self.ui, self.readers.entries());
		self.chrome().tab_bar().layout()
	}
}
