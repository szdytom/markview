//! Commands, selection gestures and clipboard actions.
use crate::cli::Mode;
use crate::settings::Setting;
use crate::state::{
	ChromeCommand, ExportCommand, SearchCommand, SettingsCommand,
};
use crate::state::{Command, PanelPage, ScrollbarAxis, ScrollbarDrag};
use markview_layout::text::TextPosition;
use std::time::{Duration, Instant};

use super::modal::{Message, Modal};
use super::{App, TOP};

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
	/// Routes one command to the domain that owns its state.
	///
	/// The pre-dispatch rules run once, before any domain sees the command:
	/// an unavailable settings catalogue rejects it, input focus is a special
	/// case, and every other command leaves search, the text input and any
	/// pointer gesture to its owner.
	pub(super) fn action(&mut self, command: Command) {
		if !self.settings_action_enabled(command) {
			return;
		}
		if let Command::Chrome(ChromeCommand::FocusInput(field)) = command {
			self.focus_input(field);
			return;
		}
		self.prepare_command(command);
		match command {
			Command::Document(command) => self.document_command(command),
			Command::Settings(command) => self.settings_command(command),
			Command::Export(command) => self.export_command(command),
			Command::Chrome(command) => self.chrome_command(command),
			Command::Search(command) => self.search_command(command),
			Command::Fonts(command) => self.fonts_command(command),
		}
	}

	/// The ownership rules every command observes before its domain sees it.
	fn prepare_command(&mut self, command: Command) {
		if matches!(
			command,
			Command::Chrome(ChromeCommand::Settings)
				| Command::Chrome(ChromeCommand::SettingsTab(_))
				| Command::Settings(SettingsCommand::Styles)
				| Command::Export(ExportCommand::Export)
				| Command::Export(ExportCommand::ExportStyles)
				| Command::Chrome(ChromeCommand::Tabs)
				| Command::Chrome(ChromeCommand::SidebarTab(_))
		) {
			self.close_search();
		}
		// Search navigation keeps the IME open while moving between results.
		if !matches!(
			command,
			Command::Search(SearchCommand::SearchNext)
				| Command::Search(SearchCommand::SearchPrevious)
		) {
			self.blur_input();
			if self.focused_input().is_some() {
				self.interaction.widgets.focus = None;
			}
		}
		self.cancel_gestures();
		// A command replaces what a glide was moving, so the viewport's
		// gesture is abandoned with the reader's own motion.
		self.abandon_dm();
	}

	fn focus_input(&mut self, field: crate::state::TextField) {
		self.interaction.widgets.focus =
			Some(Command::Chrome(ChromeCommand::FocusInput(field)).widget_id());
		self.sync_input();
		self.redraw();
	}

	/// Chrome commands: tabs, the settings page, folders, the dropdown and the
	/// confirmation.
	fn chrome_command(&mut self, command: ChromeCommand) {
		match command {
			ChromeCommand::FocusInput(_) => {
				unreachable!("input focus handled before routing")
			}
			ChromeCommand::OpenProject => {
				self.launch(env!("CARGO_PKG_REPOSITORY"));
			}
			ChromeCommand::CopyDiagnostics => self.copy_diagnostics(),
			ChromeCommand::Tabs
			| ChromeCommand::SidebarTab(_)
			| ChromeCommand::SelectTab(_)
			| ChromeCommand::CloseTab(_) => self.chrome_tab_command(command),
			ChromeCommand::Settings | ChromeCommand::SettingsTab(_) => {
				self.chrome_panel_command(command)
			}
			ChromeCommand::OpenFolder
			| ChromeCommand::Folder
			| ChromeCommand::FolderRoot
			| ChromeCommand::FolderEntry(_)
			| ChromeCommand::FolderCollapseAll
			| ChromeCommand::FolderRefresh
			| ChromeCommand::RevealFolder => self.chrome_folder_command(command),
			ChromeCommand::ToggleDropdown(id, highlight) => {
				self.chrome_dropdown_command(id, highlight)
			}
			ChromeCommand::ModalDismiss
			| ChromeCommand::ModalOpenFolder
			| ChromeCommand::ModalConfirm => self.chrome_modal_command(command),
		}
	}

	fn copy_diagnostics(&mut self) {
		let text = crate::diagnostics::report(
			self.renderer.as_ref().map(|renderer| renderer.backend),
		);
		match self.clipboard.write(text) {
			Ok(()) => self.notify(
				self.preferences.values.lang().status_copied_diagnostics(),
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

	fn chrome_tab_command(&mut self, command: ChromeCommand) {
		match command {
			ChromeCommand::Tabs => self.toggle_phone_sidebar(),
			ChromeCommand::SidebarTab(tab) => self.switch_sidebar_tab(tab),
			ChromeCommand::SelectTab(index) => {
				if self.interaction.panel == PanelPage::Tabs {
					self.show_panel(PanelPage::Closed);
				}
				self.select_tab(index);
			}
			ChromeCommand::CloseTab(index) => self.close_tab(index),
			_ => unreachable!("not a tab command"),
		}
	}

	fn chrome_panel_command(&mut self, command: ChromeCommand) {
		match command {
			ChromeCommand::Settings => {
				self.readers.session.cancel_scroll_animation();
				self.tab_strip.cancel_drag();
				self.toggle_settings();
				if self.interaction.panel_open() {
					self.dispatch_ui(super::ui_model::Message::Settings(
						super::settings_panel::Message::ResetPreview,
					));
				}
				self.refresh_hover();
				self.redraw();
			}
			ChromeCommand::SettingsTab(tab) => {
				self.show_panel(PanelPage::Settings(tab));
				self.refresh_settings_resources(true);
				self.interaction.widgets.focus = None;
				self.redraw();
			}
			_ => unreachable!("not a settings-panel command"),
		}
	}

	fn chrome_folder_command(&mut self, command: ChromeCommand) {
		match command {
			ChromeCommand::OpenFolder => self.pick_folder(),
			ChromeCommand::Folder => self.toggle_folder(),
			ChromeCommand::FolderRoot => {
				if let Some(tree) = &mut self.folder.tree {
					tree.toggle_root();
					self.scroll_folder(0.);
					self.redraw();
				}
			}
			ChromeCommand::FolderEntry(index) => {
				self.activate_folder_entry(index)
			}
			ChromeCommand::FolderCollapseAll => {
				if let Some(tree) = &mut self.folder.tree {
					tree.collapse_all();
					self.folder.grab = None;
					self.scroll_folder(0.);
					self.refresh_hover();
					self.redraw();
				}
			}
			ChromeCommand::FolderRefresh => {
				if let Some(tree) = &self.folder.tree {
					self.open_folder(tree.root.clone(), false);
				}
			}
			ChromeCommand::RevealFolder => {
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
			_ => unreachable!("not a folder command"),
		}
	}

	fn chrome_dropdown_command(
		&mut self,
		id: crate::state::DropdownId,
		highlight: usize,
	) {
		let same = self
			.interaction
			.overlays
			.dropdown
			.current()
			.is_some_and(|open| open.id() == id);
		if same {
			self.close_dropdown();
		} else {
			// The list owns the page's scroll from here: freeze its anchor
			// instead of easing it out from under the list.
			self.cancel_panel_scroll();
			self.interaction
				.overlays
				.dropdown
				.send(super::dropdown::Message::Open { id, highlight });
		}
		self.redraw();
	}

	fn chrome_modal_command(&mut self, command: ChromeCommand) {
		match command {
			ChromeCommand::ModalDismiss => {
				self.interaction.overlays.modal.send(Message::Dismiss);
				self.interaction.widgets.focus = None;
				self.refresh_hover();
				self.redraw();
			}
			ChromeCommand::ModalOpenFolder => {
				if let Some(Modal::OpenLocal { dir, .. }) =
					self.interaction.overlays.modal().cloned()
				{
					self.interaction.overlays.modal.send(Message::Dismiss);
					self.interaction.widgets.focus = None;
					self.launch(&dir.display().to_string());
				}
			}
			ChromeCommand::ModalConfirm => {
				if let Some(modal) = self.interaction.overlays.modal.take() {
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
			_ => unreachable!("not a modal command"),
		}
	}

	fn fonts_command(&mut self, command: super::font_panel::Command) {
		// Switching the page's view leaves the chooser rows the open list
		// anchors to, so the list goes with them.
		if matches!(
			command,
			super::font_panel::Command::StatusFilter(_)
				| super::font_panel::Command::Choosers
		) {
			self.interaction
				.overlays
				.dropdown
				.send(super::dropdown::Message::Close);
		}
		self.font_message(super::font_panel::Message::Command {
			command,
			offline: self.args.offline,
		});
		self.refresh_settings_resources(false);
		self.redraw();
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
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			))
	}

	pub(super) fn text_under_cursor(&self) -> bool {
		self.document_interaction().contains_text(
			markview_selection::Point::new(
				self.interaction.pointer.cursor().0,
				self.interaction.pointer.cursor().1,
			),
		)
	}

	/// Starts dragging the scrollbar under the pointer. A press on the thumb
	/// keeps it under the pointer; a press on the empty track jumps the thumb
	/// there first, so the same gesture continues as a drag.
	pub(super) fn begin_scrollbar_drag(&mut self) -> bool {
		let (x, y) = self.interaction.pointer.cursor();
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
			self.interaction.pointer.begin_scrollbar(ScrollbarDrag {
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
			self.interaction.pointer.begin_scrollbar(ScrollbarDrag {
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
		let Some(drag) = self.interaction.pointer.scrollbar() else {
			return;
		};
		let (x, y) = self.interaction.pointer.cursor();
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
		let position = if self.interaction.pointer.drag().is_some() {
			self.text_at_cursor()
		} else {
			None
		};
		self.interaction
			.move_selection(position, &self.readers.session.snapshot);
		if self.interaction.pointer.drag().is_some()
			&& self.interaction.pointer.dragged()
		{
			let (_, height, _) = self.dimensions();
			let can_scroll = markview_selection::selection_scroll(
				self.interaction.pointer.cursor().1,
				TOP,
				height - self.bottom(),
				self.readers.session.scrolling.offset,
				(self.readers.session.snapshot.height - self.viewport())
					.max(0.0),
			) != 0.0;
			self.interaction.pointer.set_auto_scroll_at(
				can_scroll.then(|| Instant::now() + Duration::from_millis(16)),
			);
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
				self.status.set_message(
					self.preferences.values.lang().status_copied_selection(),
				);
				self.status.set_error(false);
			}
			Err(e) => {
				self.status.set_message(
					self.preferences
						.values
						.lang()
						.status_copy_failed(e.to_string()),
				);
				self.status.set_error(true);
			}
		}
		self.redraw();
	}
	pub(super) fn paste_markdown(&mut self) {
		let text = match self.clipboard.read() {
			Ok(text) => text,
			Err(error) => {
				self.status.set(
					self.preferences
						.values
						.lang()
						.status_clipboard_unreadable(error.to_string()),
					true,
					3,
				);
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
			self.status.set(
				self.preferences
					.values
					.lang()
					.status_paste_failed(error.to_string()),
				true,
				3,
			);
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
