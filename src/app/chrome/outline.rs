//! The table-of-contents drawer over the document's right edge.
//!
//! Geometry here owns painting and hit testing alike: the row a press finds is
//! the row the frame drew. The drawer is an overlay, not a panel, so the
//! document behind it keeps its scroll position and its selection.
use super::super::Button;
use super::components;
use crate::{
	lang::Lang,
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState, OutlineTree},
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
const DISCLOSURE: f32 = 18.0;
const HEADER_BUTTON: f32 = 28.0;
const HEADER_GAP: f32 = 4.0;

pub(super) fn header_buttons(drawer: Rect, lang: Lang) -> [Button; 2] {
	[
		(
			lang.outline_expand_all(),
			Command::OutlineExpandAll,
			super::icons::EXPAND_ALL,
		),
		(
			lang.outline_collapse_all(),
			Command::OutlineCollapseAll,
			super::icons::COLLAPSE_ALL,
		),
	]
	.map(|(label, action, icon)| {
		let offset = if action == Command::OutlineExpandAll {
			2.0 * HEADER_BUTTON + HEADER_GAP
		} else {
			HEADER_BUTTON
		};
		let mut button = components::button(
			label,
			action,
			Rect {
				x: drawer.x + drawer.w - INSET - offset,
				y: drawer.y + (HEADER - HEADER_BUTTON) / 2.0,
				w: HEADER_BUTTON,
				h: HEADER_BUTTON,
			},
		);
		button.icon = Some(icon);
		button.kind = components::ButtonKind::Quiet;
		button
	})
}

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

/// The disclosure target before clipping to the list viewport.
fn disclosure_rect(row: Rect, level: u8) -> Rect {
	Rect {
		x: row.x + INSET + f32::from(level.saturating_sub(1)) * LEVEL_INDENT,
		w: DISCLOSURE,
		..row
	}
}

/// The clickable entry rows, already clipped to the list viewport.
pub(super) fn buttons(
	drawer: Rect,
	entries: &[OutlineEntry],
	tree: &OutlineTree,
	scroll: f32,
	lang: Lang,
) -> Vec<Button> {
	let list = viewport(drawer);
	let rows = tree.rows(entries);
	let mut buttons = Vec::new();
	for row in visible(list, rows.len(), scroll) {
		let index = rows[row];
		let rect = row_rect(list, row, scroll);
		let disclosure = disclosure_rect(rect, entries[index].level);
		let mut heading = rect;
		if OutlineTree::has_children(entries, index) {
			if let Some(rect) = disclosure.intersect(list) {
				let mut button = components::button(
					if tree.is_collapsed(index) {
						lang.outline_expand()
					} else {
						lang.outline_collapse()
					},
					Command::OutlineToggle(index),
					rect,
				);
				button.kind = components::ButtonKind::Quiet;
				buttons.push(button);
			}
			heading.x = disclosure.x + disclosure.w;
			heading.w = (rect.x + rect.w - heading.x).max(0.0);
		}
		if let Some(rect) = heading.intersect(list) {
			let mut button = components::button(
				lang.outline_heading(),
				Command::OutlineGoto(index),
				rect,
			);
			button.kind = components::ButtonKind::Quiet;
			buttons.push(button);
		}
	}
	buttons
}

/// Puts the drawer's scroll and selection back inside the entries it shows.
///
/// Switching to a shorter document, or a reload that drops headings, leaves
/// the old offsets behind; a resize changes how much of the list fits. Left
/// alone, the visible range can be empty while headings exist, and Enter can
/// address a row that is gone.
pub(in crate::app) fn normalize(
	drawer: Rect,
	rows: &[usize],
	interaction: &mut InteractionState,
) {
	let max = max_scroll(drawer, rows.len());
	interaction.outline_scroll = interaction.outline_scroll.clamp(0.0, max);
	interaction.outline_selection = rows
		.iter()
		.copied()
		.take_while(|index| {
			*index <= interaction.outline_selection.unwrap_or(0)
		})
		.last()
		.or_else(|| rows.first().copied());
	if let Some(Command::OutlineGoto(index) | Command::OutlineToggle(index)) =
		interaction.focus
		&& !rows.contains(&index)
	{
		interaction.focus =
			interaction.outline_selection.map(Command::OutlineGoto);
	}
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
	tree: &OutlineTree,
	current: Option<usize>,
	drawer: Rect,
	lang: Lang,
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
		lang.outline_contents(),
		SIZE,
		Rect {
			x: drawer.x + INSET,
			y: drawer.y,
			w: (drawer.w - 2.0 * INSET - 2.0 * (HEADER_BUTTON + HEADER_GAP))
				.max(0.0),
			h: HEADER,
		},
		C::Muted,
	));
	ui.appearance.weight = weight;
	for button in header_buttons(drawer, lang) {
		out.extend(components::draw_button(ui, interaction, &button, true));
	}
	if entries.is_empty() {
		out.extend(components::label(
			ui,
			lang.outline_empty(),
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
	let rows = tree.rows(entries);
	let current = current.and_then(|index| {
		rows.iter().copied().take_while(|row| *row <= index).last()
	});
	for row in visible(list, rows.len(), interaction.outline_scroll) {
		let index = rows[row];
		let rect = row_rect(list, row, interaction.outline_scroll);
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
		let indent = INSET
			+ f32::from(entry.level.saturating_sub(1)) * LEVEL_INDENT
			+ DISCLOSURE;
		if OutlineTree::has_children(entries, index) {
			let disclosure = disclosure_rect(rect, entry.level);
			body.push(Draw::Polygon {
				center: [
					disclosure.x + DISCLOSURE / 2.0,
					disclosure.y + ROW / 2.0,
				],
				points: if tree.is_collapsed(index) {
					[[-2.5, -4.0], [2.5, 0.0], [-2.5, 4.0]].into()
				} else {
					[[-4.0, -2.5], [4.0, -2.5], [0.0, 2.5]].into()
				},
				paint: Paint::Styled(Condition::Panel, C::Muted),
			});
		}
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
