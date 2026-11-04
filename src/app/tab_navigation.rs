//! Adapt tab gestures to session order without involving document layout workers.
use super::{App, tab_strip::TabDrag};
use crate::app::tab_metrics::CLOSE_HOVER;
use crate::layout::Rect;
use std::time::Instant;

impl<P: super::SendEvent> App<P> {
	pub(super) fn tab_at_cursor(&mut self) -> Option<usize> {
		let (x, y) = self.interaction.pointer.cursor();
		let layout = self.displayed_tab_layout();
		layout.hit_over(x, y, self.readers.active())
	}
	pub(super) fn tab_close_at_cursor(&mut self) -> Option<usize> {
		let (x, y) = self.interaction.pointer.cursor();
		let layout = self.displayed_tab_layout();
		layout.hit_over(x, y, self.readers.active()).filter(|i| {
			x >= layout.rects[*i].x + layout.rects[*i].w - CLOSE_HOVER
		})
	}
	pub(super) fn normalize_tab_scroll(&mut self) {
		let layout = self.resting_tab_layout();
		self.tab_strip.scroll.set_max(layout.max_scroll);
		if self.tab_strip.reveal_active {
			self.tab_strip.reveal_active = false;
			self.tab_strip
				.scroll
				.set(layout.reveal(self.readers.active()));
		}
	}
	pub(super) fn scroll_tabs(&mut self, delta: f32) -> bool {
		let layout = self.tab_layout();
		let (x, y) = self.interaction.pointer.cursor();
		if x < layout.viewport.x
			|| x > layout.viewport.x + layout.viewport.w
			|| !(0.0..super::TOP).contains(&y)
		{
			return false;
		}
		self.tab_strip.scroll.set_max(layout.max_scroll);
		self.tab_strip.scroll.wheel(
			delta,
			Instant::now(),
			markview_selection::WheelMode::Detent,
		);
		// Tabs slide under the resting pointer; the card closes, and the hover
		// re-arms for whichever tab arrives there without waiting for motion.
		self.dismiss_preview();
		self.move_tab_drag();
		self.hover_preview(Instant::now());
		self.redraw();
		true
	}
	pub(super) fn begin_tab_drag(&mut self, index: usize) {
		self.tab_strip.clear_slide();
		self.action(crate::state::Command::Chrome(
			crate::state::ChromeCommand::SelectTab(index),
		));
		self.normalize_tab_scroll();
		self.tab_strip.hold_shift();
		let layout = self.displayed_tab_layout();
		let x = self.interaction.pointer.cursor().0;
		self.tab_strip.drag = Some(TabDrag {
			index,
			start: x,
			grab: x - layout.rects[index].x,
			moving: false,
			slot: None,
		});
	}
	pub(super) fn move_tab_drag(&mut self) {
		let Some(mut drag) = self.tab_strip.drag else {
			return;
		};
		let layout = self.tab_layout();
		drag.update(&layout, self.interaction.pointer.cursor().0);
		self.tab_strip.drag = Some(drag);
		if drag.moving {
			self.tab_strip
				.retarget_shift(&layout, &self.tab_metrics.widths);
			self.redraw();
		}
	}
	/// Drops the held tab where the pointer indicates, sliding it and its
	/// neighbors into their new places.
	pub(super) fn finish_tab_drag(&mut self) {
		let Some(drag) = self.tab_strip.drag else {
			return;
		};
		let Some(to) = drag.drop_index() else {
			return;
		};
		let layout = self.tab_layout();
		if drag.index >= layout.rects.len() || to >= layout.rects.len() {
			return;
		}
		let mut shown = layout.rects.clone();
		if let Some(offsets) = self.tab_strip.drag_offsets(Instant::now()) {
			for (rect, offset) in shown.iter_mut().zip(offsets) {
				rect.x += offset;
			}
		}
		let x = self.interaction.pointer.cursor().0;
		let dragged = Rect {
			x: (x - drag.grab).clamp(
				layout.viewport.x,
				(layout.viewport.x + layout.viewport.w
					- layout.rects[drag.index].w)
					.max(layout.viewport.x),
			),
			..layout.rects[drag.index]
		};
		if self.readers.move_tab(drag.index, to) {
			self.tab_strip.begin_reorder(
				&layout.rects,
				&shown,
				drag.index,
				to,
				dragged,
			);
			self.redraw();
		}
	}
}
