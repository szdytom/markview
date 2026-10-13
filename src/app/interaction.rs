//! Commands, selection gestures and clipboard actions.
use crate::cli::Mode;
use crate::settings::Setting;
use crate::state::{Command, Modal, PanelPage, ScrollbarAxis, ScrollbarDrag};
use markview_layout::text::TextPosition;
use std::time::{Duration, Instant};

use super::{App, Event, TOP};

use crate::state::Selection;
fn sanitize_filename(title: &str, lang: crate::lang::Lang) -> String {
	let name: String = title
		.chars()
		.map(|c| {
			if c.is_control()
				|| matches!(
					c,
					'/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
				) {
				' '
			} else {
				c
			}
		})
		.collect();
	let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
	let name: String = name.chars().take(48).collect();
	if name.trim().is_empty() {
		lang.status_pasted().into()
	} else {
		name
	}
}

impl<P: super::SendEvent> App<P> {
	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep exhaustive command dispatch with shared input and gesture cleanup; every host command must observe the same pre-dispatch ownership rules."
		)
	)]
	pub(super) fn action(&mut self, action: Command) {
		if !self.settings_action_enabled(action) {
			return;
		}
		if let Command::FocusInput(id) = action {
			self.interaction.widgets.focus =
				Some(Command::FocusInput(id).widget_id());
			self.sync_input();
			self.redraw();
			return;
		}
		if matches!(
			action,
			Command::Settings
				| Command::SettingsTab(_)
				| Command::Styles
				| Command::Export
				| Command::ExportStyles
				| Command::Tabs
		) {
			self.close_search();
		}
		// Search navigation keeps the IME open while moving between results.
		if !matches!(action, Command::SearchNext | Command::SearchPrevious) {
			self.blur_input();
			if self.focused_input().is_some() {
				self.interaction.widgets.focus = None;
			}
		}

		self.cancel_gestures();
		// A command replaces what a glide was moving, so the viewport's
		// gesture is abandoned with the reader's own motion.
		self.abandon_dm();
		// Export-panel changes own their settings and never reflow the reader.
		if self.page_command(action) {
			return;
		}
		match action {
			Command::OpenUrl => {
				let url = self.url_input.text().trim().to_owned();
				if !url.is_empty() {
					self.open_web_page(url);
					self.url_input.set_text(&mut self.ui, "");
				}
			}
			Command::SearchOpen => {
				self.open_new_search();
			}
			Command::SearchClose => {
				self.close_search();
			}
			Command::SearchNext | Command::SearchPrevious => {
				self.navigate_search(action == Command::SearchPrevious);
				self.interaction.widgets.focus = Some(
					Command::FocusInput(crate::state::TextField::Search)
						.widget_id(),
				);
				self.sync_input();
			}
			Command::SearchCase | Command::SearchWord => {
				let options = &mut self.readers.session.search.options;
				if action == Command::SearchCase {
					options.case_sensitive = !options.case_sensitive;
				} else {
					options.whole_word = !options.whole_word;
				}
				self.search_changed();
			}
			Command::FocusInput(_) => unreachable!("input focus handled above"),
			Command::OpenProject => {
				self.launch(env!("CARGO_PKG_REPOSITORY"));
			}
			Command::CopyDiagnostics => {
				let text = crate::diagnostics::report(
					self.renderer.as_ref().map(|renderer| renderer.backend),
				);
				match self.clipboard.write(text) {
					Ok(()) => self.notify(
						self.preferences
							.values
							.lang()
							.status_copied_diagnostics(),
						false,
						3,
					),
					Err(error) => self.notify(
						&self
							.preferences
							.values
							.lang()
							.status_diagnostics_failed(format!("{error}")),
						true,
						4,
					),
				}
				self.redraw();
			}
			Command::CopyPath => {
				let Some(path) = self.readers.session.path.clone() else {
					return;
				};
				let lang = self.preferences.values.lang();
				let text = path.to_string_lossy().into_owned();
				match self.clipboard.write(text) {
					Ok(()) => self.notify(lang.status_copied_path(), false, 3),
					Err(error) => self.notify(
						&lang.status_copy_failed(error.to_string()),
						true,
						4,
					),
				}
			}
			Command::Tabs => {
				if !self.tab_strip.phone {
					return;
				}
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				let open = self.interaction.panel != PanelPage::Tabs;
				self.interaction.show_panel(if open {
					PanelPage::Tabs
				} else {
					PanelPage::Closed
				});
				if open && self.readers.session.path.is_some() {
					let (width, height, _) = self.dimensions();
					let list = super::chrome::tab_drawer::list(
						width,
						height,
						self.readers.entries().len(),
						self.tab_strip.drawer_scroll,
					);
					let row = list.row_rect(self.readers.active());
					self.tab_strip.drawer_scroll = (list.scroll
						+ (row.y - list.viewport.y).min(0.)
						+ (row.y + row.h - list.viewport.y - list.viewport.h)
							.max(0.))
					.clamp(0., list.max_scroll());
				}
				self.refresh_hover();
				self.redraw();
			}
			Command::SelectTab(index) => {
				if self.interaction.panel == PanelPage::Tabs {
					self.interaction.show_panel(PanelPage::Closed);
				}
				self.select_tab(index);
			}
			Command::ModalDismiss => {
				self.interaction.modal = None;
				self.interaction.widgets.focus = None;
				self.refresh_hover();
				self.redraw();
			}
			Command::ModalOpenFolder => {
				if let Some(Modal::OpenLocal { dir, .. }) =
					self.interaction.modal.clone()
				{
					self.interaction.modal = None;
					self.interaction.widgets.focus = None;
					self.launch(&dir.display().to_string());
				}
			}
			Command::ModalConfirm => {
				if let Some(modal) = self.interaction.modal.take() {
					self.interaction.widgets.focus = None;
					match modal {
						Modal::OpenLocal { path, .. } => {
							self.launch(&path.display().to_string())
						}
						Modal::Permission { resource, resume } => {
							self.confirm_permission(resource, resume)
						}
					}
				}
			}
			Command::RemoteDismiss => {
				// Both answers belong to this tab and this content revision.
				self.readers.session.remote_notice_dismissed = true;
				self.redraw();
			}
			Command::RemoteLoadAll => {
				self.readers.session.remote_notice_dismissed = true;
				self.readers.session.load_all_images = true;
				self.request(false);
			}
			Command::Outline => {
				self.toggle_outline();
			}
			Command::OutlineToggle(index) => {
				self.toggle_outline_entry(index);
			}
			Command::OutlineExpandAll | Command::OutlineCollapseAll => {
				self.set_outline_collapsed(
					action == Command::OutlineCollapseAll,
				);
			}
			Command::OutlineGoto(index) => {
				self.goto_outline(index);
			}
			Command::CloseTab(index) => {
				self.close_tab(index);
			}
			Command::Styles => {
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				self.interaction.show_styles(false);
				self.refresh_settings_resources(true);
				self.interaction.widgets.focus = None;
				self.redraw();
			}
			Command::ExportStyles => {
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				self.interaction.show_styles(true);
				self.refresh_settings_resources(true);
				self.interaction.widgets.focus = None;
				self.redraw();
			}
			Command::Export => {
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				if !self.interaction.export_open()
					&& self.readers.session.path.is_none()
				{
					self.notify(
						self.preferences.values.lang().status_open_first(),
						true,
						4,
					);
					return;
				}
				self.interaction.toggle_export();
				self.refresh_hover();
				self.redraw();
			}
			Command::ExportRun => {
				self.start_export(false);
			}
			Command::ExportAndWatch => {
				self.start_export(true);
			}
			// Applied by `page_command` before this match.
			Command::ExportFormat(_)
			| Command::ExportSize(_)
			| Command::ExportIndent(_)
			| Command::ExportPaper(_)
			| Command::ExportOrientation(_)
			| Command::ExportMargin(_)
			| Command::ExportScale(_) => {}
			Command::Fonts(command) => {
				// Switching the page's view leaves the chooser rows the open
				// list anchors to, so the list goes with them.
				if matches!(
					command,
					super::font_panel::Command::StatusFilter(_)
						| super::font_panel::Command::Choosers
				) {
					self.interaction.dropdown = None;
				}
				self.font_message(super::font_panel::Message::Command {
					command,
					offline: self.args.offline,
				});
				self.refresh_settings_resources(false);
				self.redraw();
			}
			Command::RetrySettingsLoad => {
				self.refresh_settings_resources(true);
				self.redraw();
			}
			Command::SettingsTab(tab) => {
				self.interaction.show_panel(PanelPage::Settings(tab));
				self.refresh_settings_resources(true);
				self.interaction.widgets.focus = None;
				self.redraw();
			}
			Command::StylesFolder => {
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
			Command::StyleToggle(_)
			| Command::StyleUp(_)
			| Command::StyleDown(_)
			| Command::ExportStyleToggle(_)
			| Command::ExportStyleUp(_)
			| Command::ExportStyleDown(_) => unreachable!("page command handled above"),
			Command::OpenConfig => {
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
			Command::SystemTheme => {
				self.args.overrides.retain(|f| *f != Setting::Theme);
				self.args.theme = None;
				self.args.style = None;
				self.preferences.follow_system();
				self.apply_saved_settings();
				self.preferences.schedule_save();
			}
			Command::Settings => {
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				self.interaction.toggle_settings();
				if self.interaction.panel_open() {
					self.dispatch_ui(super::ui_model::Message::Settings(
						super::settings_panel::Message::ResetPreview,
					));
				}
				self.refresh_hover();
				self.redraw();
			}
			Command::NewPage => {
				self.new_page();
			}
			Command::Open => {
				if self.interaction.panel == PanelPage::Tabs {
					self.interaction.show_panel(PanelPage::Closed);
					self.redraw();
				}
				if self.dialog_open {
					return;
				}
				self.dialog_open = true;
				let proxy = self.proxy.clone();
				markview_platform::pick_document(move |path| {
					proxy.send(Event::Open(path));
				});
			}
			Command::RevealFolder => {
				let dir = self
					.readers
					.session
					.path
					.as_deref()
					.and_then(std::path::Path::parent)
					.filter(|dir| dir.is_dir());
				match dir {
					Some(dir) => {
						if let Err(error) =
							markview_platform::open_external(dir)
						{
							let text = self
								.preferences
								.values
								.lang()
								.status_open_failed(
									dir.display().to_string(),
									error.to_string(),
								);
							self.notify(&text, true, 4);
						}
					}
					None => {
						self.notify(
							self.preferences.values.lang().status_open_first(),
							true,
							4,
						);
					}
				}
			}
			Command::Smaller
			| Command::Larger
			| Command::Narrower
			| Command::Wider
			| Command::Align
			| Command::Hyphens
			| Command::CodeWrap
			| Command::Indent(_)
			| Command::CjkType(_)
			| Command::Language(_)
			| Command::FontFamily(_, _)
			| Command::WindowLayout(_)
			| Command::TabStyle(_)
			| Command::SingleInstance
			| Command::RestoreSession
			| Command::ScrollSpeed(_)
			| Command::Reset
			| Command::SettingsPreview => unreachable!("page command handled above"),
			Command::ToggleDropdown(id, highlight) => {
				match self.interaction.dropdown {
					Some(open) if open.id == id => self.close_dropdown(),
					_ => {
						self.interaction.dropdown =
							Some(crate::state::Dropdown::new(id, highlight));
					}
				}
				self.redraw();
			}
		}
	}

	pub(super) fn setting_changed(&mut self, field: Option<Setting>) {
		self.services
			.handle
			.configure_http(&self.preferences.values);
		self.args
			.overrides
			.retain(|f| field.is_some_and(|changed| changed != *f));
		if field.is_none() || field == Some(Setting::Theme) {
			self.args.theme = None;
			self.args.style = None;
			self.reload_styles();
		}
		if self.args.mode == Mode::Window {
			self.preferences.changed(field);
		}
	}
	pub(super) fn text_at_cursor(&self) -> Option<TextPosition> {
		self.document_interaction()
			.position(markview_selection::Point::new(
				self.interaction.cursor.0,
				self.interaction.cursor.1,
			))
	}

	pub(super) fn text_under_cursor(&self) -> bool {
		self.document_interaction().contains_text(
			markview_selection::Point::new(
				self.interaction.cursor.0,
				self.interaction.cursor.1,
			),
		)
	}

	/// Starts dragging the scrollbar under the pointer. A press on the thumb
	/// keeps it under the pointer; a press on the empty track jumps the thumb
	/// there first, so the same gesture continues as a drag.
	pub(super) fn begin_scrollbar_drag(&mut self) -> bool {
		let (x, y) = self.interaction.cursor;
		if let Some(bar) = self.document_scrollbar()
			&& bar.hit(x, y)
		{
			let grab = if bar.on_thumb(x, y) {
				// A grabbed thumb follows the pointer directly.
				self.readers.session.cancel_scroll_animation();
				bar.grab(x, y)
			} else {
				// A track click eases the thumb to the pointer; a drag that
				// follows cancels the animation and takes the thumb over.
				let target = bar.scroll_for(x, y, 0.0);
				self.readers
					.session
					.animate_scroll_to(target, Instant::now());
				0.0
			};
			self.interaction.reset_clicks();
			self.interaction.scrollbar = Some(ScrollbarDrag {
				target: ScrollbarAxis::Document,
				grab,
			});
			self.refresh_hover();
			return true;
		}
		if let Some((block, overflow, bar)) = self.overflow_scrollbar_at(x, y) {
			let grab = if bar.on_thumb(x, y) {
				bar.grab(x, y)
			} else {
				let offset = bar.scroll_for(x, y, 0.0);
				self.readers
					.session
					.horizontal
					.insert((block, overflow), offset);
				0.0
			};
			self.interaction.reset_clicks();
			self.interaction.scrollbar = Some(ScrollbarDrag {
				target: ScrollbarAxis::Overflow { block, overflow },
				grab,
			});
			self.refresh_hover();
			return true;
		}
		false
	}

	/// Applies an in-flight scrollbar drag to the pointer's new position.
	pub(super) fn drag_scrollbar(&mut self) {
		let Some(drag) = self.interaction.scrollbar else {
			return;
		};
		let (x, y) = self.interaction.cursor;
		match drag.target {
			ScrollbarAxis::Document => {
				self.readers.session.saved_reading = None;
				if let Some(bar) = self.document_scrollbar() {
					// The thumb follows the pointer; nothing eases under a drag.
					self.readers.session.cancel_scroll_animation();
					self.readers.session.scrolling.offset =
						bar.scroll_for(x, y, drag.grab);
					self.redraw();
				}
			}
			ScrollbarAxis::Overflow { block, overflow } => {
				if let Some(bar) = self.overflow_scrollbar(block, overflow) {
					self.readers.session.horizontal.insert(
						(block, overflow),
						bar.scroll_for(x, y, drag.grab),
					);
					self.redraw();
				}
			}
		}
	}

	pub(super) fn update_drag(&mut self) {
		let position = if self.interaction.pointer_down.is_some() {
			self.text_at_cursor()
		} else {
			None
		};
		self.interaction
			.move_selection(position, &self.readers.session.snapshot);
		if self.interaction.pointer_down.is_some() && self.interaction.dragged {
			let (_, height, _) = self.dimensions();
			let can_scroll = markview_selection::selection_scroll(
				self.interaction.cursor.1,
				TOP,
				height - self.bottom(),
				self.readers.session.scrolling.offset,
				(self.readers.session.snapshot.height - self.viewport())
					.max(0.0),
			) != 0.0;
			self.interaction.drag_at =
				can_scroll.then(|| Instant::now() + Duration::from_millis(16));
			self.redraw();
		}
	}

	/// The reading text the current selection copies, or `None` when there is
	/// nothing to copy: no selection, an empty one, or one whose revision the
	/// snapshot has outlived.
	pub(super) fn selected_text(&self) -> Option<String> {
		let selection = self.interaction.selection.filter(|s| !s.is_empty())?;
		let text = self
			.readers
			.session
			.snapshot
			.extract_text(selection, self.readers.session.accepted_revision);
		(!text.is_empty()).then_some(text)
	}

	pub(super) fn copy_selection(&mut self) {
		let Some(text) = self.selected_text() else {
			return;
		};
		match self.clipboard.write(text) {
			Ok(()) => {
				self.status = self
					.preferences
					.values
					.lang()
					.status_copied_selection()
					.into();
				self.error = false;
			}
			Err(e) => {
				self.status = self
					.preferences
					.values
					.lang()
					.status_copy_failed(e.to_string());
				self.error = true;
			}
		}
		self.redraw();
	}
	pub(super) fn paste_markdown(&mut self) {
		let text = match self.clipboard.read() {
			Ok(text) => text,
			Err(error) => {
				self.status = self
					.preferences
					.values
					.lang()
					.status_clipboard_unreadable(error.to_string());
				self.error = true;
				self.status_until =
					Some(Instant::now() + Duration::from_secs(3));
				self.redraw();
				return;
			}
		};
		if crate::web_page::validate_url(text.trim()).is_ok() {
			self.open_web_page(text.trim().to_owned());
			return;
		}
		self.open_text(&text);
	}

	pub(super) fn temporary_path(&mut self, title: &str) -> std::path::PathBuf {
		self.paste_serial = self.paste_serial.wrapping_add(1);
		let filename = format!(
			"{}-{}.md",
			sanitize_filename(title, self.preferences.values.lang()),
			self.paste_serial
		);
		self.paste_dir.path().join(filename)
	}

	pub(super) fn open_text(
		&mut self,
		text: &str,
	) -> Option<std::path::PathBuf> {
		if !crate::paste::looks_like_markdown(text) {
			return None;
		}
		let title =
			crate::paste::title_for(text, self.preferences.values.lang());
		let path = self.temporary_path(&title);
		if let Err(error) = std::fs::write(&path, text) {
			self.status = self
				.preferences
				.values
				.lang()
				.status_paste_failed(error.to_string());
			self.error = true;
			self.status_until = Some(Instant::now() + Duration::from_secs(3));
			self.redraw();
			return None;
		}
		self.open_document(path, crate::security::Security::default());
		self.readers.session.path.clone()
	}

	pub(super) fn flush_settings(&mut self) {
		if self.preferences.flush() && self.args.mode == Mode::Window {
			self.apply_saved_settings();
		}
	}
	pub(super) fn apply_saved_settings(&mut self) {
		let previous = self.preferences.values.clone();
		let options = self.options();
		self.preferences.values = self.preferences.stored_settings();
		self.preferences.export = self.preferences.stored_export();
		self.preferences.values.stylesheet = previous.stylesheet.clone();
		if self.preferences.theme_preference().is_none() {
			self.preferences.values.theme =
				self.system_theme().unwrap_or_default();
		}
		for field in &self.args.overrides {
			self.preferences.values.copy_field(&previous, *field);
		}
		self.services
			.handle
			.configure_http(&self.preferences.values);
		self.sync_session_settings();
		self.reload_styles();
		if self.options() != options
			&& self.readers.session.requested_options.as_ref()
				!= Some(&self.options())
		{
			self.request(false);
		}
		if self.preferences.values != previous {
			self.redraw();
		}
	}
}

#[cfg(test)]
mod tests;
