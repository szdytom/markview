//! Delayed hover previews shared by every hover target.
//!
//! The card itself is chrome (`chrome::preview`); this module owns what the
//! pointer rests on, when the delay elapses, and which target produced the open
//! card. A target only supplies the card's content, so a new preview source
//! (a link, say) adds a `Target` arm instead of a second popup.
//!
//! A page preview is not a screenshot: the card reserves a body rectangle, and
//! the renderer composes the tab's layout instructions into it at device
//! resolution. Nothing is copied or cached between frames.
use super::tabs;
use super::{
	App, TOP,
	chrome::preview::{self as card, Card},
};
use crate::{layout::Rect, render::Theme};
use std::time::{Duration, Instant};

/// How long the pointer must rest on a target before its preview opens.
pub(super) const SHOW_DELAY: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(super) struct Preview {
	hover: Option<Hover>,
	open: Option<Open>,
	/// The body rectangle the chrome reserved this frame, if the card has one.
	body: Option<Rect>,
}

/// What the pointer was resting on. Each owner names its targets, so a tab id
/// cannot collide with a link target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
	Tab(u64),
}

struct Hover {
	target: Target,
	at: Instant,
}

/// The open card's content. The chrome paints it; the application owns it.
pub(super) struct Open {
	pub(super) target: Target,
	pub(super) card: Card,
}

impl Preview {
	pub(super) fn open(&self) -> Option<&Open> {
		self.open.as_ref()
	}

	/// Whether a card is open or a hover is waiting for its delay.
	pub(super) fn tracking(&self) -> bool {
		self.hover.is_some() || self.open.is_some()
	}

	/// Records the target under the pointer and, when the pointer left the
	/// target whose card is open, closes that card.
	pub(super) fn hover(
		&mut self,
		target: Option<Target>,
		now: Instant,
	) -> bool {
		let mut changed = false;
		if self
			.open
			.as_ref()
			.is_some_and(|open| Some(open.target) != target)
		{
			self.open = None;
			self.body = None;
			changed = true;
		}
		if self.hover.as_ref().map(|hover| hover.target) != target {
			self.hover = target.map(|target| Hover {
				target,
				at: now + SHOW_DELAY,
			});
		}
		changed
	}

	/// The target whose hover delay has elapsed, while no card is open.
	pub(super) fn due(&self, now: Instant) -> Option<Target> {
		if self.open.is_some() {
			return None;
		}
		self.hover
			.as_ref()
			.filter(|hover| hover.at <= now)
			.map(|hover| hover.target)
	}

	/// When the held hover becomes due, for the event loop's wake-up.
	pub(super) fn deadline(&self) -> Option<Instant> {
		if self.open.is_some() {
			return None;
		}
		self.hover.as_ref().map(|hover| hover.at)
	}

	pub(super) fn show(&mut self, open: Open) {
		self.open = Some(open);
		self.body = None;
	}

	/// The rectangle the chrome left for the card's preview body this frame.
	pub(super) fn body(&self) -> Option<Rect> {
		self.open.as_ref()?;
		self.body
	}

	pub(super) fn set_body(&mut self, rect: Option<Rect>) {
		self.body = rect;
	}

	/// Closes the card and forgets the hover, reporting whether a card was
	/// painted.
	pub(super) fn dismiss(&mut self) -> bool {
		self.hover = None;
		self.body = None;
		self.open.take().is_some()
	}
	/// The page scene the open card shows, for the renderer to compose.
	///
	/// Split from `App` so callers can hold the reader borrow and the renderer
	/// borrow at the same time.
	pub(super) fn scene<'a>(
		&'a self,
		readers: &'a tabs::Tabs,
		theme: Theme,
	) -> Option<crate::render::Preview<'a>> {
		let open = self.open()?;
		let rect = self.body()?;
		match open.target {
			Target::Tab(tab) => {
				let index = readers
					.entries()
					.iter()
					.position(|entry| entry.id == tab)?;
				let session = if index == readers.active() {
					&readers.session
				} else {
					&readers.entries()[index].session
				};
				let snapshot = &session.snapshot;
				if snapshot.blocks.is_empty() {
					return None;
				}
				Some(crate::render::Preview {
					snapshot,
					scroll: session.scrolling.offset,
					rect,
					theme,
					horizontal: &session.horizontal,
				})
			}
		}
	}
}

impl<P: super::SendEvent> App<P> {
	/// The hover target under the pointer after it moved, so its card can open
	/// once the delay elapses. Returns whether the painted card changed.
	pub(super) fn hover_preview(&mut self, now: Instant) -> bool {
		// Pointer motion in the document never reaches hover targets.
		if !self.preview.tracking()
			&& self.interaction.pointer.cursor().1 >= TOP
		{
			return false;
		}
		let target = if self.preview_allowed() {
			self.target_at_cursor()
		} else {
			None
		};
		self.preview.hover(target, now)
	}

	/// Opens the card whose hover delay elapsed.
	pub(super) fn preview_tick(&mut self, now: Instant) {
		if !self.preview_allowed() {
			if self.dismiss_preview() {
				self.redraw();
			}
			return;
		}
		let Some(target) = self.preview.due(now) else {
			return;
		};
		if self.open_preview(target) {
			self.redraw();
		} else {
			self.preview.dismiss();
		}
	}

	pub(super) fn dismiss_preview(&mut self) -> bool {
		self.preview.dismiss()
	}

	fn preview_allowed(&self) -> bool {
		!self.tab_strip.phone
			&& self.interaction.panel == crate::state::PanelPage::Closed
			&& !self.readers.session.search.open
			&& !self.interaction.overlays.modal_open()
			&& !self.interaction.overlays.dropdown_open()
			&& !self.interaction.overlays.viewer_open()
			&& self.tab_strip.drag.is_none()
			&& self.interaction.pointer.drag().is_none()
	}

	fn target_at_cursor(&mut self) -> Option<Target> {
		if self.interaction.pointer.cursor().1 >= TOP {
			return None;
		}
		self.tab_at_cursor()
			.map(|index| Target::Tab(self.readers.entries()[index].id))
	}

	fn open_preview(&mut self, target: Target) -> bool {
		match target {
			Target::Tab(tab) => self.open_tab_card(tab),
		}
	}

	/// Fills the card from a tab that is still open. Returns whether it could.
	fn open_tab_card(&mut self, tab: u64) -> bool {
		let Some(index) =
			self.readers.entries().iter().position(|e| e.id == tab)
		else {
			return false;
		};
		let path = self.readers.entries()[index].path.clone();
		let name = path
			.file_name()
			.unwrap_or(path.as_os_str())
			.to_string_lossy();
		let title = card::title_lines(&mut self.ui, &name);
		let detail = card::detail_line(&mut self.ui, &path.to_string_lossy());
		let active = index == self.readers.active();
		let session = if active {
			&self.readers.session
		} else {
			&self.readers.entries()[index].session
		};
		// The active page already fills the window, so its card names it only.
		let body = !active && !session.snapshot.blocks.is_empty();
		self.preview.show(Open {
			target: Target::Tab(tab),
			card: Card {
				title,
				detail,
				body,
			},
		});
		true
	}
}

#[cfg(test)]
mod tests;
