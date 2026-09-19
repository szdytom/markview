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
	count: usize,
) -> Rect {
	let mut rect = panel_rect(width, height);
	rect.h = rect.h.min(152.0 + count.max(1) as f32 * 60.0);
	rect.y = (height - rect.h) / 2.0;
	rect
}

fn style_rows(rect: Rect) -> usize {
	((rect.h - 142.) / 60.).floor().max(1.) as usize
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
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = styles_rect(width, height, entries.len());
	let rows = style_rows(r);
	let order = style_order(selected, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = vec![];
	let mut headers = vec![
		("Back", Some(icons::BACK), target.back(), r.w - 96., CONTROL),
		(
			"Close",
			Some(icons::CLOSE),
			Command::Settings,
			r.w - 24. - CONTROL,
			CONTROL,
		),
		(
			"Open styles folder",
			None,
			Command::StylesFolder,
			108.,
			146.,
		),
	];
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
		let y = r.y + 84. + row as f32 * 60.;
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
	let mut out = super::components::frame(r, width, height);
	let weight = shaper.appearance.weight;
	shaper.appearance.weight = 600;
	out.extend(shaper.label(
		"Stylesheets",
		20.0,
		r.x + 24.0,
		r.y + 36.0,
		Paint::Styled(Condition::Panel, C::Color),
	));
	shaper.appearance.weight = weight;
	for y in [r.y + 76.0, r.y + r.h - 64.0] {
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

	out.extend(shaper.label(
		target.summary(selected),
		12.,
		r.x + 20.,
		r.y + 62.,
		Paint::Styled(Condition::Panel, C::Color),
	));
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos =
			selected.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
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
	for b in style_controls(target, selected, entries, page, width, height) {
		out.extend(draw_button(shaper, interaction, &b, true));
	}
	out
}

#[cfg(test)]
mod stylesheet_tests {
	use super::*;
	#[test]
	fn stylesheet_controls_fit_and_cannot_enable_invalid_entries() {
		let entries = vec![
			crate::stylesheet::Entry {
				id: "a".into(),
				name: "A".into(),
				source: "test".into(),
				error: None,
			},
			crate::stylesheet::Entry {
				id: "broken".into(),
				name: "Broken".into(),
				source: "test".into(),
				error: Some("Invalid".into()),
			},
		];
		let selected = vec!["a".to_string()];
		for target in [StylesTarget::Reader, StylesTarget::Export] {
			for (w, h) in [(500., 300.), (820., 600.)] {
				let panel = panel_rect(w, h);
				let buttons =
					style_controls(target, Some(&selected), &entries, 0, w, h);
				assert!(buttons.iter().all(|b| {
					panel.contains(b.rect.x, b.rect.y)
						&& panel
							.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
				}));
				assert!(!buttons.iter().any(|b| matches!(
					b.action,
					Command::StyleToggle(1) | Command::ExportStyleToggle(1)
				)));
				let back =
					buttons.iter().find(|b| b.action == target.back()).unwrap();
				assert_eq!(back.label, "Back");
				assert!(back.icon.is_some());

				// Only the reader page offers the system theme.
				let system =
					buttons.iter().any(|b| b.action == Command::SystemTheme);
				assert_eq!(system, target == StylesTarget::Reader);
			}
		}
	}
}
