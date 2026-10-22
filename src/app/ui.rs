//! Adapt application state to borrowed chrome inputs.
use super::{
	App, Button,
	chrome::{self, Chrome},
};
use crate::layout::{Draw, Scrollbar};
use std::time::Instant;
impl<P: super::SendEvent> App<P> {
	pub(super) fn page_command(
		&mut self,
		action: crate::state::Command,
	) -> bool {
		use super::{settings_panel, ui_model::Message};
		use crate::state::Command;
		if self.tab_strip.phone
			&& matches!(
				action,
				Command::Narrower | Command::Wider | Command::TabStyle(_)
			) {
			return true;
		}
		let message = match action {
			Command::Reset => {
				let mut values = crate::settings::ReaderSettings::default();
				if let Some(theme) = self.system_theme() {
					values.theme = theme;
				}
				Some(Message::Settings(settings_panel::Message::Reset(
					Box::new(values),
				)))
			}
			Command::StyleToggle(index)
			| Command::StyleUp(index)
			| Command::StyleDown(index)
			| Command::ExportStyleToggle(index)
			| Command::ExportStyleUp(index)
			| Command::ExportStyleDown(index) => {
				let export = matches!(
					action,
					Command::ExportStyleToggle(_)
						| Command::ExportStyleUp(_)
						| Command::ExportStyleDown(_)
				);
				let entries = if export {
					&self.settings_resources.export_entries
				} else {
					&self.preferences.style_entries
				};
				let Some(entry) = entries.get(index) else {
					return true;
				};
				let operation = match action {
					Command::StyleToggle(_) | Command::ExportStyleToggle(_) => {
						super::style_edit::Operation::Toggle
					}
					Command::StyleUp(_) | Command::ExportStyleUp(_) => {
						super::style_edit::Operation::Up
					}
					_ => super::style_edit::Operation::Down,
				};
				let edit = super::style_edit::Edit {
					id: entry.id.clone(),
					operation,
					valid: entry.error.is_none(),
				};
				Some(if export {
					Message::Export(super::export_panel::Message::Style(edit))
				} else {
					Message::Settings(settings_panel::Message::Style(edit))
				})
			}
			Command::FontFamily(role, selection) => {
				let family = if let Some(selection) = selection {
					let Some(name) =
						self.preferences.fonts.resolve(role, selection)
					else {
						return true;
					};
					Some(name)
				} else {
					None
				};
				Some(Message::Settings(settings_panel::Message::FontFamily(
					role, family,
				)))
			}
			_ => settings_panel::Message::from_command(action)
				.map(Message::Settings)
				.or_else(|| {
					super::export_panel::Message::from_command(action)
						.map(Message::Export)
				}),
		};
		let Some(message) = message else {
			return false;
		};
		self.dispatch_ui(message);
		true
	}

	pub(super) fn dispatch_ui(&mut self, message: super::ui_model::Message) {
		use super::{settings_panel, ui_model::Effect};
		self.preferences.ui.send(message);
		let update = self.preferences.ui.flush();
		for effect in update.effects {
			match effect {
				Effect::Settings(effect) => match effect {
					settings_panel::Effect::Preview(value) => {
						self.interaction.settings_preview = value
					}
					settings_panel::Effect::Persist(field) => {
						self.setting_changed(field)
					}
					settings_panel::Effect::Reflow => self.request(false),
					settings_panel::Effect::SidebarResize(sidebar) => {
						self.sidebar_resized(sidebar)
					}
					settings_panel::Effect::ReloadStyles => {
						self.reload_styles()
					}
					settings_panel::Effect::CloseDropdown => {
						self.close_dropdown()
					}
					settings_panel::Effect::SyncSession => {
						self.sync_session_settings()
					}
					settings_panel::Effect::Flush => {
						self.flush_settings();
					}
				},
				Effect::PersistExport => {
					let export = self.preferences.export.clone();
					self.preferences.set_export(export);
					self.refresh_settings_resources(false);
				}
				Effect::Fonts(effect) => self.font_effect(effect),
			}
		}
		if update.invalidation != markview_ui_core::Invalidation::None {
			self.refresh_hover();
			self.redraw();
		}
	}
	/// The remote-image deferral count while its banner is worth showing.
	pub(super) fn remote_notice(&self) -> Option<usize> {
		self.readers.session.remote_notice()
	}

	/// Top of the document area, below the toolbar and any notice strip.
	pub(super) fn content_top(&self) -> f32 {
		super::chrome::content_top(self.remote_notice().is_some())
	}

	pub(super) fn chrome(&mut self) -> Chrome<'_> {
		let (width, height, _) = self.dimensions();
		let frame = self.frame_layout();
		let scrollbar = self.document_scrollbar();
		let remote_notice = self.remote_notice();
		// A warning is stored as the failure it is, not as text: it is raised
		// before the interface knows which language it is drawn in, so it is
		// spelled here, where that language is known.
		let lang = self.preferences.values.lang();
		let style_warning = self
			.preferences
			.style_warning
			.as_ref()
			.map(|warning| warning.text(lang));
		let settings_warning = self
			.preferences
			.settings_warning
			.as_ref()
			.map(|warning| warning.text(lang));
		let warning = style_warning.or(settings_warning);
		// An internal footnote jump has no external target to name, so the
		// footer stays empty while the pointer is over one.
		let hover_hint =
			self.interaction.hover_image.as_deref().or_else(|| {
				self.interaction
					.hover
					.as_ref()
					.map(|link| link.url.as_str())
					.filter(|url| !super::anchor::footnote_link(url))
			});
		let resource_kind = self.settings_kind();
		let show_snapshot = resource_kind.is_none_or(|kind| {
			let load = self.settings_resources.load(kind);
			load.displayed && load.cached
		});
		let view = self.preferences.ui.view();
		let mut fonts = view.fonts;
		if !show_snapshot {
			fonts.catalog = &[];
			fonts.shown.clear();
		}

		Chrome {
			folder: Some(&self.folder),
			input_draws: Vec::new(),
			backend: self.renderer.as_ref().map(|renderer| renderer.backend),
			frame,
			ui: &mut self.ui,
			session: &self.readers.session,
			tabs: self.readers.entries(),
			active_tab: self.readers.active(),
			tab_strip: &self.tab_strip,
			tab_widths: &self.tab_metrics.widths,
			preview: self.preview.open(),
			restart_pending: self.preferences.restart_pending(),
			settings: view.settings,
			font_config: &self.fonts_config,
			export: view.export,
			interaction: &self.interaction,
			style_entries: if !show_snapshot {
				&[]
			} else if self.interaction.export_styles_open() {
				&self.settings_resources.export_entries
			} else {
				&self.preferences.style_entries
			},
			resource_load: resource_kind
				.map(|kind| self.settings_resources.load(kind)),
			style_scroll: self.interaction.styles_scroll.offset(),
			fonts,
			width,
			height,
			scrollbar,
			warning,
			status: &self.status,
			hover_hint,
			remote_notice,
			watching: self.export.watching(),
		}
	}
	pub(super) fn panel_form(&mut self) -> Option<chrome::components::Form> {
		self.chrome().form()
	}
	pub(super) fn focus_buttons(&mut self) -> Vec<Button> {
		self.ui_layout().focus
	}
	/// The scrolling list of whichever panel page shows one, if any.
	///
	/// The Styles page and the export's stylesheet chooser share their offset:
	/// no two pages of the panel are ever open at once.
	fn panel_list(&self) -> Option<chrome::list::List> {
		let (width, height, _) = self.dimensions();
		if self.phone_folder_open() {
			Some(chrome::folder::list(
				self.folder_rect(),
				self.folder.tree.as_ref().map_or(0, |t| t.rows().len()),
				self.folder.scroll.offset(),
			))
		} else if self.interaction.panel == crate::state::PanelPage::Tabs {
			let mut list = chrome::tab_drawer::list(
				width,
				height,
				self.readers.entries().len(),
				self.tab_strip.drawer_scroll.offset(),
				self.preferences.values.tabs_width,
			);
			let offset = chrome::tab_drawer::animated_rect(
				width,
				height,
				self.interaction.tabs_reveal(),
				self.preferences.values.tabs_width,
			)
			.x;
			list.panel.x += offset;
			list.viewport.x += offset;
			Some(list)
		} else if self.interaction.styles_open()
			|| self.interaction.export_styles_open()
		{
			Some(chrome::styles::list(
				width,
				height,
				self.preferences.style_entries.len(),
				self.interaction.styles_scroll.offset(),
			))
		} else if self.interaction.fonts_open() {
			let fonts = self.preferences.fonts.view();
			Some(super::font_panel::view::list(
				width,
				height,
				fonts.shown.len(),
				fonts.scroll,
				fonts.choosers,
				super::font_panel::view::roles(&self.preferences.values).len(),
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
	pub(super) fn panel_bar(&mut self) -> Option<Scrollbar> {
		if let Some(form) = self.panel_form() {
			return form.scrollbar(&self.ui);
		}
		self.panel_list()?.scrollbar(&self.ui)
	}
	/// The offset owner of whichever panel page scrolls.
	fn panel_scroll_area(&mut self) -> Option<&mut crate::scroll::ScrollArea> {
		if self.phone_folder_open() {
			Some(&mut self.folder.scroll)
		} else if self.interaction.panel == crate::state::PanelPage::Tabs {
			Some(&mut self.tab_strip.drawer_scroll)
		} else if self.interaction.export_open()
			&& !self.interaction.export_styles_open()
		{
			Some(&mut self.interaction.export_scroll)
		} else if self.interaction.fonts_open() {
			Some(self.preferences.fonts.scroll_area())
		} else if self.interaction.styles_open()
			|| self.interaction.export_styles_open()
		{
			Some(&mut self.interaction.styles_scroll)
		} else if self.interaction.panel_open() {
			Some(&mut self.interaction.settings_scroll)
		} else {
			None
		}
	}
	pub(super) fn set_panel_scroll(&mut self, scroll: f32, max: f32) {
		if let Some(area) = self.panel_scroll_area() {
			area.set_max(max);
			area.set(scroll);
		}
	}
	/// Stops the open page's easing where it is, for a new input owner.
	pub(super) fn cancel_panel_scroll(&mut self) {
		if let Some(area) = self.panel_scroll_area() {
			area.cancel();
		}
	}
	pub(super) fn scroll_panel(&mut self, delta: f32) {
		// The open list owns the wheel: the page keeps its scroll, so the row
		// its list hangs from cannot move out from under it.
		if self.interaction.dropdown.is_some() {
			return;
		}
		let Some((scroll, max)) = self.panel_scroll_range() else {
			return;
		};
		self.set_panel_scroll((scroll + delta).clamp(0.0, max), max);
		self.interaction.widgets.captured = None;
		self.redraw();
	}
	/// A wheel step over the open panel page, eased like the document's own.
	pub(super) fn wheel_panel(&mut self, delta: f32) {
		if self.interaction.dropdown.is_some() {
			return;
		}
		let Some((_, max)) = self.panel_scroll_range() else {
			return;
		};
		if let Some(area) = self.panel_scroll_area() {
			area.set_max(max);
			area.wheel(delta, Instant::now());
		}
		self.interaction.widgets.captured = None;
		self.redraw();
	}
	pub(super) fn reveal_panel_focus(&mut self) {
		if let Some(form) = self.panel_form() {
			let scroll = self
				.interaction
				.widgets
				.focus
				.map_or(form.scroll, |id| form.reveal(id));
			let max = form.max_scroll;
			self.set_panel_scroll(scroll, max);
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
		// The thumb follows the pointer now, not the ease it interrupted.
		if let Some(area) = self.panel_scroll_area() {
			area.cancel();
		}
		let grab = if bar.on_thumb(x, y) {
			bar.grab(x, y)
		} else {
			self.set_panel_scroll(bar.scroll_for(x, y, 0.0), bar.max_scroll());
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
			self.set_panel_scroll(bar.scroll_for(x, y, grab), bar.max_scroll());
			self.redraw();
		}
	}
	/// Advances eased scroll on every control surface.
	pub(super) fn advance_control_scroll(&mut self, now: Instant) {
		if !self.control_scroll_animating() {
			return;
		}
		self.interaction.advance_scrolls(now);
		self.folder.scroll.advance(now);
		let strip_moving = self.tab_strip.scroll.animating();
		self.tab_strip.advance_scrolls(now);
		if strip_moving && self.tab_strip.drag.is_some() {
			// Tabs have moved under the held pointer; keep the dragged tab
			// where the pointer now sits before the next frame is drawn.
			self.move_tab_drag();
		}
		self.preferences.fonts.advance_scroll(now);
		self.refresh_hover();
		self.redraw();
	}
	pub(super) fn control_scroll_animating(&self) -> bool {
		self.interaction.scroll_animating()
			|| self.folder.scroll.animating()
			|| self.tab_strip.scroll_animating()
			|| self.preferences.fonts.scroll_area_ref().animating()
	}
	/// The earliest frame any control surface needs.
	pub(super) fn control_scroll_deadline(
		&self,
		now: Instant,
	) -> Option<Instant> {
		self.interaction
			.scroll_deadline(now)
			.into_iter()
			.chain(self.folder.scroll.deadline(now))
			.chain(self.tab_strip.scroll_deadline(now))
			.chain(self.preferences.fonts.scroll_deadline(now))
			.min()
	}

	pub(super) fn ui_layout(&mut self) -> super::widgets::UiLayout {
		self.ensure_outline();
		let mut buttons = self.dropdown_buttons();
		let form = self.panel_form();
		buttons.extend(self.chrome().buttons_for(form.as_ref()));
		buttons.extend(self.search_buttons());
		let mut focus = Vec::new();
		if let Some(form) = &form {
			focus.extend(
				buttons
					.iter()
					.filter(|b| {
						matches!(
							b.action,
							crate::state::Command::SettingsTab(_)
						)
					})
					.cloned(),
			);
			focus.extend(
				form.buttons
					.iter()
					.filter(|b| {
						b.enabled && self.settings_action_enabled(b.action)
					})
					.cloned(),
			);
		} else {
			for button in &buttons {
				if button.widget().focusable
					&& button.enabled
					&& !focus.iter().any(|b: &Button| b.id == button.id)
				{
					focus.push(button.clone());
				}
			}
		}
		let hit = super::widgets::layout(&buttons);
		super::widgets::UiLayout {
			buttons,
			focus,
			form,
			hit,
		}
	}
	pub(super) fn buttons(&mut self) -> Vec<Button> {
		self.ui_layout().buttons
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		self.refresh_viewer();
		self.ensure_outline();
		self.normalize_tab_scroll();
		// The wheel, the scrollbar and the page all read one clamped offset.
		if let Some((_, max)) = self.panel_scroll_range()
			&& let Some(area) = self.panel_scroll_area()
		{
			area.set_max(max);
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
					markview_layout::text::TextCounts::of(
						&session
							.snapshot
							.extract_text(s, session.accepted_revision),
					),
				)
			});
		}
		let layout = self.ui_layout();
		self.interaction.controls = Some(layout.hit);
		let mut inputs = self.draw_inputs_with_form(layout.form.as_ref());
		if self.readers.session.search.open {
			inputs.extend(self.draw_search());
			inputs.extend(self.draw_search_input());
		}
		let mut out = self.draw_search_highlights();
		let mut chrome = self.chrome();
		chrome.input_draws = inputs;
		let (draws, body) = chrome.overlay_with_form(layout.form.as_ref());
		out.extend(draws);
		self.preview.set_body(body);
		if markview_platform::touch_layout() {
			use crate::layout::{Paint, Rect};
			use markview_layout::style::{ColorField, Condition};
			let (width, height, _) = self.dimensions();
			let [left, top, right, bottom] = self.insets();
			// Extend the chrome behind system bars without moving its controls.
			for (x, y, w, h, condition) in [
				(-left, -top, width + left + right, top, Condition::Toolbar),
				(
					-left,
					height,
					width + left + right,
					bottom,
					Condition::Statusbar,
				),
				(-left, 0.0, left, height, Condition::Toolbar),
				(width, 0.0, right, height, Condition::Toolbar),
			] {
				out.push(Draw::Rect(
					Rect { x, y, w, h },
					Paint::Styled(condition, ColorField::Background),
				));
			}
		}
		out
	}
	pub(super) fn tab_layout(&mut self) -> super::tab_strip::TabLayout {
		self.tab_metrics.sync(&mut self.ui, self.readers.entries());
		if self.tab_strip.phone {
			return super::tab_strip::TabLayout::new(
				crate::layout::Rect {
					x: 0.,
					y: 0.,
					w: 0.,
					h: 0.,
				},
				&[],
				0.,
			);
		}
		self.chrome().tab_bar().layout()
	}
}
