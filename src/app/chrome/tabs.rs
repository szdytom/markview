use super::controls::toolbar_right_edge;
use crate::app::tab_strip::{TabLayout, TabStrip};
use crate::layout::{Draw, Paint, Rect, TextShaper};
use crate::state::ReaderTab;
use markview_core::style::{ColorField as C, Condition};
use unicode_segmentation::UnicodeSegmentation;
pub(in crate::app) struct TabBar<'a> {
	pub(super) ui: &'a mut TextShaper,
	pub(super) strip: &'a TabStrip,
	pub(super) widths: &'a [(f32, f32)],
	pub(super) tabs: &'a [ReaderTab],
	pub(super) active_tab: usize,
	pub(super) cursor: (f32, f32),
	pub(super) width: f32,
}
impl TabBar<'_> {
	pub(in crate::app) fn layout(&mut self) -> TabLayout {
		let right = toolbar_right_edge(self.width) - 4.0;
		TabLayout::new(
			Rect {
				x: 10.0,
				y: 4.0,
				w: (right - 10.0).max(0.0),
				h: 32.0,
			},
			self.widths,
			self.strip.scroll,
		)
	}

	pub(super) fn draw_tabs(&mut self) -> Vec<Draw> {
		let layout = self.layout();
		let old = self.ui.appearance.clone();
		self.ui.appearance = crate::app::tab_metrics::tab_appearance(self.ui);
		let mut out = Vec::new();
		let dragged = self.strip.drag.filter(|d| d.moving);
		let mut indices: Vec<_> = (0..self.tabs.len())
			.filter(|i| dragged.is_none_or(|d| d.index != *i))
			.collect();
		if let Some(drag) = dragged {
			indices.push(drag.index);
		}
		for index in indices {
			let mut rect = layout.rects[index];
			if let Some(drag) = dragged.filter(|d| d.index == index) {
				rect.x = (self.cursor.0 - drag.grab).clamp(
					layout.viewport.x,
					(layout.viewport.x + layout.viewport.w - rect.w)
						.max(layout.viewport.x),
				);
			}
			if rect.intersect(layout.viewport).is_none() {
				continue;
			}
			let active = index == self.active_tab;
			out.push(Draw::Box {
				rect,
				chain: Condition::Toolbar.chain(),
				condition: Condition::Toolbar,
				radius: 0.0,
				border: 1.0,
				left_only: false,
			});
			let fill = if active {
				C::ActiveBackground
			} else if rect.contains(self.cursor.0, self.cursor.1) {
				C::HoverBackground
			} else {
				C::Background
			};
			out.push(Draw::Rect(rect, Paint::Styled(Condition::Toolbar, fill)));
			let name = self.tabs[index]
				.path
				.file_name()
				.unwrap_or(self.tabs[index].path.as_os_str())
				.to_string_lossy();
			let name = fit_label(self.ui, &name, rect.w - 36.0);
			out.extend(self.ui.label(
				&name,
				12.0,
				rect.x + 12.0,
				rect.y + 21.0,
				Paint::Styled(
					Condition::Toolbar,
					if active { C::Color } else { C::Muted },
				),
			));
			out.extend(self.ui.label(
				"×",
				16.0,
				rect.x + rect.w - 19.0,
				rect.y + 21.0,
				Paint::Styled(Condition::Toolbar, C::Muted),
			));
		}
		self.ui.appearance = old;
		let mut draws = vec![Draw::Clipped {
			rect: layout.viewport,
			draws: out,
		}];
		if layout.max_scroll > 0.0 {
			let w = layout.viewport.w * layout.viewport.w
				/ (layout.viewport.w + layout.max_scroll);
			draws.push(Draw::Rect(
				Rect {
					x: layout.viewport.x
						+ (layout.viewport.w - w) * layout.scroll
							/ layout.max_scroll,
					y: 37.0,
					w,
					h: 2.0,
				},
				Paint::Styled(Condition::Scrollbar, C::Thumb),
			));
		}
		draws
	}
}

// At minimum width show the first two graphemes without spending space on an ellipsis.
fn fit_label(ui: &mut TextShaper, name: &str, width: f32) -> String {
	if ui.text_width(name, 12.0) <= width {
		return name.to_owned();
	}
	let chars: Vec<_> = name.graphemes(true).collect();
	let mut lo = 2.min(chars.len());
	let mut hi = chars.len();
	if ui.text_width(&format!("{}…", chars[..lo].concat()), 12.0) > width {
		return chars[..lo].concat();
	}
	while lo < hi {
		let mid = (lo + hi).div_ceil(2);
		if ui.text_width(&format!("{}…", chars[..mid].concat()), 12.0) <= width
		{
			lo = mid;
		} else {
			hi = mid - 1;
		}
	}
	format!("{}…", chars[..lo].concat())
}

#[cfg(test)]
mod tests;
