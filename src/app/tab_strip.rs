//! Tab strip geometry and transient gestures, independent of document sessions.
use crate::{
	layout::Rect,
	state::{Animation, DrawerAnimation, Easing, Tween},
};
use markview_selection::Scroll;
use std::time::{Duration, Instant};

pub(super) const GAP: f32 = 2.0;
/// Firefox's `--tab-dragover-transition`.
const DRAG_MOTION: Duration = Duration::from_millis(200);
/// The opening and closing tab slide's duration, matching the tabs drawer.
const TAB_MOTION: Duration = Duration::from_millis(220);

#[derive(Default)]
pub(super) struct TabStrip {
	pub phone: bool,
	pub drawer_scroll: Scroll,
	pub scroll: Scroll,
	pub drag: Option<TabDrag>,
	pub reveal_active: bool,
	/// The drawer's slide when the tabs page opens or closes.
	animation: Option<DrawerAnimation>,
	/// Tabs sliding to new positions after an open, a close, or a drop.
	slide: Option<TabSlide>,
	/// The other tabs sliding aside to leave the dragged tab's gap.
	shift: Option<TabShift>,
}
struct TabSlide {
	/// Where each tab stood when the slide started, in its new order.
	from: Vec<Rect>,
	/// The old right edge of the whole strip.
	end: f32,
	animation: Option<Tween>,
}
/// The horizontal offsets the strip shows while a drag opens a gap.
struct TabShift {
	from: Vec<f32>,
	to: Vec<f32>,
	animation: Option<Tween>,
}
impl Animation for TabSlide {
	fn advance(&mut self, now: Instant) -> bool {
		self.animation.advance(now)
	}
}

impl Animation for TabShift {
	fn advance(&mut self, now: Instant) -> bool {
		self.animation.advance(now)
	}
}

impl TabShift {
	fn offsets(&self, now: Instant) -> Vec<f32> {
		self.animation.as_ref().map_or_else(
			|| self.to.clone(),
			|animation| {
				let t = animation.progress(now);
				self.from
					.iter()
					.zip(&self.to)
					.map(|(from, to)| from + (to - from) * t)
					.collect()
			},
		)
	}

	/// Continues from where the offsets are now, Firefox switching the
	/// transition target mid-flight.
	fn retarget(&mut self, to: Vec<f32>, now: Instant) {
		self.from = self.offsets(now);
		self.to = to;
		self.animation = self
			.from
			.iter()
			.zip(&self.to)
			.any(|(from, to)| (from - to).abs() > 0.01)
			.then(|| Tween::new(0.0, 1.0, now, DRAG_MOTION, Easing::TabDrag));
	}

	fn settled(&self) -> bool {
		self.animation.is_none()
	}
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

	/// Where each tab goes to open the gap a drop at `slot` leaves.
	fn shift_targets(
		&self,
		layout: &TabLayout,
		widths: &[(f32, f32)],
	) -> Vec<f32> {
		let count = layout.rects.len();
		let mut targets = vec![0.0; count];
		let Some(slot) = self.slot else {
			return targets;
		};
		let to = slot - usize::from(slot > self.index);
		if to == self.index || widths.len() != count {
			return targets;
		}
		let mut order: Vec<_> = (0..count).collect();
		order.remove(self.index);
		order.insert(to, self.index);
		let mut reordered = widths.to_vec();
		let dragged = reordered.remove(self.index);
		reordered.insert(to, dragged);
		let preview =
			TabLayout::new(layout.viewport, &reordered, layout.scroll);
		// Offsets stay relative to the resting strip, so a slide in flight
		// cannot bake its own motion into them.
		let resting = TabLayout::new(layout.viewport, widths, layout.scroll);
		let mut position = vec![0; count];
		for (at, index) in order.into_iter().enumerate() {
			position[index] = at;
		}
		for (index, target) in targets.iter_mut().enumerate() {
			*target = preview.rects[position[index]].x - resting.rects[index].x;
		}
		// The dragged tab follows the pointer instead.
		targets[self.index] = 0.0;
		targets
	}
}
impl TabStrip {
	/// Retargets the tabs drawer's slide between closed and open.
	pub(super) fn animate(&mut self, open: bool, was_open: bool) {
		DrawerAnimation::retarget(&mut self.animation, open, was_open);
	}

	/// The tabs drawer's slide reveal at `now`.
	pub(super) fn reveal(&self, open: bool, now: Instant) -> f32 {
		DrawerAnimation::reveal(&self.animation, open, now)
	}

	pub(super) fn animating(&self) -> bool {
		self.animation.is_some()
			|| self
				.slide
				.as_ref()
				.is_some_and(|slide| slide.animation.is_some())
			|| self.shift.as_ref().is_some_and(|shift| !shift.settled())
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
		let closed = self.shift.as_mut().is_some_and(|shift| {
			if shift
				.animation
				.as_ref()
				.is_some_and(|animation| animation.is_finished(now))
			{
				shift.animation = None;
			}
			shift.settled() && shift.to.iter().all(|offset| *offset == 0.0)
		});
		if closed {
			self.shift = None;
		}
	}

	/// Starts a new tab at zero width in its slot and slides the tabs after it
	/// right, the reverse of a close. Pass `layout` without the drag gap; the gap
	/// is baked in here.
	pub(super) fn begin_open(&mut self, layout: &TabLayout, index: usize) {
		// Phone mode hides the strip, so there is no tab to grow.
		if layout.viewport.w <= 0.0 || index > layout.rects.len() {
			return;
		}
		let mut rects = layout.rects.clone();
		if let Some(offsets) = self.drag_offsets(Instant::now()) {
			for (rect, offset) in rects.iter_mut().zip(offsets) {
				rect.x += offset;
			}
		}
		let x = rects
			.last()
			.map_or(layout.viewport.x, |last| last.x + last.w);
		let slot = rects.get(index).copied().unwrap_or(Rect {
			x,
			..layout.viewport
		});
		let mut from = rects.clone();
		from.insert(index, Rect { w: 0.0, ..slot });
		// A drag's gap would shift the new tab on top of the slide.
		self.shift = None;
		self.begin_slide(&rects, from, TAB_MOTION, Easing::CubicOut);
		// An empty strip ends at the viewport's left edge, not at zero.
		if let Some(slide) = &mut self.slide
			&& rects.is_empty()
		{
			slide.end = layout.viewport.x;
		}
	}

	/// Starts the surviving tabs at their displayed rects and slides them into
	/// the layout that follows the close. `index` is the tab about to leave.
	pub(super) fn begin_close(&mut self, rects: &[Rect], index: usize) {
		let mut from = rects.to_vec();
		if index >= from.len() {
			return;
		}
		if let Some(offsets) = self.drag_offsets(Instant::now()) {
			for (rect, offset) in from.iter_mut().zip(offsets) {
				rect.x += offset;
			}
		}
		self.shift = None;
		from.remove(index);
		self.begin_slide(rects, from, TAB_MOTION, Easing::CubicOut);
	}

	/// Starts every tab at its resting or dragged rect and slides it to the
	/// layout that follows moving the tab at `from` to `to`.
	pub(super) fn begin_reorder(
		&mut self,
		rects: &[Rect],
		shown: &[Rect],
		from: usize,
		to: usize,
		dragged: Rect,
	) {
		if from >= shown.len() || to >= shown.len() || from == to {
			return;
		}
		self.shift = None;
		let mut moved = shown.to_vec();
		moved.remove(from);
		moved.insert(to, dragged);
		self.begin_slide(rects, moved, DRAG_MOTION, Easing::TabDrag);
	}

	/// Slides the other tabs toward the gap the dragged tab would drop into.
	pub(super) fn retarget_shift(
		&mut self,
		layout: &TabLayout,
		widths: &[(f32, f32)],
	) {
		let Some(drag) = self.drag else {
			return;
		};
		let targets = drag.shift_targets(layout, widths);
		let now = Instant::now();
		match self.shift.as_mut() {
			Some(shift) if shift.to.len() == targets.len() => {
				if !shift
					.to
					.iter()
					.zip(&targets)
					.any(|(old, new)| (old - new).abs() > 0.01)
				{
					return;
				}
				shift.retarget(targets, now);
			}
			_ if targets.iter().all(|offset| *offset == 0.0) => {
				self.shift = None;
			}
			_ => {
				let mut shift = TabShift {
					from: vec![0.0; targets.len()],
					to: vec![0.0; targets.len()],
					animation: None,
				};
				shift.retarget(targets, now);
				self.shift = Some(shift);
			}
		}
	}

	/// The offsets the strip's tabs show while a drag opens a gap.
	pub(super) fn drag_offsets(&self, now: Instant) -> Option<Vec<f32>> {
		self.shift.as_ref().map(|shift| shift.offsets(now))
	}

	/// Freezes a gap that is still closing, so a new drag starts from what
	/// the strip paints instead of chasing the old animation.
	pub(super) fn hold_shift(&mut self) {
		let now = Instant::now();
		if let Some(shift) = &mut self.shift
			&& shift.animation.is_some()
		{
			shift.from = shift.offsets(now);
			shift.to = shift.from.clone();
			shift.animation = None;
		}
	}

	fn begin_slide(
		&mut self,
		rects: &[Rect],
		from: Vec<Rect>,
		duration: Duration,
		easing: Easing,
	) {
		let end = rects.last().map_or(0.0, |rect| rect.x + rect.w);
		self.slide = Some(TabSlide {
			from,
			end,
			animation: Some(Tween::new(
				0.0,
				1.0,
				Instant::now(),
				duration,
				easing,
			)),
		});
	}

	/// The right edge of the tabs, including a slide.
	#[cfg(test)]
	pub(super) fn end(&self, end: f32, tabs: usize) -> f32 {
		self.end_at(end, tabs, Instant::now())
	}

	pub(super) fn end_at(&self, end: f32, tabs: usize, now: Instant) -> f32 {
		self.slide_at(tabs, now)
			.map_or(end, |(t, slide)| slide.end + (end - slide.end) * t)
	}

	/// The tabs' rects with a slide applied, or none at rest.
	#[cfg(test)]
	pub(super) fn rects(&self, layout: &TabLayout) -> Option<Vec<Rect>> {
		self.rects_at(layout, Instant::now())
	}

	pub(super) fn rects_at(
		&self,
		layout: &TabLayout,
		now: Instant,
	) -> Option<Vec<Rect>> {
		let (t, slide) = self.slide_at(layout.rects.len(), now)?;
		let mut rects = layout.rects.clone();
		for (rect, from) in rects.iter_mut().zip(&slide.from) {
			rect.x = from.x + (rect.x - from.x) * t;
			rect.w = from.w + (rect.w - from.w) * t;
		}
		Some(rects)
	}

	fn slide_at(&self, tabs: usize, now: Instant) -> Option<(f32, &TabSlide)> {
		let slide = self.slide.as_ref()?;
		if slide.from.len() != tabs {
			return None;
		}
		let t = slide
			.animation
			.as_ref()
			.map_or(1.0, |animation| animation.progress(now));
		Some((t, slide))
	}

	/// Drops an in-flight slide so the tabs paint at rest before a drag
	/// measures its gap.
	pub(super) fn clear_slide(&mut self) {
		self.slide = None;
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

	#[cfg(test)]
	pub(super) fn freeze_shift(&mut self, elapsed: std::time::Duration) {
		self.shift
			.as_mut()
			.and_then(|shift| shift.animation.as_mut())
			.expect("a tab shift is animating")
			.freeze(elapsed);
	}

	pub fn cancel_drag(&mut self) {
		self.reveal_active |= self.drag.is_some_and(|drag| drag.moving);
		self.drag = None;
		if let Some(shift) = &mut self.shift {
			shift.retarget(vec![0.0; shift.to.len()], Instant::now());
		}
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
	/// The tab strip's edge scroll speed in logical pixels per second.
	pub fn edge_scroll_speed(&self, x: f32) -> f32 {
		if x < self.viewport.x + 24.0 && self.scroll > 0.0 {
			-625.0
		} else if x > self.viewport.x + self.viewport.w - 24.0
			&& self.scroll < self.max_scroll
		{
			625.0
		} else {
			0.0
		}
	}
}

#[cfg(test)]
mod tests;
