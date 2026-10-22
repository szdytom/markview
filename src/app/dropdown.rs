//! The option list a control opens: the application's commands and input
//! routing around the chrome's geometry.
//!
//! The list is an overlay, not a panel: the page it belongs to stays open and
//! keeps its scroll. While it is up it owns input the way a confirmation does,
//! which is the rule this file exists to keep in one place — a press on it
//! picks an option, a press anywhere else closes it without reaching the page
//! it covered, and every key but a modified chord is its own.
//!
//! `Model` owns the open list and its anchor, highlight and drawn window. The
//! interaction state routes to it through `InteractionState.overlays`; every
//! other module opens, moves or closes the list with a `Message`, and drawing
//! borrows it read-only.
use super::{App, LINE_STEP};
use crate::app::chrome::components::Menu;
use crate::state::{Command, DropdownId};
use markview_ui_core::{Component, Invalidation, Update};
use winit::keyboard::{Key, NamedKey};

/// Wheel travel, in logical pixels, that moves the list one option. The
/// reader's own line step is the familiar distance for one notch.
pub(super) const NOTCH: f32 = LINE_STEP;

/// The option list a control has open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dropdown {
	id: DropdownId,
	/// The highlighted option, which `Enter` commits.
	highlight: usize,
	/// The first option drawn. It follows the highlight, so a list too long for
	/// the window always shows the part the reader is on.
	offset: usize,
	/// Wheel travel held back until it adds up to one option, so a trackpad's
	/// small deltas and a mouse's whole notches move the list alike.
	wheel: f32,
}

impl Dropdown {
	pub(crate) fn new(id: DropdownId, highlight: usize) -> Self {
		Self {
			id,
			highlight,
			offset: 0,
			wheel: 0.0,
		}
	}

	/// The control whose row the list hangs from.
	pub(crate) fn id(&self) -> DropdownId {
		self.id
	}

	/// The option the keyboard is on.
	pub(crate) fn highlight(&self) -> usize {
		self.highlight
	}

	/// The first option drawn.
	pub(crate) fn offset(&self) -> usize {
		self.offset
	}

	/// Moves the highlight by `steps`, wrapping round `count` options.
	pub(crate) fn step(&mut self, steps: i8, count: usize) {
		if count == 0 {
			return;
		}
		if steps < 0 {
			self.highlight = self.highlight.checked_sub(1).unwrap_or(count - 1);
		} else if self.highlight + 1 == count {
			self.highlight = 0;
		} else {
			self.highlight += 1;
		}
	}

	/// Keeps the highlight inside a window of `capacity` drawn options.
	pub(crate) fn follow(&mut self, capacity: usize) {
		if capacity == 0 {
			return;
		}
		let last = self.highlight.saturating_sub(capacity - 1);
		self.offset = self.offset.min(self.highlight).max(last);
	}
}

/// The dropdown's own state: the list it has open, if any.
#[derive(Default)]
pub(crate) struct Model {
	open: Option<Dropdown>,
}

/// The dropdown's control-flow transitions.
pub(crate) enum Message {
	/// Open the list anchored at `id` on `highlight`.
	Open { id: DropdownId, highlight: usize },
	/// Dismiss the list without choosing an option.
	Close,
	/// The reader committed the highlighted option; the list is done.
	Select,
	/// Move the highlight by `steps` across `options`, wrapping.
	Highlight { steps: i8, options: usize },
}

impl Component for Model {
	type Message = Message;
	type Effect = ();
	type View<'a> = Option<&'a Dropdown>;

	fn view(&self) -> Option<&Dropdown> {
		self.open.as_ref()
	}

	fn update(&mut self, message: Message) -> Update<()> {
		match message {
			Message::Open { id, highlight } => {
				self.open = Some(Dropdown::new(id, highlight));
			}
			Message::Close | Message::Select => self.open = None,
			Message::Highlight { steps, options } => {
				if let Some(open) = self.open.as_mut() {
					open.step(steps, options);
				}
			}
		}
		Update::new(Invalidation::Paint)
	}
}

impl Model {
	/// Sends one control-flow message.
	pub(crate) fn send(&mut self, message: Message) {
		self.update(message);
	}

	/// Whether a list currently owns input.
	pub(crate) fn is_open(&self) -> bool {
		self.open.is_some()
	}

	/// The open list, if any.
	pub(crate) fn current(&self) -> Option<&Dropdown> {
		self.view()
	}

	/// Stores the list the geometry pass measured, so drawing, the keys and
	/// the pointer read the same window.
	pub(crate) fn measured(&mut self, open: Dropdown) {
		self.open = Some(open);
	}

	/// Puts the highlight on `option`, as the pointer resting over it does.
	pub(crate) fn highlight(&mut self, option: usize) {
		if let Some(open) = self.open.as_mut() {
			open.highlight = option;
		}
	}

	/// Folds wheel travel into whole highlight steps, holding the remainder.
	/// Returns whether the highlight moved.
	pub(crate) fn wheel(&mut self, travel: f32, options: usize) -> bool {
		let Some(open) = self.open.as_mut() else {
			return false;
		};
		open.wheel += travel;
		let mut stepped = false;
		while open.wheel.abs() >= NOTCH {
			let forward = open.wheel > 0.0;
			open.step(if forward { 1 } else { -1 }, options);
			open.wheel -= if forward { NOTCH } else { -NOTCH };
			stepped = true;
		}
		stepped
	}

	/// Removes the open list without choosing, for a dismissal that returns
	/// focus to its chooser.
	pub(crate) fn dismiss(&mut self) -> Option<Dropdown> {
		let open = self.open;
		self.update(Message::Close);
		open
	}

	/// Removes the open list because its highlighted option was committed.
	pub(crate) fn select(&mut self) -> Option<Dropdown> {
		let open = self.open;
		self.update(Message::Select);
		open
	}
}

impl<P: super::SendEvent> App<P> {
	/// The open list, measured against the page that holds its row.
	///
	/// Measuring is what decides which options are drawn, so the state keeps
	/// the window it measured: the drawing, the keys and the pointer all read
	/// the same list, and the highlight is always one of the options on screen.
	/// A list with nowhere to stand — its row scrolled out of the page, or the
	/// page closed — closes with it rather than answering for what nobody sees.
	pub(super) fn dropdown_menu(&mut self) -> Option<Menu> {
		let mut measured =
			self.interaction.overlays.dropdown.current().copied()?;
		let menu = self.chrome().dropdown_menu(&mut measured);
		if menu.is_none() {
			self.close_dropdown();
			return None;
		}
		self.interaction.overlays.dropdown.measured(measured);
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
		let Some(open) = self.interaction.overlays.dropdown.dismiss() else {
			return;
		};
		self.focus_dropdown_anchor(open);
	}

	/// Closes the list after its highlighted option was committed.
	///
	/// The option's own command has already run; all that remains is the same
	/// focus return a dismissal does, so `Enter` reaches the chooser again.
	pub(super) fn select_dropdown(&mut self) {
		let Some(open) = self.interaction.overlays.dropdown.select() else {
			return;
		};
		self.focus_dropdown_anchor(open);
	}

	/// Returns focus to the control that anchored `open`.
	fn focus_dropdown_anchor(&mut self, open: Dropdown) {
		let anchor = self
			.buttons()
			.into_iter()
			.find(|button| {
				matches!(
					button.action,
					Command::ToggleDropdown(id, _) if id == open.id()
				)
			})
			.map(|button| button.action)
			.unwrap_or(Command::ToggleDropdown(open.id(), open.highlight()));
		self.interaction.widgets.focus = Some(anchor.widget_id());
	}

	/// The buttons the open list answers with, before the page's own.
	pub(super) fn dropdown_buttons(&mut self) -> Vec<super::Button> {
		self.dropdown_menu()
			.map_or_else(Vec::new, |menu| menu.buttons)
	}

	/// Moves the list's highlight with the wheel, one option per notch of
	/// travel.
	///
	/// The list owns the wheel wherever the pointer rests: the page behind it
	/// keeps its scroll, so the row the list hangs from cannot move out from
	/// under it. Travel shorter than a notch is held for the next event, so a
	/// trackpad's small deltas and a mouse's whole notches move alike.
	pub(super) fn wheel_dropdown(&mut self, travel: f32) {
		let Some(menu) = self.dropdown_menu() else {
			return;
		};
		if !self
			.interaction
			.overlays
			.dropdown
			.wheel(travel, menu.options)
		{
			return;
		}
		// Measuring again brings the moved highlight into the drawn window and
		// names the option it now holds.
		self.interaction.widgets.focus = self
			.dropdown_menu()
			.and_then(|menu| menu.chosen())
			.map(Command::widget_id);
		self.interaction.widgets.focus_visible = true;
		self.redraw();
	}

	/// Moves the list's highlight with the pointer, so that clicking an option
	/// and pressing `Enter` on it mean the same thing.
	pub(super) fn hover_dropdown(&mut self) {
		let Some(menu) = self.dropdown_menu() else {
			return;
		};
		let (x, y) = self.interaction.pointer.cursor();
		let Some(slot) = menu
			.buttons
			.iter()
			.position(|button| button.rect.contains(x, y))
		else {
			return;
		};
		let action = menu.buttons[slot].action;
		let Some(open) = self.interaction.overlays.dropdown.current() else {
			return;
		};
		let highlight = open.offset() + slot;
		self.interaction.overlays.dropdown.highlight(highlight);
		self.interaction.widgets.focus = Some(action.widget_id());
		self.interaction.widgets.focus_visible = true;
	}

	/// Answers the keys an open list owns, reporting whether it took the key:
	/// one it hands back still reaches the page behind it.
	pub(super) fn dropdown_key(&mut self, key: &Key) -> bool {
		if !self.interaction.overlays.dropdown.is_open() {
			return false;
		}
		// Measuring first brings the stored window of options up to date. A
		// list with no anchor has already closed, so the key is the page's.
		let Some(menu) = self.dropdown_menu() else {
			return false;
		};
		let options = menu.options;
		match key {
			Key::Named(NamedKey::ArrowUp) => {
				self.interaction
					.overlays
					.dropdown
					.send(Message::Highlight { steps: -1, options });
			}
			Key::Named(NamedKey::ArrowDown) => {
				self.interaction
					.overlays
					.dropdown
					.send(Message::Highlight { steps: 1, options });
			}
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
			_ if self.interaction.pointer.modifiers().control_key() => {
				return false;
			}
			// Nothing behind the list answers while it is up.
			_ => return true,
		}
		// The highlight moved: measure again to bring it into view and to name
		// the option it now holds.
		self.interaction.widgets.focus = self
			.dropdown_menu()
			.and_then(|menu| menu.chosen())
			.map(Command::widget_id);
		self.interaction.widgets.focus_visible = true;
		true
	}
}

#[cfg(test)]
#[path = "dropdown/tests.rs"]
mod tests;
