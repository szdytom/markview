//! The table-of-contents drawer: the application's commands and pointer
//! routing around the chrome's geometry.
//!
//! The drawer is not a panel: opening it leaves `interaction.panel_open()`
//! false, so the document keeps scrolling, selecting and following links; only
//! the wheel over the drawer, the entry rows, and Up/Down while it is open are
//! routed to the list. A panel or a confirmation opened over it owns input
//! first, and `outline_owns_input` makes every drawer path stand down.
use super::App;
use crate::layout::Rect;
use crate::state::{Command, InteractionState, OutlineTree};

/// Whether the drawer claims a pointer position.
///
/// A panel or a confirmation draws over the drawer and owns input first, so
/// the drawer stands down while either is present: the panel's scrollbar drag
/// and its outside-click dismissal must still answer where the two overlap.
pub(super) fn claims_pointer(
	interaction: &InteractionState,
	drawer: Rect,
	x: f32,
	y: f32,
) -> bool {
	interaction.outline_owns_input() && drawer.contains(x, y)
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn set_outline_collapsed(&mut self, collapsed: bool) {
		self.readers.session.ensure_outline();
		self.readers.session.outline_tree = if collapsed {
			OutlineTree::all_collapsed(self.readers.session.outline_entries())
		} else {
			OutlineTree::default()
		};
		self.ensure_outline();
		if let Some(index) = self.interaction.outline_selection {
			self.reveal_outline(index);
		}
		self.redraw();
	}

	fn outline_rows(&self) -> Vec<usize> {
		self.readers
			.session
			.outline_tree
			.rows(self.readers.session.outline_entries())
	}

	pub(super) fn toggle_outline_entry(&mut self, index: usize) {
		self.readers.session.ensure_outline();
		if !OutlineTree::has_children(
			self.readers.session.outline_entries(),
			index,
		) {
			return;
		}
		self.readers.session.outline_tree.toggle(index);
		self.interaction.outline_selection = Some(index);
		self.interaction.focus = Some(Command::OutlineToggle(index));
		self.ensure_outline();
		self.reveal_outline(index);
		self.redraw();
	}

	/// The drawer's rectangle, matching what the chrome draws.
	pub(super) fn outline_drawer(&self) -> Rect {
		let (width, height, _) = self.dimensions();
		super::chrome::outline::rect(width, height, self.content_top())
	}

	/// Whether the pointer is over the open drawer, with no panel or
	/// confirmation owning input.
	pub(super) fn pointer_in_outline(&self) -> bool {
		claims_pointer(
			&self.interaction,
			self.outline_drawer(),
			self.interaction.cursor.0,
			self.interaction.cursor.1,
		)
	}

	/// Builds the session's outline when the drawer may read it, and keeps the
	/// drawer's scroll and selection inside it. While the drawer is closed
	/// this is one branch and no document walk.
	pub(super) fn ensure_outline(&mut self) {
		if !self.interaction.outline_open {
			return;
		}
		self.readers.session.ensure_outline();
		let drawer = self.outline_drawer();
		let rows = self.outline_rows();
		super::chrome::outline::normalize(drawer, &rows, &mut self.interaction);
	}

	/// Opens or closes the drawer on the reading position's entry.
	pub(super) fn toggle_outline(&mut self) {
		if self.interaction.outline_open {
			self.interaction.close_outline();
			self.redraw();
			return;
		}
		self.readers.session.ensure_outline();
		let entries = self.readers.session.outline_entries().len();
		let rows = self.outline_rows();
		let current =
			self.readers.session.current_outline().and_then(|index| {
				rows.iter().copied().take_while(|row| *row <= index).last()
			});
		self.interaction.toggle_outline(entries, current);
		if let Some(index) = self.interaction.outline_selection {
			self.reveal_outline(index);
		}
		self.redraw();
	}

	/// Scrolls the drawer's own list, leaving the document where it is.
	pub(super) fn scroll_outline(&mut self, delta: f32) {
		self.readers.session.ensure_outline();
		let entries = self.outline_rows().len();
		let max =
			super::chrome::outline::max_scroll(self.outline_drawer(), entries);
		self.interaction.scroll_outline(delta, max);
		self.redraw();
	}

	/// Moves the drawer's keyboard selection and keeps it in view.
	pub(super) fn move_outline(&mut self, delta: isize) {
		self.readers.session.ensure_outline();
		let rows = self.outline_rows();
		if !self.interaction.move_outline(delta, &rows) {
			return;
		}
		if let Some(index) = self.interaction.outline_selection {
			self.reveal_outline(index);
		}
		self.redraw();
	}

	/// Brings one entry row into the drawer's own viewport.
	pub(super) fn reveal_outline(&mut self, index: usize) {
		self.readers.session.ensure_outline();
		let rows = self.outline_rows();
		let Some(row) = rows.iter().position(|entry| *entry == index) else {
			return;
		};
		self.interaction.outline_scroll = super::chrome::outline::reveal(
			self.outline_drawer(),
			rows.len(),
			self.interaction.outline_scroll,
			row,
		);
	}

	/// Jumps the document to one entry through the ordinary anchor path.
	pub(super) fn goto_outline(&mut self, index: usize) {
		self.readers.session.ensure_outline();
		let anchor = self
			.readers
			.session
			.outline_anchor(index)
			.map(str::to_owned);
		if let Some(anchor) = anchor {
			self.interaction.outline_selection = Some(index);
			self.goto_anchor(anchor);
		}
	}
}

#[cfg(test)]
#[path = "outline_tests.rs"]
mod tests;
