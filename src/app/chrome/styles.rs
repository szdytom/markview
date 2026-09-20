use super::super::Button;
use super::components::CONTROL;
use super::controls::{draw_button, panel_rect};
use super::icons;
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
	fn prev(self) -> Command {
		match self {
			Self::Reader => Command::StylePrev,
			Self::Export => Command::ExportStylePrev,
		}
	}
	fn next(self) -> Command {
		match self {
			Self::Reader => Command::StyleNext,
			Self::Export => Command::ExportStyleNext,
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

pub(in crate::app) fn styles_rect(
	width: f32,
	height: f32,
	_count: usize,
) -> Rect {
	panel_rect(width, height)
}

fn style_rows(rect: Rect) -> usize {
	// Reserve the footer and its separator below the last style row.
	((rect.h - 202.) / 60.).floor().max(1.) as usize
}

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

pub(super) fn style_controls(
	target: StylesTarget,
	selected: Option<&[String]>,
	entries: &[crate::stylesheet::Entry],
	page: usize,
	preview: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = styles_rect(width, height, entries.len());
	let rows = style_rows(r);
	let order = style_order(selected, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
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
	if page > 0 {
		out.push(Button {
			label: "←",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: target.prev(),
			rect: Rect {
				x: r.x + r.w - 96.,
				y: r.y + r.h - 48.,
				w: 32.,
				h: 32.,
			},
		});
	}
	if (page + 1) * rows < order.len() {
		out.push(Button {
			label: "→",
			icon: None,
			active: false,
			kind: Default::default(),
			enabled: true,
			action: target.next(),
			rect: Rect {
				x: r.x + r.w - 56.,
				y: r.y + r.h - 48.,
				w: 32.,
				h: 32.,
			},
		});
	}
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos =
			selected.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 132. + row as f32 * 60.;
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
					label: "↑",
					icon: None,
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
					label: "↓",
					icon: None,
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
	page: usize,
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
	let r = styles_rect(width, height, entries.len());
	let rows = style_rows(r);
	let order = style_order(selected, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
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
	for y in [r.y + 92.0, r.y + r.h - 64.0] {
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
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos =
			selected.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 132. + row as f32 * 60.;
		out.push(super::components::line(
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
			out.push(super::components::line(
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
		out.extend(shaper.label(
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
			out.push(Draw::Rect(
				rect,
				Paint::Styled(Condition::Button, C::Background),
			));
			out.extend(shaper.label(
				"Invalid",
				12.,
				rect.x + 7.,
				rect.y + 18.,
				Paint::Styled(Condition::Button, C::DisabledColor),
			));
		}
		let detail = e.error.as_deref().unwrap_or(&e.source);
		let detail = shaper.fit(detail, 12., r.w - 48.);
		out.extend(shaper.label(
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
	for b in
		style_controls(target, selected, entries, page, preview, width, height)
	{
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
				let buttons = style_controls(
					target,
					Some(&selected),
					&entries,
					0,
					false,
					w,
					h,
				);
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

	/// The page names itself through the settings tab row rather than a title.
	#[test]
	fn the_styles_page_is_reached_by_its_tab() {
		let (w, h) = (820., 600.);
		let tabs = super::super::components::tab_controls(
			styles_rect(w, h, 1),
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
		assert_eq!(styles_rect(820.0, 600.0, 1).h, generic.h);
		assert_eq!(styles_rect(820.0, 600.0, 20).h, generic.h);
	}
}
