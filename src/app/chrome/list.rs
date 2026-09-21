//! The scrolling list the Styles and Fonts pages share.
//!
//! A page lays its rows out from the same [`List`] it clips and hit-tests
//! with, so the row a press finds is the row the frame drew, and the wheel,
//! the panel's scrollbar and the page itself move one offset.
use crate::{
	app::Button,
	layout::{Draw, Rect, Scrollbar, TextShaper},
	state::InteractionState,
};
use markview_core::style::{ColorField as C, Condition};

/// Equal-height rows inside a clipped viewport.
#[derive(Clone, Copy)]
pub(in crate::app) struct List {
	/// The whole panel: a page lays its fixed controls out in it, and it owns
	/// the scrollbar band every page of the settings panel shares.
	pub(super) panel: Rect,
	/// The clip: rows are drawn and clicked only inside it.
	pub(super) viewport: Rect,
	/// One row's height.
	row: f32,
	/// How many rows the page has.
	rows: usize,
	/// How far the list has scrolled, already clamped.
	pub(in crate::app) scroll: f32,
}

impl List {
	pub(super) fn new(
		panel: Rect,
		viewport: Rect,
		row: f32,
		rows: usize,
		scroll: f32,
	) -> Self {
		let mut list = Self {
			panel,
			viewport,
			row,
			rows,
			scroll: 0.0,
		};
		list.scroll = scroll.clamp(0.0, list.max_scroll());
		list
	}

	/// How far the list scrolls before its last row reaches the clip's bottom.
	pub(in crate::app) fn max_scroll(&self) -> f32 {
		(self.rows as f32 * self.row - self.viewport.h).max(0.0)
	}

	/// Whether a whole row fits. A shorter clip shows a sliver of one, which a
	/// page with its own footer prefers to say rather than draw.
	pub(super) fn fits(&self) -> bool {
		self.viewport.h >= self.row
	}

	/// The rows the clip shows, and the one just below it, so a row that is
	/// half scrolled off an edge is still laid out where it belongs.
	pub(super) fn visible(&self) -> std::ops::Range<usize> {
		if self.rows == 0 || self.viewport.h <= 0.0 {
			return 0..0;
		}
		let first = (self.scroll / self.row).floor().max(0.0) as usize;
		let last = (((self.scroll + self.viewport.h) / self.row).ceil()
			as usize + 1)
			.min(self.rows);
		first.min(self.rows)..last
	}

	/// Where one row's band sits at the current offset, in window coordinates.
	pub(super) fn row_rect(&self, index: usize) -> Rect {
		Rect {
			x: self.viewport.x,
			y: self.viewport.y + index as f32 * self.row - self.scroll,
			w: self.viewport.w,
			h: self.row,
		}
	}

	/// The buttons a press may reach, each clipped to the rows on screen, so a
	/// press outside the clip cannot find a row hidden above or below it.
	pub(super) fn hit(&self, buttons: Vec<Button>) -> Vec<Button> {
		buttons
			.into_iter()
			.filter_map(|mut b| {
				b.rect = b.rect.intersect(self.viewport)?;
				Some(b)
			})
			.collect()
	}

	/// One drawn row group, clipped to the rows on screen.
	pub(super) fn clip(&self, draws: Vec<Draw>) -> Draw {
		Draw::Clipped {
			rect: self.viewport,
			draws,
		}
	}

	/// The list's bar, in the panel's scrollbar band.
	pub(in crate::app) fn scrollbar(
		&self,
		ui: &TextShaper,
	) -> Option<Scrollbar> {
		Scrollbar::vertical(
			Rect {
				x: self.panel.x + self.panel.w - 16.0,
				w: 12.0,
				..self.viewport
			},
			self.scroll,
			self.viewport.h + self.max_scroll(),
			self.viewport.h,
			ui.stylesheet.scrollbar_metrics(),
		)
	}

	/// Paints the bar, when the list has one, beside the rows it scrolls.
	pub(super) fn draw_bar(
		&self,
		out: &mut Vec<Draw>,
		ui: &TextShaper,
		interaction: &InteractionState,
	) {
		let Some(bar) = self.scrollbar(ui) else {
			return;
		};
		let hovered = bar.hit(interaction.cursor.0, interaction.cursor.1);
		let (track, thumb) =
			bar.bars(hovered || interaction.panel_grab.is_some());
		out.push(super::components::line(
			track,
			Condition::Scrollbar,
			C::Track,
		));
		out.push(super::components::line(
			thumb,
			Condition::Scrollbar,
			if hovered { C::ThumbHover } else { C::Thumb },
		));
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn list(rows: usize, scroll: f32) -> List {
		List::new(
			Rect {
				x: 0.0,
				y: 0.0,
				w: 600.0,
				h: 620.0,
			},
			Rect {
				x: 0.0,
				y: 132.0,
				w: 600.0,
				h: 424.0,
			},
			60.0,
			rows,
			scroll,
		)
	}

	#[test]
	fn a_short_list_does_not_scroll() {
		let list = list(3, 100.0);
		assert_eq!(list.max_scroll(), 0.0);
		assert_eq!(list.scroll, 0.0);
		assert_eq!(list.visible(), 0..3);
	}

	#[test]
	fn scrolling_clamps_and_moves_the_rows() {
		let list = list(12, 10_000.0);
		assert_eq!(list.scroll, 12.0 * 60.0 - 424.0);
		assert_eq!(list.row_rect(0).y, 132.0 - list.scroll);
		// The last row is the one whose bottom meets the clip's bottom.
		let last = list.row_rect(11);
		assert!((last.y + last.h - 424.0 - 132.0).abs() < 0.5);
	}

	/// Only rows the clip can show may answer a press, and a partly visible
	/// one only where it is drawn.
	#[test]
	fn hit_testing_follows_the_clip() {
		use crate::state::Command;
		let list = list(12, 500.0);
		let buttons: Vec<Button> = (0..12)
			.map(|index| {
				let row = list.row_rect(index);
				Button {
					label: "Row",
					icon: None,
					active: false,
					kind: Default::default(),
					enabled: true,
					action: Command::StyleToggle(index),
					rect: Rect {
						x: row.x + 40.0,
						y: row.y + 4.0,
						w: 80.0,
						h: 32.0,
					},
				}
			})
			.collect();
		let hit = list.hit(buttons);
		// Exactly the rows whose own rect meets the clip answer a press.
		let expected: Vec<usize> = (0..12)
			.filter(|index| {
				let row = list.row_rect(*index);
				Rect {
					x: row.x + 40.0,
					y: row.y + 4.0,
					w: 80.0,
					h: 32.0,
				}
				.intersect(list.viewport)
				.is_some()
			})
			.collect();
		let reached: Vec<usize> = hit
			.iter()
			.map(|b| match b.action {
				Command::StyleToggle(index) => index,
				other => panic!("unexpected action {other:?}"),
			})
			.collect();
		assert_eq!(reached, expected);
		// The clip really did drop rows, and no reachable one is clipped off.
		assert!(reached.len() < 12);
		assert!(hit.iter().all(|b| {
			list.viewport.contains(b.rect.x, b.rect.y)
				&& list
					.viewport
					.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
		}));
	}

	#[test]
	fn a_clip_too_short_for_a_row_says_so() {
		assert!(list(4, 0.0).fits());
		let short = List::new(
			Rect {
				x: 0.0,
				y: 0.0,
				w: 500.0,
				h: 236.0,
			},
			Rect {
				x: 0.0,
				y: 164.0,
				w: 500.0,
				h: 8.0,
			},
			72.0,
			4,
			0.0,
		);
		assert!(!short.fits());
	}
}
