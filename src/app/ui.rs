//! Adapt application state to borrowed chrome inputs.
use super::{
	App, Button,
	chrome::{self, Chrome},
};
use crate::layout::{Draw, Scrollbar};
use crate::settings::Setting;
use crate::state::{ExportCommand, SettingsCommand};
use std::time::Instant;
impl<P: super::SendEvent> App<P> {
	/// Settings commands. Page commands go through their component messages
	/// first; the rest are host effects that outlive a page update.
	pub(super) fn settings_command(&mut self, command: SettingsCommand) {
		if self.page_command(crate::state::Command::Settings(command)) {
			return;
		}
		match command {
			SettingsCommand::Styles => {
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				self.show_styles(false);
				self.refresh_settings_resources(true);
				self.interaction.widgets.focus = None;
				self.redraw();
			}
			SettingsCommand::RetrySettingsLoad => {
				self.refresh_settings_resources(true);
				self.redraw();
			}
			SettingsCommand::StylesFolder => {
				let result = crate::stylesheet::directory()
					.ok_or_else(|| {
						anyhow::anyhow!(
							self.preferences
								.values
								.lang()
								.status_no_stylesheet_directory()
						)
					})
					.and_then(|dir| {
						std::fs::create_dir_all(&dir)?;
						markview_platform::open_external(dir)?;
						Ok(())
					});
				if let Err(e) = result {
					self.preferences.style_warning = Some(
						crate::app::preferences::StyleWarning::Override(e),
					);
				}
				self.redraw();
			}
			SettingsCommand::OpenConfig => {
				let result = self.preferences.ensure_file().and_then(|()| {
					markview_platform::open_external(
						self.preferences.path().unwrap(),
					)
				});
				if let Err(error) = result {
					self.preferences.settings_warning = Some(
						crate::settings::SettingsWarning::OpenFailed(error),
					);
				}
				self.redraw();
			}
			SettingsCommand::SystemTheme => {
				self.args.overrides.retain(|f| *f != Setting::Theme);
				self.args.theme = None;
				self.args.style = None;
				self.preferences.follow_system();
				self.apply_saved_settings();
				self.preferences.schedule_save();
			}
			_ => unreachable!("page command handled above"),
		}
	}

	pub(super) fn page_command(
		&mut self,
		action: crate::state::Command,
	) -> bool {
		use super::{settings_panel, ui_model::Message};
		use crate::state::Command;
		if self.tab_strip.phone
			&& matches!(
				action,
				Command::Settings(SettingsCommand::Narrower)
					| Command::Settings(SettingsCommand::Wider)
					| Command::Settings(SettingsCommand::TabStyle(_))
			) {
			return true;
		}
		let message = match action {
			Command::Settings(SettingsCommand::Reset) => {
				let mut values = crate::settings::ReaderSettings::default();
				if let Some(theme) = self.system_theme() {
					values.theme = theme;
				}
				Some(Message::Settings(settings_panel::Message::Reset(
					Box::new(values),
				)))
			}
			Command::Settings(SettingsCommand::StyleToggle(index))
			| Command::Settings(SettingsCommand::StyleUp(index))
			| Command::Settings(SettingsCommand::StyleDown(index))
			| Command::Export(ExportCommand::ExportStyleToggle(index))
			| Command::Export(ExportCommand::ExportStyleUp(index))
			| Command::Export(ExportCommand::ExportStyleDown(index)) => {
				let export = matches!(
					action,
					Command::Export(ExportCommand::ExportStyleToggle(_))
						| Command::Export(ExportCommand::ExportStyleUp(_))
						| Command::Export(ExportCommand::ExportStyleDown(_))
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
					Command::Settings(SettingsCommand::StyleToggle(_))
					| Command::Export(ExportCommand::ExportStyleToggle(_)) => {
						super::style_edit::Operation::Toggle
					}
					Command::Settings(SettingsCommand::StyleUp(_))
					| Command::Export(ExportCommand::ExportStyleUp(_)) => {
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
			Command::Settings(SettingsCommand::FontFamily(role, selection)) => {
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
						self.select_dropdown()
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
		let hover_hint = self.interaction.pointer.hover_image().or_else(|| {
			self.interaction
				.pointer
				.hover()
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

		let style_entries: &[crate::stylesheet::Entry] = if !show_snapshot {
			&[]
		} else if self.interaction.export_styles_open() {
			&self.settings_resources.export_entries
		} else {
			&self.preferences.style_entries
		};
		let resource_load =
			resource_kind.map(|kind| self.settings_resources.load(kind));
		let frame = chrome::FrameView::new(
			frame,
			(width, height),
			scrollbar,
			remote_notice,
			self.renderer.as_ref().map(|renderer| renderer.backend),
		);
		let session = chrome::SessionView::new(
			&self.readers.session,
			self.readers.entries(),
			self.readers.active(),
			&self.tab_strip,
			&self.tab_metrics.widths,
			self.preview.open(),
			Some(&self.folder),
		);
		let settings = chrome::SettingsView::new(
			view.settings,
			self.preferences.restart_pending(),
			&self.fonts_config,
			view.export,
			style_entries,
			self.interaction.panels.styles_scroll().offset(),
			resource_load,
			fonts,
			self.export.watching(),
		);
		let status = chrome::StatusView::new(&self.status, warning, hover_hint);
		Chrome {
			frame,
			session,
			settings,
			status,
			interaction: super::widgets::Drawn::new(
				&self.interaction,
				self.controls.as_ref(),
			),
			ui: &mut self.ui,
			input_draws: Vec::new(),
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
				self.tabs_reveal(),
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
				self.interaction.panels.styles_scroll().offset(),
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

	/// Switches panel pages, retargeting the tab strip's drawer animation in
	/// the owner that holds it.
	fn switch_panel(
		&mut self,
		change: impl FnOnce(&mut crate::state::InteractionState),
	) {
		let was_tabs = self.interaction.panel == crate::state::PanelPage::Tabs;
		change(&mut self.interaction);
		self.tab_strip.animate(
			self.interaction.panel == crate::state::PanelPage::Tabs,
			was_tabs,
		);
	}

	/// Changes pages without losing the parent form's scroll position.
	pub(super) fn show_panel(&mut self, page: crate::state::PanelPage) {
		self.switch_panel(|interaction| interaction.show_panel(page));
	}

	/// Opens or closes the settings page. The page clears its own offset and
	/// preview when the host opens it.
	pub(super) fn toggle_settings(&mut self) {
		self.switch_panel(crate::state::InteractionState::toggle_settings);
		if self.interaction.panel_open() {
			self.preferences.reset_page();
		}
	}

	pub(super) fn toggle_export(&mut self) {
		self.switch_panel(crate::state::InteractionState::toggle_export);
	}

	pub(super) fn show_styles(&mut self, export: bool) {
		self.switch_panel(|interaction| interaction.show_styles(export));
	}

	/// The tabs drawer's slide reveal; the tab strip owns the animation.
	pub(super) fn tabs_reveal(&self) -> f32 {
		self.tab_strip
			.reveal(self.interaction.panel == crate::state::PanelPage::Tabs)
	}

	/// Advances the three drawer animations. Returns whether any was in
	/// flight before this frame settled them.
	pub(super) fn advance_drawers(&mut self, now: Instant) -> bool {
		let active = self.drawer_deadline(now).is_some();
		self.tab_strip.advance_animation(now);
		self.interaction.overlays.outline.advance_animation(now);
		self.folder.advance_animation(now);
		active
	}

	/// The earliest frame one of the drawer animations needs.
	pub(super) fn drawer_deadline(&self, now: Instant) -> Option<Instant> {
		(self.tab_strip.animating()
			|| self.interaction.overlays.outline.animating()
			|| self.folder.animating())
		.then_some(now + std::time::Duration::from_millis(16))
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
			Some(self.interaction.panels.export_scroll_mut())
		} else if self.interaction.fonts_open() {
			Some(self.preferences.fonts.scroll_area())
		} else if self.interaction.styles_open()
			|| self.interaction.export_styles_open()
		{
			Some(self.interaction.panels.styles_scroll_mut())
		} else if self.interaction.panel_open() {
			Some(self.preferences.settings_scroll_mut())
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
		if self.interaction.overlays.dropdown_open() {
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
		if self.interaction.overlays.dropdown_open() {
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
		let (x, y) = self.interaction.pointer.cursor();
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
		self.interaction.panels.set_grab(Some(grab));
		true
	}
	pub(super) fn drag_panel(&mut self) {
		if let Some(grab) = self.interaction.panels.grab()
			&& let Some(bar) = self.panel_bar()
		{
			let (x, y) = self.interaction.pointer.cursor();
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
		self.preferences.advance_scroll(now);
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
			|| self.preferences.scroll_animating()
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
			.chain(
				[self.preferences.scroll_deadline(now)]
					.into_iter()
					.flatten(),
			)
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
							crate::state::Command::Chrome(
								crate::state::ChromeCommand::SettingsTab(_)
							)
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
		self.controls = Some(layout.hit);
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
