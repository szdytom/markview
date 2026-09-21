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
				"Following the system appearance"
			}
			Self::Reader => "Enabled styles appear first, in priority order",
			Self::Export => "Applied to the exported document",
		}
	}
}

/// How much of the panel the title, tabs and summary take.
const LIST_TOP: f32 = 132.0;
/// How much the footer and its separator below the list take.
const FOOTER: f32 = 64.0;
/// One stylesheet row.
const ROW: f32 = 60.0;

/// The page's scrolling list of stylesheets.
pub(in crate::app) fn list(
	width: f32,
	height: f32,
	entries: usize,
	scroll: f32,
) -> List {
	let r = panel_rect(width, height);
	List::new(
		r,
		Rect {
			x: r.x,
			y: r.y + LIST_TOP,
			w: r.w,
			h: (r.h - LIST_TOP - FOOTER).max(0.0),
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
		108.,
		146.,
	));
	if let Some(system) = target.system() {
		headers.push(("System", None, system, 24., 74.));
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
	let order = style_order(selected, entries);
	let mut out = vec![];
	for row in list.visible() {
		let index = order[row];
		let e = &entries[index];
		let pos = position(selected, e);
		let y = list.row_rect(row).y;
		if e.error.is_none() || pos.is_some() {
			out.push(Button {
				label: if pos.is_some() { "Enabled" } else { "Enable" },
				icon: None,
				active: pos.is_some(),
				kind: Default::default(),
				enabled: true,
				action: target.toggle(index),
				rect: Rect {
					x: r.x + r.w - 180.,
					y,
					w: 76.,
					h: 32.,
				},
			});
		}
		if let Some(pos) = pos {
			if pos > 0 {
				out.push(Button {
					label: "Move up",
					icon: Some(icons::UP),
					active: false,
					kind: Default::default(),
					enabled: true,
					action: target.up(index),
					rect: Rect {
						x: r.x + r.w - 96.,
						y,
						w: 32.,
						h: 32.,
					},
				});
			}
			if selected.is_some_and(|ids| pos + 1 < ids.len()) {
				out.push(Button {
					label: "Move down",
					icon: Some(icons::DOWN),
					active: false,
					kind: Default::default(),
					enabled: true,
					action: target.down(index),
					rect: Rect {
						x: r.x + r.w - 58.,
						y,
						w: 32.,
						h: 32.,
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
	for y in [r.y + 92.0, r.y + r.h - FOOTER] {
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

	let summary = shaper.fit(target.summary(selected), 12., r.w - 40.);
	out.extend(shaper.label(
		&summary,
		12.,
		r.x + 20.,
		r.y + 108.,
		Paint::Styled(Condition::Panel, C::Color),
	));
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
				x: r.x + 20.0,
				y: y + 53.0,
				w: r.w - 40.0,
				h: 1.0,
			},
			Condition::Panel,
			C::BorderColor,
		));
		if pos.is_some() {
			body.push(super::components::line(
				Rect {
					x: r.x + 8.0,
					y: y + 5.0,
					w: 2.0,
					h: 38.0,
				},
				Condition::Panel,
				C::Accent,
			));
		}

		let title = format!(
			"{}{} ({})",
			pos.map(|p| format!("{}. ", p + 1)).unwrap_or_default(),
			e.name,
			e.id
		);
		let title = shaper.fit(&title, 13., r.w - 212.);
		body.extend(shaper.label(
			&title,
			13.,
			r.x + 20.,
			y + 18.,
			Paint::Styled(Condition::Panel, C::Color),
		));
		if e.error.is_some() && pos.is_none() {
			let rect = Rect {
				x: r.x + r.w - 180.,
				y,
				w: 76.,
				h: 32.,
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
		let detail = e.error.as_deref().unwrap_or(&e.source);
		let detail = shaper.fit(detail, 12., r.w - 48.);
		body.extend(shaper.label(
			&detail,
			12.,
			r.x + 20.,
			y + 40.,
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
		assert_eq!(tabs.len(), 3);
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
