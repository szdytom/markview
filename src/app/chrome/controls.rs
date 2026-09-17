use super::super::Button;
use crate::{
	layout::{Draw, Paint, Rect, TextShaper},
	settings::ReaderSettings,
	state::{Command, InteractionState},
};
use markview_core::style::{
	CjkType, ColorField as C, Condition, TextAppearance,
};
pub(in crate::app) fn panel_rect(width: f32, height: f32) -> Rect {
	let w = 540.0_f32.min((width - 32.0).max(0.0));
	let h = 480.0_f32.min((height - 32.0).max(0.0));
	Rect {
		x: (width - w) / 2.0,
		y: (height - h) / 2.0,
		w,
		h,
	}
}
fn row_geometry(rect: Rect) -> (f32, f32) {
	let top = if rect.h < 360.0 { 60.0 } else { 94.0 };
	(top, (rect.h - top - 48.0) / 8.0)
}
pub(super) fn controls(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	panel_open: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	if panel_open {
		let rect = panel_rect(width, height);
		let (top, row) = row_geometry(rect);
		let close_width = button_width(shaper, "Close", 13.0);
		let mut buttons = vec![Button {
			label: "Close",
			action: Command::Settings,
			rect: Rect {
				x: rect.x + rect.w - 20.0 - close_width,
				y: rect.y + 16.0,
				w: close_width,
				h: 28.0,
			},
		}];
		for (i, entries) in [
			vec![
				("System", Command::SystemTheme),
				("Styles…", Command::Styles),
			],
			vec![("A−", Command::Smaller), ("A+", Command::Larger)],
			vec![("W−", Command::Narrower), ("W+", Command::Wider)],
			vec![(
				if settings.justify {
					"Justified"
				} else {
					"Left aligned"
				},
				Command::Align,
			)],
			vec![(
				if settings.hyphenate { "On" } else { "Off" },
				Command::Hyphens,
			)],
			vec![(
				if settings.codeblock_wrap { "On" } else { "Off" },
				Command::CodeWrap,
			)],
			vec![
				("Off", Command::Indent(0)),
				("1 em", Command::Indent(1)),
				("2 em", Command::Indent(2)),
				("3 em", Command::Indent(3)),
			],
			vec![
				("SC", Command::CjkType(CjkType::Sc)),
				("TC", Command::CjkType(CjkType::Tc)),
				("JP", Command::CjkType(CjkType::Jp)),
				("none", Command::CjkType(CjkType::None)),
			],
		]
		.into_iter()
		.enumerate()
		{
			let count = entries.len();
			let button_width =
				(168.0 - 4.0 * (count - 1) as f32) / count as f32;
			for (j, (label, action)) in entries.into_iter().enumerate() {
				buttons.push(Button {
					label,
					action,
					rect: Rect {
						x: rect.x + rect.w - 20.0 - 168.0
							+ j as f32 * (button_width + 4.0),
						y: rect.y + top + i as f32 * row,
						w: button_width,
						h: (row - 4.0).min(32.0),
					},
				});
			}
		}
		let open_config_width =
			button_width(shaper, "Open settings.toml", 13.0);
		let reset_width = button_width(shaper, "Reset defaults", 13.0);
		for (label, action, x, w) in [
			(
				"Open settings.toml",
				Command::OpenConfig,
				20.0,
				open_config_width,
			),
			(
				"Reset defaults",
				Command::Reset,
				rect.w - 20.0 - reset_width,
				reset_width,
			),
		] {
			buttons.push(Button {
				label,
				action,
				rect: Rect {
					x: rect.x + x,
					y: rect.y + rect.h - 38.0,
					w,
					h: 28.0,
				},
			});
		}
		return buttons;
	}
	toolbar_controls(shaper, width)
}

fn button_width(shaper: &mut TextShaper, label: &str, size: f32) -> f32 {
	const HORIZONTAL_PADDING: f32 = 18.0;
	let old_appearance = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let width = shaper.text_width(label, size) + HORIZONTAL_PADDING;
	shaper.appearance = old_appearance;
	width
}

pub(super) fn toolbar_controls(
	shaper: &mut TextShaper,
	width: f32,
) -> Vec<Button> {
	const TEXT_SIZE: f32 = 13.0;
	const HORIZONTAL_PADDING: f32 = 18.0;
	const GAP: f32 = 4.0;
	let entries = [("Open", Command::Open), ("Settings", Command::Settings)];
	let old_appearance = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let widths: Vec<f32> = entries
		.iter()
		.map(|(label, _)| {
			shaper.text_width(label, TEXT_SIZE) + HORIZONTAL_PADDING
		})
		.collect();
	shaper.appearance = old_appearance;
	let mut x = toolbar_right_edge(shaper, width);
	entries
		.into_iter()
		.zip(widths)
		.map(|((label, action), w)| {
			let rect = Rect {
				x,
				y: 6.0,
				w,
				h: 28.0,
			};
			x += w + GAP;
			Button {
				rect,
				label,
				action,
			}
		})
		.collect()
}

pub(super) fn toolbar_right_edge(shaper: &mut TextShaper, width: f32) -> f32 {
	const TEXT_SIZE: f32 = 13.0;
	const HORIZONTAL_PADDING: f32 = 18.0;
	const GAP: f32 = 4.0;
	let old_appearance = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let button_widths = ["Open", "Settings"]
		.into_iter()
		.map(|label| shaper.text_width(label, TEXT_SIZE) + HORIZONTAL_PADDING)
		.collect::<Vec<_>>();
	shaper.appearance = old_appearance;
	width - button_widths.iter().sum::<f32>() - GAP - 16.0
}

pub(super) fn draw_controls(
	shaper: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let mut out = Vec::new();
	if interaction.panel_open {
		let rect = panel_rect(width, height);
		out.push(Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: width,
				h: height,
			},
			Paint::Scrim,
		));
		out.push(Draw::Rect(
			Rect {
				x: rect.x - 5.0,
				y: rect.y + 6.0,
				w: rect.w + 10.0,
				h: rect.h + 4.0,
			},
			Paint::Shadow,
		));
		out.push(Draw::Box {
			rect,
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		out.push(Draw::Rect(
			Rect {
				x: rect.x,
				y: rect.y,
				w: 3.0,
				h: rect.h,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		));
		let x = rect.x + 20.0;
		out.extend(shaper.label(
			"Reading settings",
			22.0,
			x,
			rect.y + 36.0,
			Paint::Styled(Condition::Panel, C::Color),
		));
		if rect.h >= 360.0 {
			out.extend(shaper.label(
				"Saved automatically · file changes apply live",
				12.0,
				x,
				rect.y + 61.0,
				Paint::Styled(Condition::Panel, C::Muted),
			));
		}
		let (top, row) = row_geometry(rect);
		for (i, label) in [
			format!(
				"Styles · {}",
				settings
					.style
					.as_ref()
					.map(|ids| if ids.is_empty() {
						"Light base".into()
					} else {
						ids.join(", ")
					})
					.unwrap_or_else(|| "System".into())
			),
			format!("Text size · {:.1} px", settings.font_size),
			format!("Column width · {:.1} px", settings.width),
			"Alignment".into(),
			"English hyphenation".into(),
			"Code block wrapping".into(),
			format!(
				"Paragraph indent · {}",
				if settings.paragraph_indent > 0.0 {
					format!("{} em", settings.paragraph_indent)
				} else {
					"off".into()
				}
			),
			format!("CJK type · {:?}", settings.cjk_type),
		]
		.iter()
		.enumerate()
		{
			let label = shaper.fit(label, 13., rect.w - 218.);
			out.extend(shaper.label(
				&label,
				13.0,
				x,
				rect.y + top + i as f32 * row + 19.0,
				Paint::Styled(Condition::Panel, C::Color),
			));
		}
	}
	let buttons = if interaction.panel_open {
		controls(shaper, settings, true, width, height)
	} else {
		toolbar_controls(shaper, width)
	};
	for b in buttons {
		out.push(Draw::Box {
			rect: b.rect,
			chain: Condition::Button.chain(),
			condition: Condition::Button,
			radius: 0.,
			border: 1.,
			left_only: false,
		});
		if interaction.focus == Some(b.action) {
			out.push(Draw::Rect(
				b.rect,
				Paint::Styled(Condition::Button, C::FocusColor),
			));
			out.push(Draw::Rect(
				Rect {
					x: b.rect.x + 1.0,
					y: b.rect.y + 1.0,
					w: b.rect.w - 2.0,
					h: b.rect.h - 2.0,
				},
				Paint::Styled(
					Condition::Button,
					if interaction.pressed == Some(b.action) {
						C::ActiveBackground
					} else {
						C::Background
					},
				),
			));
		} else if b.rect.contains(interaction.cursor.0, interaction.cursor.1) {
			out.push(Draw::Rect(
				b.rect,
				Paint::Styled(Condition::Button, C::HoverBackground),
			));
		} else if interaction.panel_open {
			out.push(Draw::Rect(
				b.rect,
				Paint::Styled(Condition::Button, C::Background),
			));
		}
		let label_x =
			b.rect.x + (b.rect.w - shaper.text_width(b.label, 13.0)) / 2.0;
		out.extend(shaper.label(
			b.label,
			13.0,
			label_x,
			b.rect.y + b.rect.h / 2.0 + 5.0,
			Paint::Styled(Condition::Button, C::Color),
		));
	}
	out
}
#[cfg(test)]
mod tests {
	use super::*;
	use crate::app::TOP;
	#[test]
	fn panel_exposes_first_line_indent_presets() {
		let mut shaper = TextShaper::new();
		let buttons = controls(
			&mut shaper,
			&ReaderSettings::default(),
			true,
			1200.0,
			800.0,
		);
		for (em, label) in [(0, "Off"), (1, "1 em"), (2, "2 em"), (3, "3 em")] {
			let button = buttons
				.iter()
				.find(|b| b.action == Command::Indent(em))
				.expect("indent preset");
			assert_eq!(button.label, label);
		}
	}
	#[test]
	fn panel_toggles_codeblock_wrapping() {
		let mut shaper = TextShaper::new();
		let mut label = |wrap| {
			controls(
				&mut shaper,
				&ReaderSettings {
					codeblock_wrap: wrap,
					..Default::default()
				},
				true,
				1200.0,
				800.0,
			)
			.into_iter()
			.find(|b| b.action == Command::CodeWrap)
			.expect("wrap toggle")
			.label
		};
		assert_eq!(label(false), "Off");
		assert_eq!(label(true), "On");
	}
	#[test]
	fn controls_fit_minimum_window_and_panel_focus_has_no_document_actions() {
		for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)]
		{
			let mut shaper = TextShaper::new();
			let panel = panel_rect(width, height);
			for button in controls(
				&mut shaper,
				&ReaderSettings::default(),
				true,
				width,
				height,
			) {
				assert!(panel.contains(button.rect.x, button.rect.y));
				assert!(panel.contains(
					button.rect.x + button.rect.w,
					button.rect.y + button.rect.h
				));
				assert_ne!(button.action, Command::Open);
			}
			for button in controls(
				&mut shaper,
				&ReaderSettings::default(),
				false,
				width,
				height,
			) {
				assert!(button.rect.x + button.rect.w <= width);
			}
			let toolbar = controls(
				&mut shaper,
				&ReaderSettings::default(),
				false,
				width,
				height,
			);
			assert_eq!(
				toolbar.iter().map(|b| b.action).collect::<Vec<_>>(),
				vec![Command::Open, Command::Settings]
			);
			assert_eq!(toolbar[1].rect.x + toolbar[1].rect.w, width - 16.0);
			assert!(toolbar.iter().all(|b| b.rect.y + b.rect.h < TOP));
		}
	}
}
