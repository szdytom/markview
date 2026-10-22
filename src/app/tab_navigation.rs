//! Adapt tab gestures to session order without involving document layout workers.
use super::{
	App,
	tab_strip::{TabDrag, TabLayout},
};
use std::time::{Duration, Instant};

impl<P: super::SendEvent> App<P> {
	pub(super) fn tab_at_cursor(&mut self) -> Option<usize> {
		let (x, y) = self.interaction.pointer.cursor();
		self.tab_layout().hit(x, y)
	}
	pub(super) fn tab_close_at_cursor(&mut self) -> Option<usize> {
		let (x, y) = self.interaction.pointer.cursor();
		let layout = self.tab_layout();
		layout
			.hit(x, y)
			.filter(|i| x >= layout.rects[*i].x + layout.rects[*i].w - 24.0)
	}
	pub(super) fn normalize_tab_scroll(&mut self) {
		let layout = self.tab_layout();
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
		self.tab_strip.scroll.wheel(delta, Instant::now());
		// Tabs slide under the resting pointer; the next motion re-arms the
		// preview for whichever one arrives there.
		self.dismiss_preview();
		self.move_tab_drag();
		self.redraw();
		true
	}
	pub(super) fn begin_tab_drag(&mut self, index: usize) {
		self.action(crate::state::Command::SelectTab(index));
		self.normalize_tab_scroll();
		let layout = self.tab_layout();
		let x = self.interaction.pointer.cursor().0;
		self.tab_strip.drag = Some(TabDrag {
			index,
			start: x,
			grab: x - layout.rects[index].x,
			last: x + layout.scroll,
			moving: false,
		});
	}
	pub(super) fn move_tab_drag(&mut self) {
		let Some(mut drag) = self.tab_strip.drag else {
			return;
		};
		let layout = self.tab_layout();
		let from = drag.index;
		drag.update(&layout, self.interaction.pointer.cursor().0);
		if !drag.moving {
			return;
		}
		self.readers.move_tab(from, drag.index);
		self.tab_strip.drag = Some(drag);
		self.schedule_tab_scroll(&layout);
		self.redraw();
	}
	fn schedule_tab_scroll(&mut self, layout: &TabLayout) {
		self.tab_strip.scroll_at =
			(layout.edge_scroll(self.interaction.pointer.cursor().0) != 0.0)
				.then(|| Instant::now() + Duration::from_millis(16));
	}
	pub(super) fn auto_scroll_tabs(&mut self, now: Instant) {
		if self.tab_strip.scroll_at.is_none_or(|d| d > now) {
			return;
		}
		self.tab_strip.scroll_at = None;
		if self.tab_strip.drag.is_none() {
			return;
		}
		let layout = self.tab_layout();
		self.tab_strip.scroll.set_max(layout.max_scroll);
		self.tab_strip.scroll.set(
			(layout.scroll
				+ layout.edge_scroll(self.interaction.pointer.cursor().0))
			.clamp(0.0, layout.max_scroll),
		);
		self.move_tab_drag();
	}
}
