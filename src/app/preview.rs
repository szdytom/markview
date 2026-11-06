//! Delayed hover previews shared by every hover target.
//!
//! The card itself is chrome (`chrome::preview`); this module owns what the
//! pointer rests on, when the delay elapses, and which target produced the open
//! card. A target only supplies the card's content, so a new preview source
//! (a link, say) adds a `Target` arm instead of a second popup.
//!
//! A page preview is not a screenshot: the card reserves a body rectangle, and
//! the renderer composes the tab's layout instructions into it at device
//! resolution. Idle tabs share a bounded three-screen cache with tab restoration.
use super::tabs;
use super::{
	App, TOP,
	chrome::preview::{self as card, Card},
};
use crate::{
	layout::{Draw, LayoutSnapshot, PlacedBlock, Rect},
	render::Theme,
	state::ReaderSession,
};
use std::{
	collections::{HashMap, HashSet},
	ops::Range,
	time::{Duration, Instant},
};

/// How long the pointer must rest on a target before its preview opens.
pub(super) const SHOW_DELAY: Duration = Duration::from_millis(100);

/// Per-tab and shared budgets for the resume/hover cache.
pub(super) const PAGE_BUDGET: usize = 8 * 1024 * 1024;
pub(super) const CACHE_BUDGET: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct CachedSlice {
	pub first: usize,
	pub range: Range<f32>,
	pub bytes: usize,
}

/// Three screenfuls shared by tab restoration and hover previews.
pub(crate) struct Page {
	snapshot: LayoutSnapshot,
	scroll: f32,
	horizontal: HashMap<(usize, usize), f32>,
	slice: CachedSlice,
}

impl Page {
	pub(super) fn bytes(&self) -> usize {
		self.slice.bytes
	}
	pub(super) fn capture(session: &ReaderSession) -> Option<Self> {
		let source = &session.snapshot;
		if source.blocks.is_empty()
			|| (session.layout_pending && session.cached_slice.is_none())
		{
			return None;
		}
		let viewport = if session.viewport > 0. {
			session.viewport
		} else {
			source.width.max(1.) * card::BODY_RATIO
		};
		let scroll = session.scrolling.offset.max(0.0);
		let mut range = (scroll - viewport).max(0.0)
			..(scroll + viewport * 2.0).min(source.height);
		if let Some(cached) = &session.cached_slice {
			if !cached.covers(scroll, viewport, source.height) {
				return None;
			}
			range.start = range.start.max(cached.range.start);
			range.end = range.end.min(cached.range.end);
		}
		let start = source
			.blocks
			.partition_point(|b| b.y + b.height() < range.start);
		let end = source.blocks.partition_point(|b| b.y <= range.end);
		let blocks = &source.blocks[start..end];
		let mut bytes = 1024;
		for block in blocks {
			// Reject giant blocks before walking their drawing commands.
			if block.layout.draws.capacity() * std::mem::size_of::<Draw>()
				> PAGE_BUDGET
			{
				return None;
			}
			bytes += block.retained_bytes();
			if bytes > PAGE_BUDGET {
				return None;
			}
		}
		let mut snapshot = LayoutSnapshot {
			presentation_key: source.presentation_key,
			content_id: source.content_id,
			revision: source.revision,
			centered: source.centered,
			document_box: source.document_box.clone(),
			blocks: blocks.to_vec(),
			height: source.height,
			width: source.width,
			..Default::default()
		};
		snapshot.images.generation = source.images.generation;
		bytes += Self::capture_images(
			session,
			&mut snapshot,
			start,
			&range,
			PAGE_BUDGET - bytes,
		)?;
		let horizontal = session
			.horizontal
			.iter()
			.filter(|&(&(block, _), _)| (start..end).contains(&block))
			.map(|(&(block, command), &offset)| {
				((block - start, command), offset)
			})
			.collect();
		let first = start
			+ session.cached_slice.as_ref().map_or(0, |slice| slice.first);
		Some(Self {
			snapshot,
			scroll,
			horizontal,
			slice: CachedSlice {
				first,
				range,
				bytes,
			},
		})
	}
	fn capture_images(
		session: &ReaderSession,
		snapshot: &mut LayoutSnapshot,
		start: usize,
		range: &Range<f32>,
		budget: usize,
	) -> Option<usize> {
		let source = &session.snapshot;
		let mut bytes = 0;
		let mut allocations = HashSet::new();
		for (block_index, block) in snapshot.blocks.iter().enumerate() {
			let viewport = Rect {
				x: 0.,
				y: range.start - block.y,
				w: source.width.max(1.),
				h: range.end - range.start,
			};
			for (index, draw, _) in block.draws() {
				let Draw::Image {
					src, version, rect, ..
				} = draw
				else {
					continue;
				};
				let Some(mut rect) = block.rect(index, *rect) else {
					continue;
				};
				let (offset, clip) = block.command_view(
					index,
					start + block_index,
					&session.horizontal,
				);
				rect.x -= offset;
				let clip = clip
					.map_or(Some(viewport), |clip| viewport.intersect(clip));
				if clip.and_then(|clip| rect.intersect(clip)).is_none() {
					continue;
				}
				if let Some(info) = source.images.entries.get(src) {
					if !snapshot.images.entries.contains_key(src) {
						bytes += src.len()
							+ info.error.as_ref().map_or(0, String::capacity)
							+ 128;
					}
					snapshot.images.entries.insert(src.clone(), info.clone());
				}
				if let Some(pixels) = source.images.pixels.get(src, *version) {
					bytes += src.len() + 64;
					if allocations.insert(std::sync::Arc::as_ptr(&pixels)) {
						bytes += pixels.rgba.len();
					}
					if bytes > budget {
						return None;
					}
					snapshot.images.pixels.insert(
						src.clone(),
						*version,
						pixels,
					);
				}
			}
		}
		if bytes > budget {
			return None;
		}
		Some(bytes)
	}
	pub(super) fn restore(self, session: &mut ReaderSession) {
		if session.snapshot.blocks.is_empty() {
			session.snapshot = self.snapshot;
			session.horizontal = self.horizontal;
			session.scrolling.offset = self.scroll;
			session.cached_slice = Some(self.slice);
			session.layout_pending = true;
		} else {
			if !self.snapshot.images.entries.is_empty() {
				session.cached_slice = Some(CachedSlice {
					first: 0,
					// Only the captured slice has pinned pixels, even though
					// the whole geometry survives.
					range: self.slice.range,
					bytes: self.slice.bytes,
				});
			}
			session.snapshot.images.pixels = self.snapshot.images.pixels;
		}
	}
}

impl CachedSlice {
	pub(crate) fn target(
		&self,
		old: &LayoutSnapshot,
		new: &LayoutSnapshot,
		scroll: f32,
		viewport: f32,
	) -> Option<f32> {
		if !self.covers(scroll, viewport, old.height) {
			return Some(scroll);
		}
		let index = old
			.blocks
			.partition_point(|b| b.y <= scroll)
			.saturating_sub(1);
		let before = old.blocks.get(index)?;
		// Original indices disambiguate identical blocks outside the cached slice.
		let after = new.blocks.get(self.first + index)?;
		if before.content != after.content {
			return None;
		}
		let local = scroll - before.y;
		// Follow the text under the offset: a picture resized while the tab
		// was idle can move it inside a block whose origin stays put.
		// Loading labels are transient anchors when the geometry is unchanged.
		let relaid = before.height() != after.height()
			|| image_rects(before).ne(image_rects(after));
		if relaid
			&& let Some(y) = markview_layout::layout::intra_block_anchor(
				before, after, local,
			) {
			return Some(after.y + y);
		}
		Some(after.y + local)
	}

	pub(crate) fn covers(
		&self,
		scroll: f32,
		viewport: f32,
		height: f32,
	) -> bool {
		scroll >= self.range.start
			&& (scroll + viewport).min(height) <= self.range.end
	}
}

fn image_rects(block: &PlacedBlock) -> impl Iterator<Item = Rect> + '_ {
	block.draws().filter_map(|(command, draw, _)| {
		let Draw::Image { rect, .. } = draw else {
			return None;
		};
		block.rect(command, *rect)
	})
}

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
				let page = readers.entries()[index].preview.as_ref();
				let (snapshot, scroll, horizontal) = match page {
					Some(page) => (
						&page.snapshot,
						if page.snapshot.centered {
							let b = &page.snapshot.blocks[0];
							(b.y + b.height() / 2.
								- page.snapshot.width * card::BODY_RATIO / 2.)
								.max(0.)
						} else {
							page.scroll
						},
						&page.horizontal,
					),
					None => (
						&session.snapshot,
						session.scrolling.offset,
						&session.horizontal,
					),
				};
				if snapshot.blocks.is_empty() && page.is_none() {
					return None;
				}
				Some(crate::render::Preview {
					snapshot,
					scroll,
					rect,
					theme,
					horizontal,
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
		self.frame.focused
			&& !self.tab_strip.phone
			&& self.interaction.panel == crate::state::PanelPage::Closed
			&& !self.readers.session.search.open
			&& !self.interaction.overlays.modal_open()
			&& !self.interaction.overlays.quick_open.open()
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
		let (name, detail) = self.readers.card_labels(index);
		let title = card::title_lines(&mut self.ui, &name);
		let detail = card::detail_line(&mut self.ui, &detail);
		let active = index == self.readers.active();
		let session = if active {
			&self.readers.session
		} else {
			&self.readers.entries()[index].session
		};
		// The active page already fills the window, so its card names it only.
		let body = !active
			&& (!session.snapshot.blocks.is_empty()
				|| self.readers.entries()[index].preview.is_some());
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
