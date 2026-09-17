//! Commands, selection gestures and clipboard actions.
use crate::cli::Mode;
use crate::settings::{ReaderSettings, Setting};
use crate::state::{Command, Modal, ScrollbarAxis, ScrollbarDrag};
use markview_core::text::TextPosition;
use std::time::{Duration, Instant};

use super::{App, BOTTOM, Event, TOP, system_theme};

fn sanitize_filename(title: &str) -> String {
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
		"Pasted Markdown".into()
	} else {
		name
	}
}

impl App {
	pub(super) fn action(&mut self, action: Command) {
		match action {
			Command::SelectTab(index) => {
				self.select_tab(index);
				return;
			}
			Command::ModalDismiss => {
				self.interaction.modal = None;
				self.interaction.focus = None;
				self.refresh_hover();
				self.redraw();
				return;
			}
			Command::ModalOpenFolder => {
				if let Some(Modal::OpenLocal { dir, .. }) =
					self.interaction.modal.clone()
				{
					self.interaction.modal = None;
					self.interaction.focus = None;
					self.launch(&dir.display().to_string());
				}
				return;
			}
			Command::ModalConfirm => {
				if let Some(Modal::OpenLocal { path, .. }) =
					self.interaction.modal.clone()
				{
					self.interaction.modal = None;
					self.interaction.focus = None;
					self.launch(&path.display().to_string());
				}
				return;
			}
			Command::RemoteDismiss => {
				// Both answers belong to this tab and this content revision.
				self.readers.session.remote_notice_dismissed = true;
				self.redraw();
				return;
			}
			Command::RemoteLoadAll => {
				self.readers.session.remote_notice_dismissed = true;
				self.readers.session.load_all_images = true;
				self.request(false);
				return;
			}
			Command::CloseTab(index) => {
				self.close_tab(index);
				return;
			}
			Command::Styles => {
				self.tab_strip.cancel_drag();
				self.interaction.panel_open = true;
				self.interaction.styles_open = !self.interaction.styles_open;
				self.preferences.style_entries = crate::stylesheet::catalog(
					crate::stylesheet::directory().as_deref(),
					self.preferences.values.style.as_deref(),
				);
				self.preferences.style_page = 0;
				self.interaction.focus = None;
				self.redraw();
				return;
			}
			Command::StylesFolder => {
				let result = crate::stylesheet::directory()
					.ok_or_else(|| anyhow::anyhow!("No stylesheet directory"))
					.and_then(|dir| {
						std::fs::create_dir_all(&dir)?;
						open::that_detached(dir)?;
						Ok(())
					});
				if let Err(e) = result {
					self.preferences.style_warning = Some(format!("{e:#}"));
				}
				self.redraw();
				return;
			}
			Command::StylePrev => {
				self.preferences.style_page =
					self.preferences.style_page.saturating_sub(1);
				self.redraw();
				return;
			}
			Command::StyleNext => {
				self.preferences.style_page += 1;
				self.redraw();
				return;
			}
			Command::StyleToggle(index)
			| Command::StyleUp(index)
			| Command::StyleDown(index) => {
				let Some(entry) = self.preferences.style_entries.get(index)
				else {
					return;
				};
				let mut ids =
					self.preferences.values.style.clone().unwrap_or_default();
				let position = ids.iter().position(|id| id == &entry.id);
				match action {
					Command::StyleToggle(_) => {
						if position.is_some() {
							ids.retain(|id| id != &entry.id);
						} else if entry.error.is_none() {
							ids.insert(0, entry.id.clone());
						} else {
							return;
						}
					}
					Command::StyleUp(_) => {
						if let Some(i) = position.filter(|i| *i > 0) {
							ids.swap(i, i - 1);
						}
					}
					Command::StyleDown(_) => {
						if let Some(i) = position.filter(|i| i + 1 < ids.len())
						{
							ids.swap(i, i + 1);
						}
					}
					_ => {}
				}
				self.preferences.values.style = Some(ids);
				self.setting_changed(Some(Setting::Theme));
				self.reload_styles();
				self.redraw();
				return;
			}
			Command::OpenConfig => {
				let result = self.preferences.ensure_file().and_then(|()| {
					open::that_detached(self.preferences.path().unwrap())
						.map_err(Into::into)
				});
				if let Err(error) = result {
					self.preferences.settings_warning =
						Some(format!("Cannot open settings: {error}"));
				}
				self.redraw();
				return;
			}
			Command::SystemTheme => {
				self.args.overrides.retain(|f| *f != Setting::Theme);
				self.args.theme = None;
				self.args.style = None;
				self.preferences.follow_system();
				self.apply_saved_settings();
				self.preferences.schedule_save();
				return;
			}
			Command::Settings => {
				self.tab_strip.cancel_drag();
				self.interaction.panel_open = !self.interaction.panel_open;
				self.interaction.styles_open = false;
				self.interaction.pointer_down = None;
				self.interaction.drag_at = None;
				self.interaction.scrollbar = None;
				self.interaction.focus =
					self.interaction.panel_open.then_some(Command::Styles);
				self.refresh_hover();
				self.redraw();
				return;
			}
			Command::Reset => {
				self.preferences.values = ReaderSettings::default();
				// Reset also drops the saved theme preference, so the system
				// theme applies again immediately and on the next launch.
				if let Some(theme) =
					self.window.as_ref().and_then(|w| system_theme(w))
				{
					self.preferences.values.theme = theme;
				}
			}
			Command::Open => {
				if self.dialog_open {
					return;
				}
				self.dialog_open = true;
				let proxy = self.proxy.clone();
				std::thread::spawn(move || {
					let path = rfd::FileDialog::new()
						.add_filter(
							"Markdown",
							&["md", "markdown", "mdown", "txt"],
						)
						.pick_file();
					let _ = proxy.send_event(Event::Open(path));
				});
				return;
			}
			Command::Smaller => {
				self.preferences.values.font_size =
					(self.preferences.values.font_size - 1.0).max(10.0)
			}
			Command::Larger => {
				self.preferences.values.font_size =
					(self.preferences.values.font_size + 1.0).min(40.0)
			}
			Command::Narrower => {
				self.preferences.values.width =
					(self.preferences.values.width - 60.0).max(240.0)
			}
			Command::Wider => {
				self.preferences.values.width =
					(self.preferences.values.width + 60.0).min(1600.0)
			}
			Command::Align => {
				self.preferences.values.justify =
					!self.preferences.values.justify
			}
			Command::Hyphens => {
				self.preferences.values.hyphenate =
					!self.preferences.values.hyphenate
			}
			Command::CodeWrap => {
				self.preferences.values.codeblock_wrap =
					!self.preferences.values.codeblock_wrap
			}
			Command::Indent(em) => {
				self.preferences.values.paragraph_indent = f32::from(em)
			}
			Command::CjkType(value) => {
				self.preferences.values.cjk_type = value;
				self.setting_changed(Some(Setting::CjkType));
				self.request(false);
				self.redraw();
				return;
			}
		}
		let field = match action {
			Command::Smaller | Command::Larger => Some(Setting::FontSize),
			Command::Narrower | Command::Wider => Some(Setting::Width),
			Command::Align => Some(Setting::Justify),
			Command::Hyphens => Some(Setting::Hyphenate),
			Command::CodeWrap => Some(Setting::CodeblockWrap),
			Command::Indent(_) => Some(Setting::ParagraphIndent),
			Command::CjkType(_) => Some(Setting::CjkType),
			_ => None,
		};
		self.setting_changed(field);
		self.request(false);
		self.redraw();
	}
	pub(super) fn setting_changed(&mut self, field: Option<Setting>) {
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
		let (x, y) = self.view_geometry().document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		self.readers.session.snapshot.hit_test_text(
			x,
			y,
			&self.readers.session.horizontal,
			self.readers.session.accepted_revision,
		)
	}

	pub(super) fn text_under_cursor(&self) -> bool {
		let geometry = self.view_geometry();
		if !geometry
			.clip()
			.contains(self.interaction.cursor.0, self.interaction.cursor.1)
		{
			return false;
		}
		let (x, y) = geometry.document_point(
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		);
		self.readers.session.snapshot.contains_text(
			x,
			y,
			&self.readers.session.horizontal,
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
				bar.grab(x, y)
			} else {
				self.readers.session.scroll = bar.scroll_for(x, y, 0.0);
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
				if let Some(bar) = self.document_scrollbar() {
					self.readers.session.scroll =
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
			let can_scroll = (self.interaction.cursor.1 < TOP + 24.0
				&& self.readers.session.scroll > 0.0)
				|| (self.interaction.cursor.1 > height - BOTTOM - 24.0
					&& self.readers.session.scroll
						< (self.readers.session.snapshot.height
							- self.viewport())
						.max(0.0));
			self.interaction.drag_at =
				can_scroll.then(|| Instant::now() + Duration::from_millis(16));
			self.redraw();
		}
	}

	pub(super) fn copy_selection(&mut self) {
		if let Some(selection) = self.interaction.selection
			&& !selection.is_empty()
		{
			let text = self.readers.session.snapshot.extract_text(
				selection,
				self.readers.session.accepted_revision,
			);
			if !text.is_empty() {
				match self.clipboard.write(text) {
					Ok(()) => {
						self.status = "Copied selection".into();
						self.error = false;
					}
					Err(e) => {
						self.status = format!("Cannot copy: {e}");
						self.error = true;
					}
				}
				self.redraw();
			}
		}
	}
	pub(super) fn paste_markdown(&mut self) {
		let text = match self.clipboard.read() {
			Ok(text) => text,
			Err(error) => {
				self.status = format!("Cannot read clipboard: {error}");
				self.error = true;
				self.status_until =
					Some(Instant::now() + Duration::from_secs(3));
				self.redraw();
				return;
			}
		};
		if !crate::paste::looks_like_markdown(&text) {
			return;
		}
		let title = crate::paste::title_for(&text);
		self.paste_serial = self.paste_serial.wrapping_add(1);
		let filename =
			format!("{}-{}.md", sanitize_filename(&title), self.paste_serial);
		let path = self.paste_dir.path().join(filename);
		if let Err(error) = std::fs::write(&path, text) {
			self.status = format!("Cannot paste Markdown: {error}");
			self.error = true;
			self.status_until = Some(Instant::now() + Duration::from_secs(3));
			self.redraw();
			return;
		}
		self.open(path);
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
		self.preferences.values.stylesheet = previous.stylesheet.clone();
		if self.preferences.theme_preference().is_none() {
			self.preferences.values.theme = self
				.window
				.as_ref()
				.and_then(|w| system_theme(w))
				.unwrap_or_default();
		}
		for field in &self.args.overrides {
			self.preferences.values.copy_field(&previous, *field);
		}
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
