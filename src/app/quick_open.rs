//! The floating file picker owns its workspace search session.
use super::{App, SendEvent, chrome};
use crate::state::{ChromeCommand, Command, PanelPage, Selection, TextField};
use markview_interaction::{Scroll, WheelMode};
use markview_ui_core::WidgetId;
use std::time::Instant;
use winit::keyboard::{Key, NamedKey};

#[derive(Default)]
pub(crate) struct Model {
	open: bool,
	selected: usize,
	scroll: Scroll,
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
		self.scroll.offset()
	}
	pub(in crate::app) fn scroll_mut(&mut self) -> &mut Scroll {
		&mut self.scroll
	}
	pub(in crate::app) fn scroll_ref(&self) -> &Scroll {
		&self.scroll
	}
	pub(in crate::app) fn set_scroll_max(&mut self, max: f32) {
		self.scroll.set_max(max);
	}
	pub(in crate::app) fn set_scroll(&mut self, offset: f32) {
		self.scroll.set(offset);
	}
	pub(in crate::app) fn cancel_scroll(&mut self) {
		self.scroll.cancel();
	}
	pub(in crate::app) fn wheel(
		&mut self,
		delta: f32,
		now: Instant,
		mode: WheelMode,
	) {
		self.scroll.wheel(delta, now, mode);
	}
	pub(in crate::app) fn reset(&mut self) {
		self.selected = 0;
		self.scroll.reset();
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
			picker.search.input.set_text(&mut self.ui, "");
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
		picker.cancel_scroll();
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
			mods.meta_key()
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
		let list = self.quick_open_list();
		let picker = &mut self.interaction.overlays.quick_open;
		picker.selected =
			picker.selected.saturating_add_signed(delta).min(len - 1);
		let row = list.row_rect(picker.selected);
		let correction = if row.y < list.viewport.y {
			row.y - list.viewport.y
		} else {
			(row.y + row.h - list.viewport.y - list.viewport.h).max(0.0)
		};
		picker.set_scroll(
			(list.scroll + correction).clamp(0.0, list.max_scroll()),
		);
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

	/// The picker's list, with the scroll area's bound refreshed to match.
	pub(super) fn quick_open_list(&mut self) -> chrome::list::List {
		let list = self.quick_open_geometry().list;
		self.interaction
			.overlays
			.quick_open
			.set_scroll_max(list.max_scroll());
		list
	}

	pub(super) fn quick_open_scrollbar(
		&mut self,
	) -> Option<crate::layout::Scrollbar> {
		if self.interaction.overlays.quick_open.search.pending {
			return None;
		}
		self.quick_open_list().scrollbar(&self.ui)
	}

	pub(super) fn wheel_quick_open(
		&mut self,
		delta: f32,
		mode: markview_interaction::WheelMode,
	) {
		self.quick_open_list();
		self.interaction
			.overlays
			.quick_open
			.wheel(delta, Instant::now(), mode);
		self.interaction.widgets.captured = None;
		self.refresh_hover();
		self.redraw();
	}

	/// Starts dragging the picker's scrollbar when a press lands on it.
	fn begin_quick_open_drag(&mut self) -> bool {
		let Some(bar) = self.quick_open_scrollbar() else {
			return false;
		};
		let (x, y) = self.interaction.pointer.cursor();
		if !bar.hit(x, y) {
			return false;
		}
		let picker = &mut self.interaction.overlays.quick_open;
		let grab = if bar.on_thumb(x, y) {
			picker.cancel_scroll();
			bar.grab(x, y)
		} else {
			picker.set_scroll(bar.scroll_for(x, y, 0.0));
			0.0
		};
		self.interaction.reset_clicks();
		self.interaction
			.pointer
			.begin_scrollbar(crate::state::ScrollbarDrag {
				target: crate::state::ScrollbarAxis::QuickOpen,
				grab,
			});
		self.refresh_hover();
		true
	}

	pub(super) fn press_quick_open(&mut self) -> bool {
		if !self.interaction.overlays.quick_open.open() {
			return false;
		}
		let (x, y) = self.interaction.pointer.cursor();
		if self.quick_open_geometry().panel.contains(x, y) {
			if !self.begin_quick_open_drag() {
				let buttons = self.buttons();
				self.press_widget(&buttons);
			}
		} else {
			self.close_quick_open();
		}
		self.redraw();
		true
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	#[test]
	fn wheel_scrolling_eases_and_stops_at_the_bound() {
		let start = Instant::now();
		let mut picker = Model::default();
		picker.set_scroll_max(600.0);
		picker.wheel(120.0, start, WheelMode::Detent);
		assert!(picker.scroll_ref().animating());
		assert_eq!(picker.scroll(), 0.0);
		assert!(
			picker
				.scroll_mut()
				.advance(start + Duration::from_millis(60))
		);
		assert!(picker.scroll() > 0.0 && picker.scroll() < 120.0);
		assert!(!picker.scroll_mut().advance(start + Duration::from_secs(1)));
		assert!((picker.scroll() - 120.0).abs() < 0.01);

		picker.wheel(10_000.0, start, WheelMode::Detent);
		picker.scroll_mut().advance(start + Duration::from_secs(1));
		assert!((picker.scroll() - 600.0).abs() < 0.01);
	}

	#[test]
	fn resetting_the_picker_stops_motion_and_returns_to_the_top() {
		let start = Instant::now();
		let mut picker = Model::default();
		picker.set_scroll_max(600.0);
		picker.wheel(120.0, start, WheelMode::Detent);
		picker
			.scroll_mut()
			.advance(start + Duration::from_millis(40));
		picker.reset();
		assert_eq!(picker.scroll(), 0.0);
		assert!(!picker.scroll_ref().animating());
	}
}
