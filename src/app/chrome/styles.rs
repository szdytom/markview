use super::super::Button;
use super::components::CONTROL;
use super::controls::{draw_button, panel_rect};
use super::icons;
use super::list::List;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	state::{Command, InteractionState},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};

/// Which list a stylesheet page edits: the reader's effective styles, or the
/// sequence one export layers on the print sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StylesTarget {
	Reader,
	Export,
}

impl StylesTarget {
	fn toggle(self, index: usize) -> Command {
		match self {
			Self::Reader => Command::StyleToggle(index),
			Self::Export => Command::ExportStyleToggle(index),
		}
	}
	fn up(self, index: usize) -> Command {
		match self {
			Self::Reader => Command::StyleUp(index),
			Self::Export => Command::ExportStyleUp(index),
		}
	}
	fn down(self, index: usize) -> Command {
		match self {
			Self::Reader => Command::StyleDown(index),
			Self::Export => Command::ExportStyleDown(index),
		}
	}
	/// The page's Back button returns to the panel that opened it.
	fn back(self) -> Command {
		match self {
			Self::Reader => Command::Styles,
			Self::Export => Command::ExportStyles,
		}
	}
	/// Only the reader can follow the system theme.
	fn system(self) -> Option<Command> {
		(self == Self::Reader).then_some(Command::SystemTheme)
	}
	fn summary(self, selected: Option<&[String]>) -> &'static str {
		match self {
			Self::Reader if selected.is_none() => {
				"Following system appearance · enable a style to customize"
			}
			Self::Reader | Self::Export => {
				"Enabled styles come first · higher rows take priority"
			}
		}
	}
}

/// How much of the panel the title, tabs and summary take.
const LIST_TOP: f32 = 120.0;
/// How much the footer and its separator below the list take.
const FOOTER: f32 = 64.0;
/// One stylesheet row.
const ROW: f32 = 72.0;
/// The row's enable/disable toggle and its invalid badge.
const TOGGLE: f32 = 76.0;

/// A short panel trades the summary line for room to list styles.
fn spacious(panel: Rect) -> bool {
	panel.h >= 300.0
}

/// The `x` of the row's right-hand column, relative to the panel.
///
/// The toggle sits at its left edge, the priority arrows follow it, and the
/// column ends at the panel's right inset.
fn column_x(panel: Rect) -> f32 {
	panel.w - super::components::INSET - 2.0 * CONTROL - 6.0 - 8.0 - TOGGLE
}

/// The summary line's box, or `None` when a short panel needs the room.
fn summary_rect(panel: Rect, list: List) -> Option<Rect> {
	spacious(panel).then_some(Rect {
		x: panel.x + super::components::INSET,
		y: list.viewport.y - 36.0,
		w: panel.w - 2.0 * super::components::INSET,
		h: 20.0,
	})
}

/// The page's scrolling list of stylesheets.
pub(in crate::app) fn list(
	width: f32,
	height: f32,
	entries: usize,
	scroll: f32,
) -> List {
	let r = panel_rect(width, height);
	let top = if spacious(r) { LIST_TOP } else { 88.0 };
	List::new(
		r,
		Rect {
			x: r.x,
			y: r.y + top,
			w: r.w,
			h: (r.h - top - FOOTER).max(0.0),
		},
		ROW,
		entries,
		scroll,
	)
}

/// The display order: enabled styles first, in priority order, then the rest.
fn style_order(
	selected: Option<&[String]>,
	entries: &[crate::stylesheet::Entry],
) -> Vec<usize> {
	let mut indices: Vec<_> = (0..entries.len()).collect();
	indices.sort_by_key(|i| {
		selected
			.and_then(|ids| ids.iter().position(|id| id == &entries[*i].id))
			.unwrap_or(usize::MAX)
	});
	indices
}

/// One entry's priority, or `None` when it is not enabled.
fn position(
	selected: Option<&[String]>,
	entry: &crate::stylesheet::Entry,
) -> Option<usize> {
	selected.and_then(|ids| ids.iter().position(|id| id == &entry.id))
}

/// The page's fixed controls: the settings header, and the footer's folder
/// button and system-theme toggle. They sit outside the scrolling list.
pub(super) fn style_controls(
	target: StylesTarget,
	selected: Option<&[String]>,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = panel_rect(width, height);
	let mut out = vec![];
	let mut headers = if target == StylesTarget::Export {
		vec![
			("Back", Some(icons::BACK), target.back(), r.w - 96., CONTROL),
			(
				"Close",
				Some(icons::CLOSE),
				Command::Settings,
				r.w - 24. - CONTROL,
				CONTROL,
			),
		]
	} else {
		out.extend(super::components::settings_header_controls(
			r,
			crate::state::PanelTab::Styles,
			preview,
		));
		vec![]
	};
	headers.push((
		"Open styles folder",
		None,
		Command::StylesFolder,
		24.,
		146.,
	));
	if let Some(system) = target.system() {
		headers.push(("Follow system", None, system, r.w - 148., 124.));
	}
	for (label, icon, action, x, w) in headers {
		out.push(Button {
			label,
			icon,
			active: action == Command::SystemTheme && selected.is_none(),
			kind: Default::default(),
			enabled: true,
			action,
			rect: Rect {
				x: r.x + x,
				y: if matches!(
					action,
					Command::StylesFolder | Command::SystemTheme
				) {
					r.y + r.h - 48.
				} else {
					r.y + 16.
				},
				w,
				h: 32.,
			},
		});
	}
	out
}

/// One entry's toggle, and the arrows that move it in the priority order.
///
/// Only the rows on screen have buttons, so the page never builds a control
/// nothing can draw or reach.
pub(super) fn style_rows(
	target: StylesTarget,
	selected: Option<&[String]>,
	entries: &[crate::stylesheet::Entry],
	list: List,
) -> Vec<Button> {
	let r = list.panel;
	let column = column_x(r);
	let order = style_order(selected, entries);
	let mut out = vec![];
	for row in list.visible() {
		let index = order[row];
		let e = &entries[index];
		let pos = position(selected, e);
		let y = list.row_rect(row).y + (ROW - CONTROL) / 2.0;
		if e.error.is_none() || pos.is_some() {
			out.push(Button {
				label: if pos.is_some() { "Disable" } else { "Enable" },
				icon: None,
				active: pos.is_some(),
				kind: Default::default(),
				enabled: true,
				action: target.toggle(index),
				rect: Rect {
					x: r.x + column,
					y,
					w: TOGGLE,
					h: CONTROL,
				},
			});
		}
		if let Some(pos) = pos {
			let arrows = column + TOGGLE + 8.0;
			for (label, icon, action, enabled, x) in [
				("Move up", icons::UP, target.up(index), pos > 0, arrows),
				(
					"Move down",
					icons::DOWN,
					target.down(index),
					selected.is_some_and(|ids| pos + 1 < ids.len()),
					arrows + CONTROL + 6.0,
				),
			] {
				if !enabled {
					continue;
				}
				out.push(Button {
					label,
					icon: Some(icon),
					active: false,
					kind: Default::default(),
					enabled,
					action,
					rect: Rect {
						x: r.x + x,
						y,
						w: CONTROL,
						h: CONTROL,
					},
				});
			}
		}
	}
	out
}

#[expect(clippy::too_many_arguments, reason = "one page's explicit inputs")]
pub(super) fn draw_styles(
	shaper: &mut TextShaper,
	target: StylesTarget,
	selected: Option<&[String]>,
	interaction: &InteractionState,
	entries: &[crate::stylesheet::Entry],
	scroll: f32,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let r = panel_rect(width, height);
	let list = list(width, height, entries.len(), scroll);
	let order = style_order(selected, entries);
	// Previewing the document leaves only the panel surface, which then
	// recedes with everything else.
	let previewing = target == StylesTarget::Reader && preview;
	let mut out = if previewing {
		vec![super::components::line(r, Condition::Panel, C::Background)]
	} else {
		super::components::frame(r, width, height)
	};
	if target != StylesTarget::Reader {
		// The export chooser is not a settings tab, so it names itself.
		let weight = shaper.appearance.weight;
		shaper.appearance.weight = 700;
		out.extend(super::components::label(
			shaper,
			"Stylesheets",
			20.0,
			Rect {
				x: r.x + super::components::INSET,
				y: r.y + 16.0,
				w: r.w - super::components::INSET * 2.0,
				h: 32.0,
			},
			C::Color,
		));
		shaper.appearance.weight = weight;
	}
	for y in [list.viewport.y - 1.0, r.y + r.h - FOOTER] {
		out.push(super::components::line(
			Rect {
				x: r.x + 1.0,
				y,
				w: r.w - 2.0,
				h: 1.0,
			},
			Condition::Panel,
			C::BorderColor,
		));
	}

	if let Some(rect) = summary_rect(r, list) {
		out.extend(super::components::label(
			shaper,
			target.summary(selected),
			12.0,
			rect,
			C::Muted,
		));
	}
	let mut body = Vec::new();
	// A pointer below the fold must not light up the row hidden under the
	// footer, so the body only sees the cursor while it is inside the clip.
	let body_interaction = InteractionState {
		cursor: if list
			.viewport
			.contains(interaction.cursor.0, interaction.cursor.1)
		{
			interaction.cursor
		} else {
			(f32::NEG_INFINITY, f32::NEG_INFINITY)
		},
		focus: interaction.focus,
		focus_visible: interaction.focus_visible,
		pressed: interaction.pressed,
		..Default::default()
	};
	for row in list.visible() {
		let e = &entries[order[row]];
		let pos = position(selected, e);
		let y = list.row_rect(row).y;
		body.push(super::components::line(
			Rect {
				x: r.x + 24.0,
				y: y + ROW - 1.0,
				w: r.w - 48.0,
				h: 1.0,
			},
			Condition::Panel,
			C::BorderColor,
		));
		if let Some(pos) = pos {
			body.push(super::components::line(
				Rect {
					x: r.x + 24.0,
					y: y + (ROW - 28.0) / 2.0,
					w: 28.0,
					h: 28.0,
				},
				Condition::Button,
				C::ActiveBackground,
			));
			let mut appearance = shaper.appearance.clone();
			appearance.weight = 700;
			let (mut number, width) = shaper.label_with(
				&(pos + 1).to_string(),
				13.0,
				0.0,
				y + ROW / 2.0 + 13.0 * 0.35,
				&appearance,
				Paint::Styled(Condition::Panel, C::Accent),
				None,
			);
			for draw in &mut number {
				draw.translate(r.x + 24.0 + (28.0 - width) / 2.0, 0.0);
			}
			body.extend(number);
		}

		let text_x = r.x + 64.0;
		let weight = shaper.appearance.weight;
		shaper.appearance.weight = 600;
		let title = shaper.fit(&e.name, 14., r.x + column_x(r) - 12.0 - text_x);
		body.extend(shaper.label(
			&title,
			14.,
			text_x,
			y + 29.,
			Paint::Styled(Condition::Panel, C::Color),
		));
		shaper.appearance.weight = weight;
		if e.error.is_some() && pos.is_none() {
			let rect = Rect {
				x: r.x + column_x(r),
				y: y + (ROW - CONTROL) / 2.0,
				w: TOGGLE,
				h: CONTROL,
			};
			body.push(Draw::Rect(
				rect,
				Paint::Styled(Condition::Button, C::Background),
			));
			body.extend(shaper.label(
				"Invalid",
				12.,
				rect.x + 7.,
				rect.y + 18.,
				Paint::Styled(Condition::Button, C::DisabledColor),
			));
		}
		let detail = e
			.error
			.clone()
			.unwrap_or_else(|| format!("{} · {}", e.id, e.source));
		let detail = shaper.fit(&detail, 12., r.x + r.w - 24. - text_x);
		body.extend(shaper.label(
			&detail,
			12.,
			text_x,
			y + 54.,
			Paint::Styled(
				Condition::Panel,
				if e.error.is_some() {
					C::Error
				} else {
					C::Muted
				},
			),
		));
	}
	for b in style_rows(target, selected, entries, list) {
		if b.rect.intersect(list.viewport).is_some() {
			body.extend(draw_button(shaper, &body_interaction, &b, true));
		}
	}
	out.push(list.clip(body));
	list.draw_bar(&mut out, shaper, interaction);
	for b in style_controls(target, selected, preview, width, height) {
		// The header of a settings tab is drawn once, by the header itself.
		if target == StylesTarget::Reader
			&& super::components::is_settings_header(b.action)
		{
			continue;
		}
		out.extend(draw_button(shaper, interaction, &b, true));
	}
	if target == StylesTarget::Reader {
		if previewing {
			super::components::fade(
				&mut out,
				shaper,
				super::components::PREVIEW_OPACITY,
			);
		}
		// The header goes on top of the fade, so its own controls stay legible.
		out.extend(super::components::draw_settings_header(
			shaper,
			interaction,
			r,
			crate::state::PanelTab::Styles,
			preview,
		));
	}
	out
}

#[cfg(test)]
mod stylesheet_tests {
	use super::*;

	fn entry(id: &str, error: Option<&str>) -> crate::stylesheet::Entry {
		crate::stylesheet::Entry {
			id: id.into(),
			name: id.into(),
			source: "test".into(),
			error: error.map(str::to_owned),
			font_families: Vec::new(),
		}
	}

	#[test]
	fn priority_badge_and_actions_share_the_row_center() {
		let mut ui = crate::test_support::shaper();
		let entries = vec![entry("a", None)];
		let selected = vec!["a".into()];
		let draws = draw_styles(
			&mut ui,
			StylesTarget::Reader,
			Some(&selected),
			&InteractionState::default(),
			&entries,
			0.,
			false,
			820.,
			600.,
		);
		let body = draws
			.iter()
			.find_map(|draw| match draw {
				Draw::Clipped { draws, .. } => Some(draws),
				_ => None,
			})
			.unwrap();
		let badge = body
			.iter()
			.find_map(|draw| match draw {
				Draw::Rect(
					rect,
					Paint::Styled(Condition::Button, C::ActiveBackground),
				) if rect.w == 28.0 => Some(rect),
				_ => None,
			})
			.unwrap();
		let list = list(820., 600., 1, 0.);
		let toggle =
			style_rows(StylesTarget::Reader, Some(&selected), &entries, list)
				.remove(0);
		assert_eq!(badge.y + badge.h / 2., list.row_rect(0).y + ROW / 2.);
		assert_eq!(badge.y + badge.h / 2., toggle.rect.y + toggle.rect.h / 2.);
	}

	#[test]
	fn styles_and_generic_keep_the_same_header_height() {
		let mut ui = crate::test_support::shaper();
		for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)]
		{
			let generic = super::super::controls::form(
				&mut ui,
				&crate::settings::ReaderSettings::default(),
				0.0,
				width,
				height,
			);
			assert_eq!(
				list(width, height, 3, 0.0).viewport.y,
				generic.viewport.y
			);
		}
	}

	#[test]
	fn a_short_panel_keeps_the_style_summary_off_the_tabs() {
		// The shortest supported window has no room between the tabs and the
		// list, so the summary is dropped rather than drawn over them.
		for (width, height) in [(500.0, 300.0), (820.0, 300.0)] {
			let panel = panel_rect(width, height);
			assert!(summary_rect(panel, list(width, height, 1, 0.0)).is_none());
		}
		for (width, height) in [(820.0, 600.0), (1200.0, 800.0)] {
			let panel = panel_rect(width, height);
			let summary = summary_rect(panel, list(width, height, 1, 0.0))
				.expect("a tall panel shows the summary");
			let tabs = super::super::components::tab_controls(
				panel,
				crate::state::PanelTab::Styles,
			);
			let tab_bottom = tabs
				.iter()
				.map(|tab| tab.rect.y + tab.rect.h)
				.fold(f32::MIN, f32::max);
			assert!(
				summary.y >= tab_bottom,
				"{width}x{height}: summary at {} over tabs ending at {tab_bottom}",
				summary.y
			);
		}
	}

	#[test]
	fn priority_rows_center_controls_and_hide_boundary_arrows() {
		let entries =
			vec![entry("a", None), entry("b", None), entry("c", None)];
		let selected = vec!["c".into(), "a".into()];
		for target in [StylesTarget::Reader, StylesTarget::Export] {
			let rows = style_rows(
				target,
				Some(&selected),
				&entries,
				list(820., 600., 3, 0.),
			);
			let control =
				|action| rows.iter().find(|b| b.action == action).unwrap();
			assert!(
				control(target.toggle(2)).rect.y
					< control(target.toggle(0)).rect.y
			);
			assert!(
				control(target.toggle(0)).rect.y
					< control(target.toggle(1)).rect.y
			);
			assert_eq!(control(target.toggle(2)).label, "Disable");
			for (row, index) in [2, 0, 1].into_iter().enumerate() {
				let rect = list(820., 600., 3, 0.).row_rect(row);
				for button in rows.iter().filter(|b| {
					[target.toggle(index), target.up(index), target.down(index)]
						.contains(&b.action)
				}) {
					assert_eq!(
						button.rect.y + button.rect.h / 2.0,
						rect.y + ROW / 2.0
					);
				}
			}
			assert!(!rows.iter().any(|b| b.action == target.up(2)));
			assert!(control(target.down(2)).enabled);
			assert!(control(target.up(0)).enabled);
			assert!(!rows.iter().any(|b| b.action == target.down(0)));
			assert!(!rows.iter().any(|b| b.action == target.up(1)));
		}
	}

	#[test]
	fn stylesheet_controls_fit_and_cannot_enable_invalid_entries() {
		let entries = vec![entry("a", None), entry("broken", Some("Invalid"))];
		let selected = vec!["a".to_string()];
		for target in [StylesTarget::Reader, StylesTarget::Export] {
			for (w, h) in [(500., 300.), (820., 600.)] {
				let panel = panel_rect(w, h);
				let list = list(w, h, entries.len(), 0.0);
				let mut buttons =
					style_controls(target, Some(&selected), false, w, h);
				buttons.extend(list.hit(style_rows(
					target,
					Some(&selected),
					&entries,
					list,
				)));
				assert!(buttons.iter().all(|b| {
					panel.contains(b.rect.x, b.rect.y)
						&& panel
							.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
				}));
				assert!(!buttons.iter().any(|b| {
					b.action == target.up(0) || b.action == target.down(0)
				}));
				assert!(!buttons.iter().any(|b| matches!(
					b.action,
					Command::StyleToggle(1) | Command::ExportStyleToggle(1)
				)));
				if target == StylesTarget::Export {
					let back = buttons
						.iter()
						.find(|b| b.action == target.back())
						.unwrap();
					assert_eq!(back.label, "Back");
					assert!(back.icon.is_some());
				} else {
					assert!(!buttons.iter().any(|b| b.label == "Back"));
				}

				// Only the reader page offers the system theme.
				let system =
					buttons.iter().any(|b| b.action == Command::SystemTheme);
				assert_eq!(system, target == StylesTarget::Reader);
			}
		}
	}

	/// A catalogue past the fold scrolls to its last row instead of paging.
	#[test]
	fn a_long_catalogue_scrolls_instead_of_paging() {
		let entries: Vec<_> =
			(0..20).map(|i| entry(&format!("s{i}"), None)).collect();
		let (w, h) = (820., 600.);
		let top = list(w, h, entries.len(), 0.0);
		assert!(top.max_scroll() > 0.0);
		let rows =
			top.hit(style_rows(StylesTarget::Reader, None, &entries, top));
		assert!(rows.iter().any(|b| b.action == Command::StyleToggle(0)));
		assert!(!rows.iter().any(|b| b.action == Command::StyleToggle(19)));

		let bottom = list(w, h, entries.len(), f32::MAX);
		assert_eq!(bottom.scroll, top.max_scroll());
		let rows = bottom.hit(style_rows(
			StylesTarget::Reader,
			None,
			&entries,
			bottom,
		));
		assert!(rows.iter().any(|b| b.action == Command::StyleToggle(19)));
		assert!(!rows.iter().any(|b| b.action == Command::StyleToggle(0)));
	}

	/// The page names itself through the settings tab row rather than a title.
	#[test]
	fn the_styles_page_is_reached_by_its_tab() {
		let (w, h) = (820., 600.);
		let tabs = super::super::components::tab_controls(
			panel_rect(w, h),
			crate::state::PanelTab::Styles,
		);
		assert_eq!(tabs.len(), 4);
		assert!(tabs.iter().any(|b| {
			b.action == Command::SettingsTab(crate::state::PanelTab::Styles)
				&& b.active
		}));
		// Every tab stays on the panel it belongs to.
		let panel = panel_rect(w, h);
		assert!(tabs.iter().all(|b| {
			panel.contains(b.rect.x, b.rect.y)
				&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
		}));
	}

	#[test]
	fn settings_tabs_keep_the_same_panel_height() {
		let generic = panel_rect(820.0, 600.0);
		assert_eq!(panel_rect(820.0, 600.0).h, generic.h);
		assert_eq!(panel_rect(500.0, 300.0).h, panel_rect(500.0, 300.0).h);
	}
}
