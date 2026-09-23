//! The option list a control opens: the application's commands and input
//! routing around the chrome's geometry.
//!
//! The list is an overlay, not a panel: the page it belongs to stays open and
//! keeps its scroll. While it is up it owns input the way a confirmation does,
//! which is the rule this file exists to keep in one place — a press on it
//! picks an option, a press anywhere else closes it without reaching the page
//! it covered, and every key but a modified chord is its own.
use super::App;
use crate::app::chrome::components::Menu;
use crate::state::Command;
use winit::keyboard::{Key, NamedKey};

impl<P: super::SendEvent> App<P> {
	/// The open list, measured against the page that holds its row.
	///
	/// Measuring is what decides which options are drawn, so the state keeps
	/// the window it measured: the drawing, the keys and the pointer all read
	/// the same list, and the highlight is always one of the options on screen.
	/// A list with nowhere to stand — its row scrolled out of the page, or the
	/// page closed — closes with it rather than answering for what nobody sees.
	pub(super) fn dropdown_menu(&mut self) -> Option<Menu> {
		let mut open = self.interaction.dropdown?;
		let menu = self.chrome().dropdown_menu(&mut open);
		if menu.is_none() {
			self.close_dropdown();
			return None;
		}
		self.interaction.dropdown = Some(open);
		menu
	}

	/// Closes the open list, leaving focus on the control that opened it.
	///
	/// An option's command is not a control: it disappears with the list, so
	/// the chooser takes focus back. `Tab` then continues from it instead of
	/// restarting, and `Enter` can open the list again. The chooser's own
	/// action is looked up rather than rebuilt from the highlight, because
	/// that is the exact command the page offers for `Enter` and `Tab` to
	/// find; the two carry different option numbers once the highlight has
	/// moved.
	pub(super) fn close_dropdown(&mut self) {
		let Some(open) = self.interaction.dropdown.take() else {
			return;
		};
		let anchor = self
			.buttons()
			.into_iter()
			.find(|button| {
				matches!(
					button.action,
					Command::ToggleDropdown(id, _) if id == open.id
				)
			})
			.map(|button| button.action)
			.unwrap_or(Command::ToggleDropdown(open.id, open.highlight));
		self.interaction.focus = Some(anchor);
	}

	/// The buttons the open list answers with, before the page's own.
	pub(super) fn dropdown_buttons(&mut self) -> Vec<super::Button> {
		self.dropdown_menu()
			.map_or_else(Vec::new, |menu| menu.buttons)
	}

	/// Moves the list's highlight with the pointer, so that clicking an option
	/// and pressing `Enter` on it mean the same thing.
	pub(super) fn hover_dropdown(&mut self) {
		let Some(menu) = self.dropdown_menu() else {
			return;
		};
		let (x, y) = self.interaction.cursor;
		let Some(slot) = menu
			.buttons
			.iter()
			.position(|button| button.rect.contains(x, y))
		else {
			return;
		};
		let action = menu.buttons[slot].action;
		let Some(mut open) = self.interaction.dropdown else {
			return;
		};
		open.highlight = open.offset + slot;
		self.interaction.dropdown = Some(open);
		self.interaction.focus = Some(action);
		self.interaction.focus_visible = true;
	}

	/// Answers the keys an open list owns, reporting whether it took the key:
	/// one it hands back still reaches the page behind it.
	pub(super) fn dropdown_key(&mut self, key: &Key) -> bool {
		if self.interaction.dropdown.is_none() {
			return false;
		}
		// Measuring first brings the stored window of options up to date. A
		// list with no anchor has already closed, so the key is the page's.
		let Some(menu) = self.dropdown_menu() else {
			return false;
		};
		let Some(mut open) = self.interaction.dropdown else {
			return false;
		};
		let options = menu.options;
		match key {
			Key::Named(NamedKey::ArrowUp) => open.step(-1, options),
			Key::Named(NamedKey::ArrowDown) => open.step(1, options),
			Key::Named(NamedKey::Enter) => {
				// A committed option closes the list itself, once the setting
				// it names is in force, so the chooser it returns focus to
				// already carries that option.
				match menu.chosen() {
					Some(action) => self.action(action),
					None => self.close_dropdown(),
				}
				return true;
			}
			Key::Named(NamedKey::Escape) => {
				self.close_dropdown();
				return true;
			}
			// Closing hands the tab on to the page's own traversal, which
			// continues from the chooser the focus returns to.
			Key::Named(NamedKey::Tab) => {
				self.close_dropdown();
				return false;
			}
			// A chord with a modifier belongs to the reader, menus or not.
			_ if self.interaction.modifiers.control_key() => return false,
			// Nothing behind the list answers while it is up.
			_ => return true,
		}
		self.interaction.dropdown = Some(open);
		// The highlight moved: measure again to bring it into view and to name
		// the option it now holds.
		self.interaction.focus =
			self.dropdown_menu().and_then(|menu| menu.chosen());
		self.interaction.focus_visible = true;
		true
	}
}

#[cfg(test)]
#[path = "dropdown/tests.rs"]
mod tests;
