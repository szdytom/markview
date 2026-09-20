//! Adapt application state to borrowed chrome inputs.
use super::{
	App, Button,
	chrome::{self, Chrome},
};
use crate::layout::Draw;
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
			style_page: self.preferences.style_page,
			fonts: &self.fonts,
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
			form.buttons.into_iter().filter(|b| b.enabled).collect()
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
	pub(super) fn set_panel_scroll(&mut self, scroll: f32) {
		if self.interaction.export_open {
			self.interaction.export_scroll = scroll;
		} else {
			self.interaction.settings_scroll = scroll;
		}
	}
	pub(super) fn scroll_panel(&mut self, delta: f32) {
		if let Some(form) = self.panel_form() {
			self.set_panel_scroll(
				(form.scroll + delta).clamp(0.0, form.max_scroll),
			);
			self.interaction.pressed = None;
			self.redraw();
		}
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
		let Some(bar) =
			self.panel_form().and_then(|form| form.scrollbar(&self.ui))
		else {
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
			&& let Some(bar) =
				self.panel_form().and_then(|form| form.scrollbar(&self.ui))
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
		if let Some(form) = self.panel_form() {
			self.set_panel_scroll(form.scroll);
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
