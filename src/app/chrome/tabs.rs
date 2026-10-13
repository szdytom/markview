use super::components::mix;
use crate::app::tab_strip::{GAP, TabLayout, TabStrip};
use crate::layout::{Draw, Paint, Rect, TextShaper};
use crate::settings::TabStyle;
use crate::state::ReaderTab;
use markview_layout::style::{ColorField as C, Condition};
use unicode_segmentation::UnicodeSegmentation;

const LEFT: &[markview_layout::scene::IconPath] =
	markview_icon::icon!("assets/ui/tab-left.svg");
const RIGHT: &[markview_layout::scene::IconPath] =
	markview_icon::icon!("assets/ui/tab-right.svg");
const ACTIVE_LEFT: &[markview_layout::scene::IconPath] =
	markview_icon::icon!("assets/ui/tab-active-left.svg");
const ACTIVE_RIGHT: &[markview_layout::scene::IconPath] =
	markview_icon::icon!("assets/ui/tab-active-right.svg");
const RADIUS: f32 = 8.0;

pub(in crate::app) struct TabBar<'a> {
	pub(super) style: TabStyle,
	pub(super) ui: &'a mut TextShaper,
	pub(super) strip: &'a TabStrip,
	pub(super) widths: &'a [(f32, f32)],
	pub(super) tabs: &'a [ReaderTab],
	pub(super) active_tab: usize,
	pub(super) cursor: (f32, f32),
	pub(super) viewport: Rect,
}
impl TabBar<'_> {
	pub(in crate::app) fn layout(&mut self) -> TabLayout {
		let mut viewport = self.viewport;
		if self.style == TabStyle::Underline {
			viewport.h -= 4.0;
		}
		TabLayout::new(viewport, self.widths, self.strip.scroll)
	}

	pub(super) fn draw_tabs(&mut self) -> Vec<Draw> {
		let layout = self.layout();
		let style = self.style;
		let old = self.ui.appearance.clone();
		self.ui.appearance = crate::app::tab_metrics::tab_appearance(self.ui);
		let mut out = Vec::new();
		let dragged = self.strip.drag.filter(|d| d.moving);
		let mut indices: Vec<_> = (0..self.tabs.len())
			.filter(|i| dragged.is_none_or(|d| d.index != *i))
			.collect();
		indices.sort_by_key(|index| *index == self.active_tab);
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
			let hovered = rect.contains(self.cursor.0, self.cursor.1);
			draw_tab_surface(&mut out, style, rect, active, hovered);
			if index + 1 < self.tabs.len()
				&& !active && index + 1 != self.active_tab
			{
				out.push(Draw::Rect(
					Rect {
						x: rect.x + rect.w - 1.0,
						y: rect.y + 8.0,
						w: 1.0,
						h: rect.h - 16.0,
					},
					Paint::Styled(Condition::Toolbar, C::BorderColor),
				));
			}

			let name = self.tabs[index]
				.path
				.file_name()
				.unwrap_or(self.tabs[index].path.as_os_str())
				.to_string_lossy();
			let name = fit_label(self.ui, &name, rect.w - 36.0);
			let label = if active {
				Paint::Styled(Condition::Toolbar, C::Color)
			} else if hovered {
				mix(
					self.ui,
					Paint::Styled(Condition::Toolbar, C::Muted),
					Paint::Styled(Condition::Toolbar, C::Color),
					0.7,
				)
			} else {
				Paint::Styled(Condition::Toolbar, C::Muted)
			};
			out.extend(self.ui.label(
				&name,
				12.0,
				rect.x + 12.0,
				rect.y + rect.h / 2.0 + 5.0,
				label,
			));
			out.push(Draw::Icon {
				paths: super::icons::CLOSE,
				paint: Paint::Styled(Condition::Toolbar, C::Muted),
				x: rect.x + rect.w - 22.0,
				y: rect.y + (rect.h - 16.0) / 2.0,
				size: 16.0,
			});
		}
		self.ui.appearance = old;
		let padding =
			if style == TabStyle::Connected && layout.max_scroll == 0.0 {
				RADIUS + GAP / 2.0
			} else {
				0.0
			};
		let mut draws = vec![Draw::Clipped {
			rect: Rect {
				x: layout.viewport.x - padding,
				w: layout.viewport.w + padding * 2.0,
				h: super::TOP - layout.viewport.y,
				..layout.viewport
			},
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
					y: 1.0,
					w,
					h: 2.0,
				},
				Paint::Styled(Condition::Scrollbar, C::Thumb),
			));
		}
		draws
	}
}

fn draw_tab_surface(
	out: &mut Vec<Draw>,
	style: TabStyle,
	rect: Rect,
	active: bool,
	hovered: bool,
) {
	let fill = if active {
		match style {
			TabStyle::Underline => {
				Paint::Styled(Condition::Panel, C::Background)
			}
			TabStyle::Connected => Paint::Background,
		}
	} else if hovered {
		Paint::Styled(Condition::Button, C::HoverBackground)
	} else {
		Paint::Styled(Condition::Toolbar, C::Background)
	};
	match style {
		TabStyle::Connected => draw_connected(out, rect, fill, active),
		TabStyle::Underline => {
			out.push(Draw::Rect(rect, fill));
			if active {
				out.push(Draw::Rect(
					Rect {
						y: rect.y + rect.h - 2.0,
						h: 2.0,
						..rect
					},
					Paint::Styled(Condition::Toolbar, C::Accent),
				));
			}
		}
	}
}

fn draw_connected(out: &mut Vec<Draw>, rect: Rect, fill: Paint, active: bool) {
	// Neighboring rounded bodies meet at the center of their hit-test gap.
	let body = Rect {
		x: rect.x - GAP / 2.0,
		w: rect.w + GAP,
		..rect
	};
	out.push(Draw::Rect(
		Rect {
			x: body.x + RADIUS - 1.0,
			w: body.w - 2.0 * RADIUS + 2.0,
			..body
		},
		fill,
	));
	// Active edges combine the body and its shared join to avoid a raster seam.
	let edges = if active {
		[
			(ACTIVE_LEFT, body.x - RADIUS),
			(ACTIVE_RIGHT, body.x + body.w - RADIUS),
		]
	} else {
		[(LEFT, body.x), (RIGHT, body.x + body.w - RADIUS)]
	};
	for (paths, x) in edges {
		out.push(Draw::Icon {
			paths,
			paint: fill,
			x,
			y: body.y,
			size: body.h,
		});
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
