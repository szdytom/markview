//! The pointer a browser drives: canvas coordinates in, reading text out.
//!
//! `markview-interaction` owns the gesture machine. This module owns the state
//! that machine reads and writes, and turns a canvas-local point into a
//! reading position through the published snapshot.

use markview_interaction::{DocumentInteraction, Horizontal};
use markview_interaction::{Drag, Grain, Host, Modifiers, Point, Selection};
use markview_layout::{
	layout::LayoutSnapshot,
	scene::Viewport,
	text::{TextPosition, TextSelection},
};
use web_time::Instant;

/// A canvas point, and everything needed to read the text under it.
pub(crate) struct Reading<'a> {
	pub(crate) snapshot: &'a LayoutSnapshot,
	pub(crate) revision: u64,
	pub(crate) viewport: Viewport,
	pub(crate) horizontal: &'a Horizontal,
}
impl Reading<'_> {
	pub(crate) fn context(&self) -> DocumentInteraction<'_> {
		DocumentInteraction {
			snapshot: self.snapshot,
			viewport: self.viewport,
			horizontal: self.horizontal,
			revision: self.revision,
		}
	}

	/// The reading position under a canvas-local point, if any text is near.
	fn position(&self, x: f32, y: f32) -> Option<TextPosition> {
		self.context().position(Point::new(x, y))
	}
}

/// The selection machine's host state, one per `Markview` handle.
#[derive(Default)]
pub(crate) struct Pointer {
	cursor: Point,
	modifiers: Modifiers,
	selection: Option<TextSelection>,
	drag: Option<Drag>,
	dragged: bool,
	last_click: Option<(Instant, Point, u8)>,
}
impl Pointer {
	/// The modifier keys the next press answers to.
	pub(crate) fn set_modifiers(&mut self, modifiers: Modifiers) {
		self.modifiers = modifiers;
	}

	/// Starts a press at a canvas-local point: a word or block selection on a
	/// repeated click, a plain one otherwise, nothing when no text is near.
	pub(crate) fn press(&mut self, reading: &Reading<'_>, x: f32, y: f32) {
		self.cursor = Point::new(x, y);
		if !reading.context().contains(self.cursor) {
			return;
		}
		let link = reading.context().link(self.cursor).map(str::to_owned);
		let Some(position) = reading.position(x, y) else {
			if let Some(link) = link {
				self.begin_link_press(link);
			}
			return;
		};
		let count = if self.modifiers.shift_key() {
			self.reset_clicks();
			1
		} else {
			self.click_count(Instant::now())
		};
		match count {
			2 => {
				if !self.begin_grain_selection(
					reading.snapshot.select_word_at(position),
					Grain::Word,
				) {
					self.begin_selection(position, link.clone());
				}
			}
			3 => {
				if !self.begin_grain_selection(
					reading.snapshot.select_block_at(position),
					Grain::Block,
				) {
					self.begin_selection(position, link.clone());
				}
			}
			_ => self.begin_selection(position, link.clone()),
		}
	}
	/// Extends an in-flight press toward a canvas-local point, reporting
	/// whether that moved the selection.
	pub(crate) fn drag_to(
		&mut self,
		reading: &Reading<'_>,
		x: f32,
		y: f32,
	) -> bool {
		self.cursor = Point::new(x, y);
		let position = reading.position(x, y);
		self.move_selection(position, reading.snapshot)
	}
	/// Ends the press at a canvas-local point.
	pub(crate) fn release(
		&mut self,
		x: f32,
		y: f32,
		link: Option<&str>,
	) -> Option<String> {
		self.cursor = Point::new(x, y);
		self.finish_selection(link)
	}
	/// Selects the whole snapshot.
	pub(crate) fn select_all(
		&mut self,
		snapshot: &LayoutSnapshot,
		revision: u64,
	) {
		self.set_selection(snapshot.select_all(revision));
		self.set_drag(None);
	}
	/// Drops any selection and the press in flight.
	pub(crate) fn clear(&mut self) {
		self.clear_selection();
	}
	/// The reading text of the selection, `""` when empty or stale.
	pub(crate) fn selected_text(
		&self,
		snapshot: &LayoutSnapshot,
		revision: u64,
	) -> String {
		let Some(selection) = self.selection.filter(|s| !s.is_empty()) else {
			return String::new();
		};
		snapshot.extract_text(selection, revision)
	}
	/// Carries the multi-click base onto the next snapshot the way the
	/// selection is carried, or drops it. A drag in flight extends from this
	/// base, so a base left on the old revision would build a selection
	/// `extract_text` rejects and the next publication clears.
	pub(crate) fn rebase_drag(
		&mut self,
		previous: &LayoutSnapshot,
		next: &LayoutSnapshot,
		from: u64,
		to: u64,
	) {
		let Some(drag) = &mut self.drag else {
			return;
		};
		if let Some(base) = drag.base {
			drag.base = previous.rebase_selection(next, base, from, to);
		}
	}
	/// Moves the selection and the drag base onto `to`. A pass only appends
	/// blocks, so every reading position survives unchanged and only the
	/// revision has to follow; a position still stamped with another revision
	/// is left alone.
	pub(crate) fn retag(&mut self, from: u64, to: u64) {
		if let Some(selection) = self.selection
			&& selection.anchor.revision == from
			&& selection.focus.revision == from
		{
			self.selection = Some(restamped(selection, to));
		}
		if let Some(drag) = &mut self.drag
			&& let Some(base) = drag.base
			&& base.anchor.revision == from
			&& base.focus.revision == from
		{
			drag.base = Some(restamped(base, to));
		}
	}
}

/// The same reading positions, stamped with another revision.
fn restamped(selection: TextSelection, revision: u64) -> TextSelection {
	TextSelection {
		anchor: TextPosition {
			revision,
			..selection.anchor
		},
		focus: TextPosition {
			revision,
			..selection.focus
		},
	}
}
impl Host for Pointer {
	fn cursor(&self) -> Point {
		self.cursor
	}
	fn modifiers(&self) -> Modifiers {
		self.modifiers
	}
	fn selection(&self) -> Option<TextSelection> {
		self.selection
	}
	fn set_selection(&mut self, selection: Option<TextSelection>) {
		self.selection = selection;
	}
	fn drag(&self) -> Option<&Drag> {
		self.drag.as_ref()
	}
	fn set_drag(&mut self, drag: Option<Drag>) {
		self.drag = drag;
	}
	fn take_drag(&mut self) -> Option<Drag> {
		self.drag.take()
	}
	fn dragged(&self) -> bool {
		self.dragged
	}
	fn set_dragged(&mut self, dragged: bool) {
		self.dragged = dragged;
	}
	fn last_click(&self) -> Option<(Instant, Point, u8)> {
		self.last_click
	}
	fn set_last_click(&mut self, click: Option<(Instant, Point, u8)>) {
		self.last_click = click;
	}
	/// The page has no chrome to hand focus to; the canvas keeps it.
	fn blur(&mut self) {}
	/// Pointer capture belongs to the page, so there is nothing to release.
	fn release_pointer(&mut self) {}
}
