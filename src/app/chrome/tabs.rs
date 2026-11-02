use super::components::mix;
use crate::app::tab_strip::{GAP, TabDrag, TabLayout, TabStrip};
use crate::layout::{Draw, Paint, Rect, TextShaper};
use crate::state::ReaderTab;
use markview_layout::style::{ColorField as C, Condition};
use std::time::Instant;
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
		TabLayout::new(self.viewport, self.widths, self.strip.scroll.offset())
	}

	pub(super) fn draw_tabs(&mut self) -> Vec<Draw> {
		let layout = self.layout();
		let mut rects = displayed(self.strip, &layout, Instant::now());
		let dragged = self.strip.drag.filter(|d| d.moving);
		let floating =
			floating_tab(dragged, &rects, layout.viewport, self.cursor.0);
		// A moving drag owns the hover: the tabs it covers are not hovered,
		// even where their own rects still lie under the pointer.
		let hovered_tab =
			hovered_tab(&rects, floating, self.cursor, self.active_tab);
		if let Some((index, rect)) = floating {
			rects[index] = rect;
		}
		let dragged = dragged.map(|drag| drag.index);
		let old = self.ui.appearance.clone();
		self.ui.appearance = crate::app::tab_metrics::tab_appearance(self.ui);
		let mut out = Vec::new();
		// Inactive tabs first, so separators sit above their fills.
		let active_tab = self.active_tab;
		for index in (0..self.tabs.len())
			.filter(|index| *index != active_tab && Some(*index) != dragged)
		{
			self.draw_tab(&mut out, index, &layout, &rects, hovered_tab);
		}
		out.extend(separators(&rects, self.active_tab, dragged, hovered_tab));
		if active_tab < self.tabs.len() && dragged != Some(active_tab) {
			self.draw_tab(&mut out, active_tab, &layout, &rects, hovered_tab);
		}
		if let Some(index) = dragged {
			self.draw_tab(&mut out, index, &layout, &rects, hovered_tab);
		}
		self.ui.appearance = old;
		let padding = if layout.max_scroll == 0.0 {
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

	fn draw_tab(
		&mut self,
		out: &mut Vec<Draw>,
		index: usize,
		layout: &TabLayout,
		rects: &[Rect],
		hovered_tab: Option<usize>,
	) {
		let rect = rects[index];
		if rect.intersect(layout.viewport).is_none() {
			return;
		}
		let active = index == self.active_tab;
		let hovered = hovered_tab == Some(index);
		let dragging = self
			.strip
			.drag
			.is_some_and(|drag| drag.moving && drag.index == index);
		draw_tab_surface(out, rect, active, hovered);
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
		// Hovering the close affordance brightens it like any other icon.
		let close_hovered =
			close_hovered(self.cursor.0, rect, hovered, dragging);
		out.push(Draw::Icon {
			paths: super::icons::CLOSE,
			paint: Paint::Styled(
				Condition::Toolbar,
				if close_hovered { C::Color } else { C::Muted },
			),
			x: rect.x + rect.w - 22.0,
			y: rect.y + (rect.h - 16.0) / 2.0,
			size: 16.0,
		});
	}
}

fn displayed(strip: &TabStrip, layout: &TabLayout, now: Instant) -> Vec<Rect> {
	let mut rects = strip.rects(layout).unwrap_or_else(|| layout.rects.clone());
	if let Some(offsets) = strip.drag_offsets(now) {
		for (rect, offset) in rects.iter_mut().zip(offsets) {
			rect.x += offset;
		}
	}
	rects
}

/// The dragged tab's painted rect at `x`.
fn floating_tab(
	dragged: Option<TabDrag>,
	rects: &[Rect],
	viewport: Rect,
	x: f32,
) -> Option<(usize, Rect)> {
	let drag = dragged?;
	let rect = rects[drag.index];
	Some((
		drag.index,
		Rect {
			x: (x - drag.grab).clamp(
				viewport.x,
				(viewport.x + viewport.w - rect.w).max(viewport.x),
			),
			..rect
		},
	))
}

/// Whether hovering a tab's trailing edge brightens its close icon.
fn close_hovered(
	cursor_x: f32,
	rect: Rect,
	hovered: bool,
	dragging: bool,
) -> bool {
	hovered && !dragging && cursor_x >= rect.x + rect.w - 24.0
}

/// The hovered tab, letting a moving drag own the pointer over what it covers.
fn hovered_tab(
	rects: &[Rect],
	floating: Option<(usize, Rect)>,
	cursor: (f32, f32),
	active: usize,
) -> Option<usize> {
	let inside = |rect: &Rect| rect.contains(cursor.0, cursor.1);
	if let Some((index, rect)) = floating {
		return inside(&rect).then_some(index);
	}
	// The active tab is painted over the rest, so it answers first.
	if rects.get(active).is_some_and(inside) {
		return Some(active);
	}
	rects.iter().position(inside)
}

/// Divides every pair of neighboring inactive tabs whose bodies touch.
fn separators(
	rects: &[Rect],
	active: usize,
	dragged: Option<usize>,
	hovered: Option<usize>,
) -> Vec<Draw> {
	let order: Vec<_> = (0..rects.len())
		.filter(|index| {
			*index != active
				&& Some(*index) != dragged
				&& Some(*index) != hovered
		})
		.collect();
	order
		.windows(2)
		.filter_map(|pair| {
			let (left, right) = (pair[0], pair[1]);
			let edge = rects[left].x + rects[left].w;
			// A gap or overlap between painted neighbors has no boundary.
			((rects[right].x - edge - GAP).abs() < 1.0).then(|| {
				Draw::Rect(
					Rect {
						x: edge + GAP / 2.0 - 0.5,
						y: rects[left].y + 8.0,
						w: 1.0,
						h: rects[left].h - 16.0,
					},
					Paint::Styled(Condition::Toolbar, C::BorderColor),
				)
			})
		})
		.collect()
}

fn draw_tab_surface(
	out: &mut Vec<Draw>,
	rect: Rect,
	active: bool,
	hovered: bool,
) {
	let fill = if active {
		Paint::Background
	} else if hovered {
		Paint::Styled(Condition::Button, C::HoverBackground)
	} else {
		Paint::Styled(Condition::Toolbar, C::Background)
	};
	draw_connected(out, rect, fill, active);
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
