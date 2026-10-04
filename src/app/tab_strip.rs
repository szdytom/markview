//! Tab strip geometry and transient gestures, independent of document sessions.
use crate::layout::Rect;
use std::time::Instant;

pub(super) const GAP: f32 = 2.0;

#[derive(Default)]
pub(super) struct TabStrip {
	pub phone: bool,
	pub drawer_scroll: f32,
	pub scroll: f32,
	pub drag: Option<TabDrag>,
	pub reveal_active: bool,
	pub scroll_at: Option<Instant>,
}
#[derive(Clone, Copy)]
pub(super) struct TabDrag {
	pub index: usize,
	pub start: f32,
	pub grab: f32,
	pub last: f32,
	pub moving: bool,
}
impl TabDrag {
	pub fn update(&mut self, layout: &TabLayout, x: f32) {
		self.moving |= (x - self.start).abs() >= 5.0;
		if !self.moving {
			return;
		}
		let content_x = x + layout.scroll;
		// Only cross neighbors in the direction of travel: unequal tab widths
		// must not cause a stationary pointer to swap back after a reorder.
		if content_x > self.last {
			while self.index + 1 < layout.rects.len()
				&& x > layout.rects[self.index + 1].x
					+ layout.rects[self.index + 1].w / 2.0
			{
				self.index += 1;
			}
		} else if content_x < self.last {
			while self.index > 0
				&& x < layout.rects[self.index - 1].x
					+ layout.rects[self.index - 1].w / 2.0
			{
				self.index -= 1;
			}
		}
		self.last = content_x;
	}
}
impl TabStrip {
	pub fn cancel_drag(&mut self) {
		self.reveal_active |= self.drag.is_some_and(|drag| drag.moving);
		self.drag = None;
		self.scroll_at = None;
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
