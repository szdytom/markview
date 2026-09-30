//! Pointer-driven text selection, with no windowing toolkit in the way.
//!
//! The machine is the one the reader has always run: a press names a reading
//! position and may extend it by grapheme, word or block; a second press
//! within the click interval moves the grain up; and a release that never
//! dragged may still activate the link it started on. All of that behaviour is
//! here. What is not here is where the state lives, and which events feed it.
//!
//! The host keeps the state and reaches it through [`Host`], so the same
//! machine drives the native window and any other front end. Core owns the
//! reading text and its geometry, this crate owns the pointer that walks it,
//! and a front end owns the events.

use markview_core::layout::LayoutSnapshot;
use markview_core::text::{TextPosition, TextSelection};
use std::time::Duration;
use web_time::Instant;

/// How long two presses still count as the same click.
const CLICK_INTERVAL: Duration = Duration::from_millis(500);
/// How far apart two presses may land and still be one click.
const CLICK_DISTANCE: f32 = 6.0;
/// How far a press must travel before it is a drag rather than a click. A
/// click that never moves this far stays a click, so the link it started on
/// is still reachable.
const DRAG_DISTANCE: f32 = 4.0;

/// A pointer position in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
	pub x: f32,
	pub y: f32,
}
impl Point {
	pub fn new(x: f32, y: f32) -> Self {
		Self { x, y }
	}
	fn distance(self, other: Self) -> f32 {
		(self.x - other.x).hypot(self.y - other.y)
	}
}

/// The modifier keys a selection gesture answers to.
///
/// A front end fills this from whatever it has: a windowing toolkit's modifier
/// state, or the four booleans a browser puts on an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
	pub shift: bool,
	pub control: bool,
	pub alt: bool,
	pub meta: bool,
}
impl Modifiers {
	pub const SHIFT: Self = Self {
		shift: true,
		control: false,
		alt: false,
		meta: false,
	};
	/// Shift extends an existing selection instead of starting a new one.
	pub fn shift_key(self) -> bool {
		self.shift
	}
}

/// Selection unit of an in-flight press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grain {
	Char,
	Word,
	Block,
}

/// An in-flight press: where it started and the link it would activate.
#[derive(Clone, Debug)]
pub struct Drag {
	pub start: Point,
	pub link: Option<String>,
	pub grain: Grain,
	/// The word or block a multi-click press selected, kept as the drag base.
	pub base: Option<TextSelection>,
}

/// The state the machine reads and writes, kept by the host.
///
/// Everything the machine needs to remember between events is one of these.
/// A host that derives `Default` starts with no selection, no press in flight
/// and no click remembered, which is what a freshly opened document wants.
pub trait Host {
	fn cursor(&self) -> Point;
	fn modifiers(&self) -> Modifiers;
	fn selection(&self) -> Option<TextSelection>;
	fn set_selection(&mut self, selection: Option<TextSelection>);
	fn drag(&self) -> Option<&Drag>;
	fn set_drag(&mut self, drag: Option<Drag>);
	/// Takes the press in flight, leaving none.
	fn take_drag(&mut self) -> Option<Drag>;
	fn dragged(&self) -> bool;
	fn set_dragged(&mut self, dragged: bool);
	/// When the drag past an edge next asks for a scroll step.
	fn auto_scroll_at(&self) -> Option<Instant>;
	fn set_auto_scroll_at(&mut self, at: Option<Instant>);
	fn last_click(&self) -> Option<(Instant, Point, u8)>;
	fn set_last_click(&mut self, click: Option<(Instant, Point, u8)>);

	/// A press on the page takes the pointer, so the keyboard focus leaves
	/// whatever chrome held it.
	fn blur(&mut self);
	/// The page stops owning the pointer, so a drag the host was holding for
	/// something else under the pointer ends with it.
	fn release_pointer(&mut self);
}

/// Extends a multi-click base selection to the word or block under the pointer,
/// keeping the base as the fixed edge.
fn extend(
	base: TextSelection,
	unit: TextSelection,
	position: TextPosition,
) -> TextSelection {
	let (first, last) = base.ordered();
	if position.cmp_reading(first).is_lt() {
		TextSelection {
			anchor: last,
			focus: unit.anchor,
		}
	} else if position.cmp_reading(last).is_gt() {
		TextSelection {
			anchor: first,
			focus: unit.focus,
		}
	} else {
		base
	}
}

/// The selection machine, implemented on every host.
pub trait Selection: Host {
	fn reset_clicks(&mut self) {
		self.set_last_click(None);
	}

	/// Repeated presses in the same place cycle single, word and block. A
	/// press that lands elsewhere, arrives late, or follows a click the page
	/// already answered starts again at one.
	fn click_count(&mut self, now: Instant) -> u8 {
		let cursor = self.cursor();
		let count = match self.last_click() {
			Some((at, point, count))
				if now.duration_since(at) <= CLICK_INTERVAL
					&& cursor.distance(point) <= CLICK_DISTANCE =>
			{
				count % 3 + 1
			}
			_ => 1,
		};
		self.set_last_click(Some((now, cursor, count)));
		count
	}

	fn begin_selection(
		&mut self,
		position: TextPosition,
		link: Option<String>,
	) {
		let anchor = if self.modifiers().shift_key() {
			self.selection().map_or(position, |s| s.anchor)
		} else {
			position
		};
		self.set_selection(Some(TextSelection {
			anchor,
			focus: position,
		}));
		self.set_drag(Some(Drag {
			start: self.cursor(),
			link,
			grain: Grain::Char,
			base: None,
		}));
		// Shift already extended a selection, so the press counts as dragged
		// and cannot also open the link under it.
		self.set_dragged(self.modifiers().shift_key());
		self.blur();
	}

	/// Starts a press that already selected a word or block, so dragging
	/// extends the selection by that unit instead of by grapheme. Returns
	/// false when there is no selection to start from.
	fn begin_grain_selection(
		&mut self,
		selection: Option<TextSelection>,
		grain: Grain,
	) -> bool {
		let Some(selection) = selection else {
			return false;
		};
		self.set_selection(Some(selection));
		self.set_drag(Some(Drag {
			start: self.cursor(),
			link: None,
			grain,
			base: Some(selection),
		}));
		self.set_dragged(false);
		self.blur();
		true
	}

	/// Starts a press that has no text under it, so only a link-like target
	/// can activate on release. A `<details>` marker is such a target.
	fn begin_link_press(&mut self, link: String) {
		self.set_drag(Some(Drag {
			start: self.cursor(),
			link: Some(link),
			grain: Grain::Char,
			base: None,
		}));
		self.set_dragged(false);
		self.blur();
	}

	fn move_selection(
		&mut self,
		position: Option<TextPosition>,
		snapshot: &LayoutSnapshot,
	) {
		let Some(drag) = self.drag() else {
			return;
		};
		let (start, grain, base) = (drag.start, drag.grain, drag.base);
		let dragged =
			self.dragged() || self.cursor().distance(start) >= DRAG_DISTANCE;
		self.set_dragged(dragged);
		if !dragged {
			return;
		}
		let Some(position) = position else {
			return;
		};
		let Some(selection) = self.selection() else {
			return;
		};
		let moved = match (grain, base) {
			(Grain::Char, _) | (_, None) => TextSelection {
				focus: position,
				..selection
			},
			(Grain::Word, Some(base)) => {
				match snapshot.select_word_at(position) {
					Some(word) => extend(base, word, position),
					None => selection,
				}
			}
			(Grain::Block, Some(base)) => {
				match snapshot.select_block_at(position) {
					Some(block) => extend(base, block, position),
					None => selection,
				}
			}
		};
		self.set_selection(Some(moved));
	}

	/// Ends the press, returning the link to activate: the one it started on,
	/// and only when the pointer never dragged away from it.
	fn finish_selection(
		&mut self,
		release_link: Option<&str>,
	) -> Option<String> {
		self.set_auto_scroll_at(None);
		let drag = self.take_drag()?;
		drag.link.filter(|link| {
			!self.dragged() && release_link == Some(link.as_str())
		})
	}

	fn clear_selection(&mut self) {
		self.set_selection(None);
		self.set_drag(None);
		self.set_auto_scroll_at(None);
		self.release_pointer();
	}
}
impl<T: Host> Selection for T {}
