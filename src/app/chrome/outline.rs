//! The table-of-contents drawer over the document's right edge.
//!
//! Geometry here owns painting and hit testing alike: the row a press finds is
//! the row the frame drew. The drawer is an overlay, not a panel, so the
//! document behind it keeps its scroll position and its selection.
use super::super::Button;
use super::components;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState},
};
use markview_core::document::OutlineEntry;
use markview_core::style::{ColorField as C, Condition};

/// The drawer's width, flush with the right window edge.
pub(in crate::app) const WIDTH: f32 = 300.0;
/// The title band above the entry list.
const HEADER: f32 = 40.0;
/// One entry row.
const ROW: f32 = 26.0;
/// Inset of a first-level entry from the drawer's left edge.
const INSET: f32 = 14.0;
/// Extra indent for each heading level below the first.
const LEVEL_INDENT: f32 = 12.0;
const SIZE: f32 = 13.0;

/// The drawer's rectangle, from `top` down to just above the footer.
pub(in crate::app) fn rect(width: f32, height: f32, top: f32) -> Rect {
	let w = WIDTH.min(width);
	Rect {
		x: width - w,
		y: top,
		w,
		h: (height - top - super::super::BOTTOM).max(0.0),
	}
}

/// The entry list below the title.
fn viewport(drawer: Rect) -> Rect {
	Rect {
		x: drawer.x,
		y: drawer.y + HEADER,
		w: drawer.w,
		h: (drawer.h - HEADER).max(0.0),
	}
}

/// How far the entry list may scroll.
pub(in crate::app) fn max_scroll(drawer: Rect, entries: usize) -> f32 {
	(entries as f32 * ROW - viewport(drawer).h).max(0.0)
}

/// The row one entry occupies at `scroll`, in window coordinates.
fn row_rect(list: Rect, index: usize, scroll: f32) -> Rect {
	Rect {
		x: list.x,
		y: list.y + index as f32 * ROW - scroll,
		w: list.w,
		h: ROW,
	}
}

/// The entries intersecting the list viewport at `scroll`.
fn visible(list: Rect, entries: usize, scroll: f32) -> std::ops::Range<usize> {
	if entries == 0 || list.h <= 0.0 {
		return 0..0;
	}
	let first = (scroll / ROW).floor().max(0.0) as usize;
	let last = ((((scroll + list.h) / ROW).ceil() as usize) + 1).min(entries);
	first.min(entries)..last
}

/// The scroll offset that brings `index` inside the list.
pub(in crate::app) fn reveal(
	drawer: Rect,
	entries: usize,
	scroll: f32,
	index: usize,
) -> f32 {
	let list = viewport(drawer);
	let top = index as f32 * ROW;
	let max = max_scroll(drawer, entries);
	if top < scroll {
		top.clamp(0.0, max)
	} else if top + ROW > scroll + list.h {
		(top + ROW - list.h).clamp(0.0, max)
	} else {
		scroll.clamp(0.0, max)
	}
}

/// The clickable entry rows, already clipped to the list viewport.
pub(super) fn buttons(
	drawer: Rect,
	entries: usize,
	scroll: f32,
) -> Vec<Button> {
	let list = viewport(drawer);
	visible(list, entries, scroll)
		.filter_map(|index| {
			// A partly visible row is clickable only where it is drawn, so a
			// press in the header or below the list cannot reach a hidden row.
			let rect = row_rect(list, index, scroll).intersect(list)?;
			let mut b = components::button(
				"Heading",
				Command::OutlineGoto(index),
				rect,
			);
			b.kind = components::ButtonKind::Quiet;
			Some(b)
		})
		.collect()
}

/// Puts the drawer's scroll and selection back inside the entries it shows.
///
/// Switching to a shorter document, or a reload that drops headings, leaves
/// the old offsets behind; a resize changes how much of the list fits. Left
/// alone, the visible range can be empty while headings exist, and Enter can
/// address a row that is gone.
pub(in crate::app) fn normalize(
	drawer: Rect,
	entries: usize,
	interaction: &mut InteractionState,
) {
	let max = max_scroll(drawer, entries);
	interaction.outline_scroll = interaction.outline_scroll.clamp(0.0, max);
	interaction.outline_selection = if entries == 0 {
		None
	} else {
		Some(interaction.outline_selection.unwrap_or(0).min(entries - 1))
	};
}

/// Draws the drawer: its frame, the title, and the entries currently visible.
///
/// `current` is the entry holding the reading position; it is filled with the
/// button condition's active background. The keyboard selection gets an accent
/// bar, so the two never have to be confused.
pub(super) fn draw(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	entries: &[OutlineEntry],
	current: Option<usize>,
	drawer: Rect,
) -> Vec<Draw> {
	components::appearance(ui);
	let list = viewport(drawer);
	let mut out = vec![
		Draw::Rect(drawer, Paint::Styled(Condition::Panel, C::Background)),
		Draw::Rect(
			Rect {
				x: drawer.x,
				y: drawer.y,
				w: 1.0,
				h: drawer.h,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		),
	];
	let weight = ui.appearance.weight;
	ui.appearance.weight = 700;
	out.extend(components::label(
		ui,
		"Contents",
		SIZE,
		Rect {
			x: drawer.x + INSET,
			y: drawer.y,
			w: (drawer.w - 2.0 * INSET).max(0.0),
			h: HEADER,
		},
		C::Muted,
	));
	ui.appearance.weight = weight;
	if entries.is_empty() {
		out.extend(components::label(
			ui,
			"No headings",
			12.0,
			Rect {
				x: drawer.x + INSET,
				y: list.y + 8.0,
				w: (drawer.w - 2.0 * INSET).max(0.0),
				h: ROW,
			},
			C::Muted,
		));
		return out;
	}
	let mut body = Vec::new();
	for index in visible(list, entries.len(), interaction.outline_scroll) {
		let rect = row_rect(list, index, interaction.outline_scroll);
		let entry = &entries[index];
		let hovered = rect.contains(interaction.cursor.0, interaction.cursor.1);
		let pressed =
			hovered && interaction.pressed == Some(Command::OutlineGoto(index));
		let active = current == Some(index);
		if active {
			body.push(Draw::Rect(
				rect,
				Paint::Styled(Condition::Button, C::ActiveBackground),
			));
		} else if pressed || hovered {
			body.push(Draw::Rect(
				rect,
				Paint::Styled(Condition::Button, C::HoverBackground),
			));
		}
		if interaction.outline_selection == Some(index) {
			body.push(Draw::Rect(
				Rect {
					x: rect.x,
					y: rect.y + 3.0,
					w: 2.0,
					h: rect.h - 6.0,
				},
				Paint::Styled(Condition::Button, C::Accent),
			));
		}
		let indent =
			INSET + f32::from(entry.level.saturating_sub(1)) * LEVEL_INDENT;
		let text =
			ui.fit(&entry.text, SIZE, (rect.w - indent - INSET).max(0.0));
		body.extend(ui.label(
			&text,
			SIZE,
			rect.x + indent,
			rect.y + ROW / 2.0 + 4.5,
			Paint::Styled(
				Condition::Panel,
				if active { C::Color } else { C::Muted },
			),
		));
	}
	out.push(Draw::Clipped {
		rect: list,
		draws: body,
	});
	out
}

#[cfg(test)]
#[path = "outline_tests.rs"]
mod tests;
