//! The table-of-contents drawer: its own model, plus the application's
//! commands and pointer routing around the chrome's geometry.
//!
//! `Model` owns the drawer's open flag, list offset, keyboard selection and
//! slide animation. The interaction state routes to it through
//! `InteractionState.overlays`; `chrome::outline` paints and hit-tests it.
//!
//! The drawer is not a panel: opening it leaves `interaction.panel_open()`
//! false, so the document keeps scrolling, selecting and following links; only
//! the wheel over the drawer, the entry rows, and Up/Down while it is open are
//! routed to the list. A panel or a confirmation opened over it owns input
//! first, and `outline_owns_input` makes every drawer path stand down.
use super::App;
use crate::layout::Rect;
use crate::scroll::ScrollArea;
use crate::state::{Command, DrawerAnimation, InteractionState, OutlineTree};
use markview_ui_core::{Component, Invalidation, Update};
use std::time::Instant;

/// The drawer's own state: whether it is open, its list offset, the entry its
/// keyboard selection is on and its slide animation.
///
/// The drawer remembers its scroll and selection while closed, so reopening it
/// shows the rows the reader was last looking at. Geometry stays with the
/// chrome that paints and hit-tests the drawer; the host passes in the numbers
/// each transition needs.
#[derive(Default)]
pub(crate) struct Model {
	open: bool,
	scroll: ScrollArea,
	selection: Option<usize>,
	animation: Option<DrawerAnimation>,
}

/// The drawer's control-flow transitions.
pub(crate) enum Message {
	/// Open or close on the heading at the reading position. `entries` and
	/// `current` seed the selection only when the drawer remembers none.
	Toggle {
		entries: usize,
		current: Option<usize>,
	},
	Close,
	/// Move the keyboard selection by `delta` rows.
	Move {
		delta: isize,
		rows: Vec<usize>,
	},
}

impl Component for Model {
	type Message = Message;
	type Effect = ();
	type View<'a> = &'a Model;
	fn view(&self) -> &Model {
		self
	}
	fn update(&mut self, message: Message) -> Update<()> {
		match message {
			Message::Toggle { entries, current } => {
				self.toggle(entries, current);
			}
			Message::Close => self.close(),
			Message::Move { delta, rows } => {
				self.move_selection(delta, &rows);
			}
		}
		Update::new(Invalidation::Paint)
	}
}

impl Model {
	pub(crate) fn open(&self) -> bool {
		self.open
	}

	pub(crate) fn selection(&self) -> Option<usize> {
		self.selection
	}

	pub(crate) fn scroll_offset(&self) -> f32 {
		self.scroll.offset()
	}

	#[cfg(test)]
	pub(crate) fn scroll_mut(&mut self) -> &mut ScrollArea {
		&mut self.scroll
	}

	fn toggle(&mut self, entries: usize, current: Option<usize>) -> bool {
		DrawerAnimation::retarget(&mut self.animation, !self.open, self.open);
		self.open = !self.open;
		if self.open && self.selection.is_none() {
			self.selection = (entries > 0).then_some(current.unwrap_or(0));
		}
		self.open
	}

	fn close(&mut self) {
		DrawerAnimation::retarget(&mut self.animation, false, self.open);
		self.open = false;
	}

	/// Sets openness without touching the scroll, selection or animation, as
	/// a page change that drops the old drawer does.
	pub(crate) fn set_open(&mut self, open: bool) {
		self.open = open;
	}

	pub(crate) fn select(&mut self, index: Option<usize>) {
		self.selection = index;
	}

	fn move_selection(&mut self, delta: isize, rows: &[usize]) {
		if rows.is_empty() {
			self.selection = None;
			return;
		}
		let base = rows
			.iter()
			.position(|index| Some(*index) == self.selection)
			.unwrap_or(0) as isize;
		let next =
			rows[(base + delta).clamp(0, rows.len() as isize - 1) as usize];
		self.selection = Some(next);
	}

	/// Puts the remembered selection back inside the rows the drawer can show
	/// and clamps the offset to the list's new bound.
	pub(crate) fn normalize(&mut self, rows: &[usize], max: f32) {
		self.scroll.set_max(max);
		self.selection = rows
			.iter()
			.copied()
			.take_while(|index| *index <= self.selection.unwrap_or(0))
			.last()
			.or_else(|| rows.first().copied());
	}

	/// Brings one entry row into the drawer's own viewport.
	pub(crate) fn reveal_row(
		&mut self,
		drawer: Rect,
		rows: &[usize],
		index: usize,
	) {
		let Some(row) = rows.iter().position(|entry| *entry == index) else {
			return;
		};
		let max = super::chrome::outline::max_scroll(drawer, rows.len());
		self.scroll.set_max(max);
		self.scroll.set(super::chrome::outline::reveal(
			drawer,
			rows.len(),
			self.scroll.offset(),
			row,
		));
	}

	pub(crate) fn scroll_by(&mut self, delta: f32, max: f32) {
		self.scroll.set_max(max);
		self.scroll.by(delta);
	}

	pub(crate) fn wheel(&mut self, delta: f32, max: f32, now: Instant) {
		self.scroll.set_max(max);
		self.scroll.wheel(delta, now);
	}

	pub(crate) fn cancel_scroll(&mut self) {
		self.scroll.cancel();
	}

	pub(crate) fn advance_scroll(&mut self, now: Instant) {
		self.scroll.advance(now);
	}

	pub(crate) fn scroll_deadline(&self, now: Instant) -> Option<Instant> {
		self.scroll.deadline(now)
	}

	pub(crate) fn scroll_animating(&self) -> bool {
		self.scroll.animating()
	}

	/// The drawer's slide reveal at `now`.
	pub(crate) fn reveal(&self) -> f32 {
		DrawerAnimation::reveal(&self.animation, self.open, Instant::now())
	}

	pub(crate) fn animating(&self) -> bool {
		self.animation.is_some()
	}

	pub(crate) fn advance_animation(&mut self, now: Instant) {
		if self
			.animation
			.as_ref()
			.is_some_and(|animation| animation.is_finished(now))
		{
			self.animation = None;
		}
	}
}

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
		if let Some(index) = self.interaction.overlays.outline.selection() {
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
		self.interaction.overlays.outline.select(Some(index));
		self.interaction.widgets.focus =
			Some(Command::OutlineToggle(index).widget_id());
		self.ensure_outline();
		self.reveal_outline(index);
		self.redraw();
	}

	/// The drawer's rectangle, matching what the chrome draws.
	pub(super) fn outline_drawer(&self) -> Rect {
		let (width, height, _) = self.dimensions();
		let mut drawer = super::chrome::outline::rect_above(
			width,
			height,
			self.content_top(),
			self.bottom(),
			self.preferences
				.values
				.outline_width
				.min(width - self.folder_inset()),
		);
		drawer.x +=
			drawer.w * (1.0 - self.interaction.overlays.outline.reveal());
		drawer
	}

	/// Whether the pointer is over the open drawer, with no panel or
	/// confirmation owning input.
	pub(super) fn pointer_in_outline(&self) -> bool {
		claims_pointer(
			&self.interaction,
			self.outline_drawer(),
			self.interaction.pointer.cursor().0,
			self.interaction.pointer.cursor().1,
		)
	}

	/// Builds the session's outline when the drawer may read it, and keeps the
	/// drawer's scroll and selection inside it. While the drawer is closed
	/// this is one branch and no document walk.
	pub(super) fn ensure_outline(&mut self) {
		if !self.interaction.overlays.outline.open() {
			return;
		}
		self.readers.session.ensure_outline();
		let drawer = self.outline_drawer();
		let rows = self.outline_rows();
		super::chrome::outline::normalize(drawer, &rows, &mut self.interaction);
	}

	/// Opens or closes the drawer on the reading position's entry.
	pub(super) fn toggle_outline(&mut self) {
		if self.folder_inset() == 0. {
			self.set_folder_open(false);
		}
		if self.interaction.overlays.outline.open() {
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
		// A remembered selection may no longer be on screen: a shorter document
		// drops the entry and a collapsed parent hides it. Seed from the
		// reading position so Enter never activates a row the drawer cannot
		// show.
		let remembered = self.interaction.overlays.outline.selection();
		if !remembered.is_some_and(|index| rows.contains(&index)) {
			self.interaction
				.overlays
				.outline
				.select((entries > 0).then_some(current.unwrap_or(0)));
		}
		if let Some(index) = self.interaction.overlays.outline.selection() {
			self.reveal_outline(index);
		}
		self.redraw();
	}

	/// Pans the drawer's own list by a touch or touchpad stream.
	pub(super) fn scroll_outline(&mut self, delta: f32) {
		self.readers.session.ensure_outline();
		let entries = self.outline_rows().len();
		let max =
			super::chrome::outline::max_scroll(self.outline_drawer(), entries);
		self.interaction.overlays.outline.scroll_by(delta, max);
		self.redraw();
	}

	/// A wheel step over the drawer, eased like the document's own.
	pub(super) fn wheel_outline(&mut self, delta: f32) {
		self.readers.session.ensure_outline();
		let entries = self.outline_rows().len();
		let max =
			super::chrome::outline::max_scroll(self.outline_drawer(), entries);
		self.interaction.overlays.outline.wheel(
			delta,
			max,
			std::time::Instant::now(),
		);
		self.redraw();
	}

	/// Moves the drawer's keyboard selection and keeps it in view.
	pub(super) fn move_outline(&mut self, delta: isize) {
		self.readers.session.ensure_outline();
		let rows = self.outline_rows();
		if !self.interaction.move_outline(delta, &rows) {
			return;
		}
		if let Some(index) = self.interaction.overlays.outline.selection() {
			self.reveal_outline(index);
		}
		self.redraw();
	}

	/// Brings one entry row into the drawer's own viewport.
	pub(super) fn reveal_outline(&mut self, index: usize) {
		self.readers.session.ensure_outline();
		let rows = self.outline_rows();
		self.interaction.overlays.outline.reveal_row(
			self.outline_drawer(),
			&rows,
			index,
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
			self.interaction.overlays.outline.select(Some(index));
			self.goto_anchor(anchor);
		}
	}
}

#[cfg(test)]
#[path = "outline_tests.rs"]
mod tests;
