use super::super::Button;
use super::controls::{ICON_BUTTON, button_icon, panel_rect};
use super::icons;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	settings::ReaderSettings,
	state::{Command, InteractionState},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};
fn style_rows(rect: Rect) -> usize {
	((rect.h - 142.) / 60.).floor().max(1.) as usize
}
fn style_order(
	settings: &ReaderSettings,
	entries: &[crate::stylesheet::Entry],
) -> Vec<usize> {
	let mut indices: Vec<_> = (0..entries.len()).collect();
	indices.sort_by_key(|i| {
		settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &entries[*i].id))
			.unwrap_or(usize::MAX)
	});
	indices
}
pub(super) fn style_controls(
	settings: &ReaderSettings,
	entries: &[crate::stylesheet::Entry],
	page: usize,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let r = panel_rect(width, height);
	let rows = style_rows(r);
	let order = style_order(settings, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = vec![];
	for (label, icon, action, x, w) in [
		("Back", None, Command::Styles, 20., 58.),
		("System", None, Command::SystemTheme, 86., 74.),
		(
			"Close",
			Some(icons::CLOSE),
			Command::Settings,
			r.w - 20. - ICON_BUTTON,
			ICON_BUTTON,
		),
		("Open styles folder", None, Command::StylesFolder, 20., 146.),
	] {
		out.push(Button {
			label,
			icon,
			action,
			rect: Rect {
				x: r.x + x,
				y: if action == Command::StylesFolder {
					r.y + r.h - 38.
				} else {
					r.y + 16.
				},
				w,
				h: 28.,
			},
		});
	}
	if page > 0 {
		out.push(Button {
			label: "Previous",
			icon: None,
			action: Command::StylePrev,
			rect: Rect {
				x: r.x + r.w - 190.,
				y: r.y + r.h - 38.,
				w: 82.,
				h: 28.,
			},
		});
	}
	if (page + 1) * rows < order.len() {
		out.push(Button {
			label: "Next",
			icon: None,
			action: Command::StyleNext,
			rect: Rect {
				x: r.x + r.w - 100.,
				y: r.y + r.h - 38.,
				w: 80.,
				h: 28.,
			},
		});
	}
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos = settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
		if e.error.is_none() || pos.is_some() {
			out.push(Button {
				label: if pos.is_some() { "Disable" } else { "Enable" },
				icon: None,
				action: Command::StyleToggle(index),
				rect: Rect {
					x: r.x + r.w - 180.,
					y,
					w: 76.,
					h: 26.,
				},
			});
		}
		if let Some(pos) = pos {
			if pos > 0 {
				out.push(Button {
					label: "↑",
					icon: None,
					action: Command::StyleUp(index),
					rect: Rect {
						x: r.x + r.w - 96.,
						y,
						w: 32.,
						h: 26.,
					},
				});
			}
			if settings
				.style
				.as_ref()
				.is_some_and(|ids| pos + 1 < ids.len())
			{
				out.push(Button {
					label: "↓",
					icon: None,
					action: Command::StyleDown(index),
					rect: Rect {
						x: r.x + r.w - 58.,
						y,
						w: 32.,
						h: 26.,
					},
				});
			}
		}
	}
	out
}
pub(super) fn draw_styles(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
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
	let r = panel_rect(width, height);
	let rows = style_rows(r);
	let order = style_order(settings, entries);
	let page = page.min(order.len().saturating_sub(1) / rows);
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.,
				y: 0.,
				w: width,
				h: height,
			},
			Paint::Scrim,
		),
		Draw::Box {
			rect: r,
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			radius: 0.,
			border: 1.,
			left_only: false,
		},
	];
	let summary = if settings.style.is_none() {
		"Stylesheets · following system"
	} else {
		"Stylesheets · highest priority first"
	};
	out.extend(shaper.label(
		summary,
		13.,
		r.x + 20.,
		r.y + 66.,
		Paint::Styled(Condition::Panel, C::Color),
	));
	for (row, index) in
		order.into_iter().skip(page * rows).take(rows).enumerate()
	{
		let e = &entries[index];
		let pos = settings
			.style
			.as_ref()
			.and_then(|ids| ids.iter().position(|id| id == &e.id));
		let y = r.y + 84. + row as f32 * 60.;
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
				h: 26.,
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
		let detail = shaper.fit(detail, 10., r.w - 40.);
		out.extend(shaper.label(
			&detail,
			10.,
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
	for b in style_controls(settings, entries, page, width, height) {
		out.push(Draw::Box {
			rect: b.rect,
			chain: Condition::Button.chain(),
			condition: Condition::Button,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		let hovered =
			b.rect.contains(interaction.cursor.0, interaction.cursor.1);
		out.push(Draw::Rect(
			b.rect,
			Paint::Styled(
				Condition::Button,
				if interaction.pressed == Some(b.action) {
					C::ActiveBackground
				} else if hovered {
					C::HoverBackground
				} else {
					C::Background
				},
			),
		));
		if interaction.focus == Some(b.action) {
			for rect in [
				Rect { h: 1., ..b.rect },
				Rect {
					y: b.rect.y + b.rect.h - 1.,
					h: 1.,
					..b.rect
				},
				Rect { w: 1., ..b.rect },
				Rect {
					x: b.rect.x + b.rect.w - 1.,
					w: 1.,
					..b.rect
				},
			] {
				out.push(Draw::Rect(
					rect,
					Paint::Styled(Condition::Button, C::FocusColor),
				));
			}
		}
		if let Some(icon) = button_icon(&b) {
			out.push(icon);
		} else {
			let label_x =
				b.rect.x + (b.rect.w - shaper.text_width(b.label, 12.)) / 2.0;
			out.extend(shaper.label(
				b.label,
				12.,
				label_x,
				b.rect.y + 18.,
				Paint::Styled(Condition::Button, C::Color),
			));
		}
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
		let settings = ReaderSettings {
			style: Some(vec!["a".into()]),
			..Default::default()
		};
		for (w, h) in [(500., 300.), (820., 600.)] {
			let panel = panel_rect(w, h);
			let buttons = style_controls(&settings, &entries, 0, w, h);
			assert!(buttons.iter().all(|b| panel.contains(b.rect.x, b.rect.y)
				&& panel.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)));
			assert!(
				!buttons.iter().any(|b| b.action == Command::StyleToggle(1))
			);
		}
	}
}
