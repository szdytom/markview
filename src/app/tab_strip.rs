//! Tab strip geometry and transient gestures, independent of document sessions.
use crate::{layout::Rect, scroll::ScrollArea, state::DrawerAnimation};
use std::time::Instant;

pub(super) const GAP: f32 = 2.0;

#[derive(Default)]
pub(super) struct TabStrip {
	pub phone: bool,
	pub drawer_scroll: ScrollArea,
	pub scroll: ScrollArea,
	pub drag: Option<TabDrag>,
	pub reveal_active: bool,
	pub scroll_at: Option<Instant>,
	/// The drawer's slide when the tabs page opens or closes.
	animation: Option<DrawerAnimation>,
	/// Tabs sliding to new positions after a close or a drop.
	slide: Option<TabSlide>,
}
struct TabSlide {
	/// Where each tab stood when the slide started, in its new order.
	from: Vec<Rect>,
	/// The old right edge of the whole strip.
	end: f32,
	animation: Option<DrawerAnimation>,
}
#[derive(Clone, Copy)]
pub(super) struct TabDrag {
	/// The tab's index while the strip still rests.
	pub index: usize,
	pub start: f32,
	pub grab: f32,
	pub moving: bool,
	/// The boundary the pointer would insert the tab at, in resting order.
	pub slot: Option<usize>,
}
impl TabDrag {
	pub fn update(&mut self, layout: &TabLayout, x: f32) {
		self.moving |= (x - self.start).abs() >= 5.0;
		self.slot = self.moving.then(|| layout.slot(x));
	}

	/// The index the tab would take if dropped now, if that is a move.
	pub fn drop_index(&self) -> Option<usize> {
		let slot = self.slot?;
		let index = slot - usize::from(slot > self.index);
		(index != self.index).then_some(index)
	}
}
impl TabStrip {
	/// Retargets the tabs drawer's slide between closed and open.
	pub(super) fn animate(&mut self, open: bool, was_open: bool) {
		DrawerAnimation::retarget(&mut self.animation, open, was_open);
	}

	/// The tabs drawer's slide reveal at `now`.
	pub(super) fn reveal(&self, open: bool) -> f32 {
		DrawerAnimation::reveal(&self.animation, open, Instant::now())
	}

	pub(super) fn animating(&self) -> bool {
		self.animation.is_some()
			|| self
				.slide
				.as_ref()
				.is_some_and(|slide| slide.animation.is_some())
	}

	pub(super) fn advance_animation(&mut self, now: Instant) {
		if self
			.animation
			.as_ref()
			.is_some_and(|animation| animation.is_finished(now))
		{
			self.animation = None;
		}
		let settled = self.slide.as_ref().is_some_and(|slide| {
			slide
				.animation
				.as_ref()
				.is_some_and(|animation| animation.is_finished(now))
		});
		if settled {
			self.slide = None;
		}
	}

	/// Starts the surviving tabs at `rects` and slides them into the layout
	/// that follows the close. `index` is the tab about to leave.
	pub(super) fn begin_close(&mut self, rects: &[Rect], index: usize) {
		let mut from = rects.to_vec();
		if index >= from.len() {
			return;
		}
		from.remove(index);
		self.begin_slide(rects, from);
	}

	/// Starts every tab at its resting or dragged rect and slides it to the
	/// layout that follows moving the tab at `from` to `to`.
	pub(super) fn begin_reorder(
		&mut self,
		rects: &[Rect],
		from: usize,
		to: usize,
		dragged: Rect,
	) {
		if from >= rects.len() || to >= rects.len() || from == to {
			return;
		}
		let mut moved = rects.to_vec();
		moved.remove(from);
		moved.insert(to, dragged);
		self.begin_slide(rects, moved);
	}

	fn begin_slide(&mut self, rects: &[Rect], from: Vec<Rect>) {
		let end = rects.last().map_or(0.0, |rect| rect.x + rect.w);
		let mut animation = None;
		DrawerAnimation::retarget(&mut animation, false, true);
		self.slide = animation.map(|animation| TabSlide {
			from,
			end,
			animation: Some(animation),
		});
	}

	/// The right edge of the tabs, including a slide.
	pub(super) fn end(&self, end: f32, tabs: usize) -> f32 {
		self.slide_at(tabs)
			.map_or(end, |(t, slide)| slide.end + (end - slide.end) * t)
	}

	/// The tabs' rects with a slide applied, or none at rest.
	pub(super) fn rects(&self, layout: &TabLayout) -> Option<Vec<Rect>> {
		let (t, slide) = self.slide_at(layout.rects.len())?;
		let mut rects = layout.rects.clone();
		for (rect, from) in rects.iter_mut().zip(&slide.from) {
			rect.x = from.x + (rect.x - from.x) * t;
			rect.w = from.w + (rect.w - from.w) * t;
		}
		Some(rects)
	}

	fn slide_at(&self, tabs: usize) -> Option<(f32, &TabSlide)> {
		let slide = self.slide.as_ref()?;
		if slide.from.len() != tabs {
			return None;
		}
		let t = 1.0
			- DrawerAnimation::reveal(&slide.animation, false, Instant::now());
		Some((t, slide))
	}

	/// Drops the slide where it is, as a page change without animation does.
	#[cfg(test)]
	pub(super) fn stop_animation(&mut self) {
		self.animation = None;
		self.slide = None;
	}

	#[cfg(test)]
	pub(super) fn freeze_animation(&mut self, elapsed: std::time::Duration) {
		self.animation
			.as_mut()
			.expect("the tabs drawer is animating")
			.freeze(elapsed);
	}

	#[cfg(test)]
	pub(super) fn freeze_slide(&mut self, elapsed: std::time::Duration) {
		self.slide
			.as_mut()
			.and_then(|slide| slide.animation.as_mut())
			.expect("a tab slide is animating")
			.freeze(elapsed);
	}

	pub fn cancel_drag(&mut self) {
		self.reveal_active |= self.drag.is_some_and(|drag| drag.moving);
		self.drag = None;
		self.scroll_at = None;
	}
	/// Advances eased wheel motion on the strip and its drawer.
	pub(super) fn advance_scrolls(&mut self, now: Instant) {
		self.scroll.advance(now);
		self.drawer_scroll.advance(now);
	}
	pub(super) fn scroll_animating(&self) -> bool {
		self.scroll.animating() || self.drawer_scroll.animating()
	}
	pub(super) fn scroll_deadline(&self, now: Instant) -> Option<Instant> {
		[self.scroll.deadline(now), self.drawer_scroll.deadline(now)]
			.into_iter()
			.flatten()
			.chain(self.scroll_at)
			.min()
	}
}

pub(super) struct TabLayout {
	pub viewport: Rect,
	pub rects: Vec<Rect>,
	pub max_scroll: f32,
	pub scroll: f32,
}

#[derive(Default)]
pub(super) struct TabSpan {
	natural: f32,
	minimum: f32,
	gaps: f32,
}
impl TabSpan {
	pub fn new(widths: &[(f32, f32)]) -> Self {
		Self {
			natural: widths.iter().map(|(w, _)| w).sum(),
			minimum: widths.iter().map(|(_, w)| w).sum(),
			gaps: widths.len().saturating_sub(1) as f32 * GAP,
		}
	}
	fn fit(&self, width: f32, scroll: f32) -> (f32, f32, f32, f32) {
		let shrink = ((self.natural + self.gaps - width)
			/ (self.natural - self.minimum).max(1.0))
		.clamp(0.0, 1.0);
		let total =
			self.natural - (self.natural - self.minimum) * shrink + self.gaps;
		let max_scroll = (total - width).max(0.0);
		(shrink, total, max_scroll, scroll.clamp(0.0, max_scroll))
	}
	pub fn end(&self, viewport: Rect, scroll: f32) -> f32 {
		let (_, total, _, scroll) = self.fit(viewport.w, scroll);
		viewport.x + total - scroll
	}
}
impl TabLayout {
	pub fn new(viewport: Rect, widths: &[(f32, f32)], scroll: f32) -> Self {
		let (shrink, _, max_scroll, scroll) =
			TabSpan::new(widths).fit(viewport.w, scroll);
		let mut x = viewport.x - scroll;
		let rects = widths
			.iter()
			.map(|(natural, minimum)| {
				let w = natural - (natural - minimum) * shrink;
				let rect = Rect { x, w, ..viewport };
				x += w + GAP;
				rect
			})
			.collect();
		Self {
			viewport,
			rects,
			max_scroll,
			scroll,
		}
	}
	pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
		self.viewport
			.contains(x, y)
			.then(|| self.rects.iter().position(|r| r.contains(x, y)))
			.flatten()
	}
	/// The tab at `(x, y)` as painted: the active tab lies above the rest.
	pub fn hit_over(&self, x: f32, y: f32, active: usize) -> Option<usize> {
		if self.viewport.contains(x, y)
			&& self
				.rects
				.get(active)
				.is_some_and(|rect| rect.contains(x, y))
		{
			return Some(active);
		}
		self.hit(x, y)
	}
	/// The boundary before the tab whose center is past `x`.
	pub fn slot(&self, x: f32) -> usize {
		self.rects
			.iter()
			.position(|rect| x < rect.x + rect.w / 2.0)
			.unwrap_or(self.rects.len())
	}
	pub fn reveal(&self, index: usize) -> f32 {
		let Some(rect) = self.rects.get(index) else {
			return self.scroll;
		};
		let delta = if rect.x < self.viewport.x {
			rect.x - self.viewport.x
		} else {
			(rect.x + rect.w - self.viewport.x - self.viewport.w).max(0.0)
		};
		(self.scroll + delta).clamp(0.0, self.max_scroll)
	}
	pub fn edge_scroll(&self, x: f32) -> f32 {
		if x < self.viewport.x + 24.0 && self.scroll > 0.0 {
			-10.0
		} else if x > self.viewport.x + self.viewport.w - 24.0
			&& self.scroll < self.max_scroll
		{
			10.0
		} else {
			0.0
		}
	}
}

#[cfg(test)]
mod tests;
