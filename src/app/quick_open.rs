//! The floating file picker owns its workspace search session.
use super::{App, SendEvent, chrome};
use crate::state::{ChromeCommand, Command, PanelPage, TextField};
use markview_ui_core::WidgetId;
use winit::keyboard::{Key, NamedKey};

#[derive(Default)]
pub(crate) struct Model {
	open: bool,
	selected: usize,
	scroll: f32,
	previous_focus: Option<WidgetId>,
	pub(in crate::app) search: super::start::Search,
}
impl Model {
	pub(in crate::app) fn open(&self) -> bool {
		self.open
	}
	pub(in crate::app) fn selected(&self) -> usize {
		self.selected
	}
	pub(in crate::app) fn scroll(&self) -> f32 {
		self.scroll
	}
	pub(in crate::app) fn reset(&mut self) {
		self.selected = 0;
		self.scroll = 0.0;
	}
}

impl<P: SendEvent> App<P> {
	pub(super) fn open_quick_open(&mut self) {
		if self.interaction.overlays.modal_open()
			|| self.interaction.overlays.viewer_open()
		{
			return;
		}
		if !self.interaction.overlays.quick_open.open() {
			let previous_focus = self.interaction.widgets.focus;
			self.clear_input_focus();
			self.close_search();
			self.dismiss_preview();
			self.cancel_gestures();
			self.interaction.pointer.cancel();
			self.readers.session.cancel_scroll_animation();
			self.tab_strip.cancel_drag();
			self.show_panel(PanelPage::Closed);
			let picker = &mut self.interaction.overlays.quick_open;
			picker.open = true;
			picker.previous_focus = previous_focus;
			picker.reset();
			self.refresh_start_files();
		}
		self.action(Command::Chrome(ChromeCommand::FocusInput(
			TextField::QuickOpen,
		)));
		self.interaction
			.overlays
			.quick_open
			.search
			.input
			.select_all(&mut self.ui);
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn close_quick_open(&mut self) {
		if !self.interaction.overlays.quick_open.open() {
			return;
		}
		self.clear_input_focus();
		let picker = &mut self.interaction.overlays.quick_open;
		picker.open = false;
		let previous = picker.previous_focus.take();
		self.interaction.widgets.captured = None;
		self.interaction.widgets.focus = previous.filter(|id| {
			self.buttons()
				.iter()
				.any(|button| button.id == *id && button.enabled)
		});
		self.sync_input();
		self.refresh_hover();
		self.redraw();
	}

	/// The chord precedes text input so it also works inside another field.
	pub(super) fn quick_open_shortcut(&mut self, key: &Key) -> bool {
		let mods = self.interaction.pointer.modifiers();
		let primary = if markview_platform::command_key() {
			mods.super_key()
		} else {
			mods.control_key()
		};
		if primary
			&& !mods.alt_key()
			&& !mods.shift_key()
			&& matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("p"))
		{
			self.open_quick_open();
			return true;
		}
		false
	}

	pub(super) fn quick_open_key(&mut self, key: &Key) -> bool {
		if !self.interaction.overlays.quick_open.open()
			|| self
				.interaction
				.overlays
				.quick_open
				.search
				.input
				.is_composing()
		{
			return false;
		}
		match key {
			Key::Named(NamedKey::Escape) => self.close_quick_open(),
			Key::Named(NamedKey::Enter) => self.submit_quick_open(),
			Key::Named(NamedKey::ArrowDown) => self.move_quick_open(1),
			Key::Named(NamedKey::ArrowUp) => self.move_quick_open(-1),
			Key::Named(NamedKey::Tab) => self.action(Command::Chrome(
				ChromeCommand::FocusInput(TextField::QuickOpen),
			)),
			_ => return false,
		}
		true
	}

	fn move_quick_open(&mut self, delta: isize) {
		let search = &self.interaction.overlays.quick_open.search;
		if search.pending || search.matches.is_empty() {
			return;
		}
		let len = search.matches.len();
		self.interaction.widgets.captured = None;
		let list = self.quick_open_geometry().list;
		let picker = &mut self.interaction.overlays.quick_open;
		picker.selected =
			picker.selected.saturating_add_signed(delta).min(len - 1);
		let row = list.row_rect(picker.selected);
		let correction = if row.y < list.viewport.y {
			row.y - list.viewport.y
		} else {
			(row.y + row.h - list.viewport.y - list.viewport.h).max(0.0)
		};
		picker.scroll =
			(list.scroll + correction).clamp(0.0, list.max_scroll());
		self.redraw();
	}

	pub(super) fn submit_quick_open(&mut self) {
		if !self.folder.file_search_available() {
			self.action(Command::Chrome(ChromeCommand::OpenFolder));
		} else if !self.interaction.overlays.quick_open.search.pending
			&& let Some(file) = self
				.interaction
				.overlays
				.quick_open
				.search
				.matches
				.get(self.interaction.overlays.quick_open.selected())
		{
			self.action(Command::Chrome(ChromeCommand::QuickOpenFile(file.id)));
		}
	}

	pub(super) fn quick_open_geometry(&self) -> chrome::quick_open::Geometry {
		let (width, height, _) = self.dimensions();
		chrome::quick_open::geometry(
			(width, height),
			self.interaction.overlays.quick_open.search.matches.len(),
			self.interaction.overlays.quick_open.scroll(),
		)
	}

	pub(super) fn scroll_quick_open(&mut self, delta: f32) {
		let list = self.quick_open_geometry().list;
		self.interaction.overlays.quick_open.scroll =
			(list.scroll + delta).clamp(0.0, list.max_scroll());
		self.interaction.widgets.captured = None;
		self.refresh_hover();
		self.redraw();
	}

	pub(super) fn press_quick_open(&mut self) -> bool {
		if !self.interaction.overlays.quick_open.open() {
			return false;
		}
		let (x, y) = self.interaction.pointer.cursor();
		if self.quick_open_geometry().panel.contains(x, y) {
			let buttons = self.buttons();
			self.press_widget(&buttons);
		} else {
			self.close_quick_open();
		}
		self.redraw();
		true
	}
}
