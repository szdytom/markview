//! The export panel: the format, the paper, and the typography the export
//! writes with. None of it is the reading view's own settings.
use super::super::Button;
use super::controls::{ICON_BUTTON, button_width, draw_button, panel_rect};
use super::icons;
use crate::{
	app::export::{MARGIN_PRESETS, PAPER_PRESETS, SCALE_PRESETS},
	export,
	layout::{Draw, Paint, Rect, TextShaper},
	settings::{ExportFormat, ExportSettings},
	state::{Command, InteractionState},
};
use markview_core::style::{ColorField as C, Condition, TextAppearance};

/// Width of the right-aligned button column.
const BUTTONS: f32 = 240.0;
/// Gap between buttons in a row.
const GAP: f32 = 4.0;

/// One labelled row of the panel.
struct Row {
	label: String,
	actions: Vec<Action>,
}

/// One choice inside a row.
struct Action {
	label: &'static str,
	action: Command,
	active: bool,
}

fn action(label: &'static str, active: bool, action: Command) -> Action {
	Action {
		label,
		action,
		active,
	}
}

fn quote(em: u8) -> &'static str {
	match em {
		0 => "Off",
		1 => "1 em",
		2 => "2 em",
		_ => "3 em",
	}
}

fn indent_label(value: f32) -> String {
	if value > 0.0 {
		format!("{value} em")
	} else {
		"off".into()
	}
}

/// The panel's contents, in the order it draws them.
fn rows(settings: &ExportSettings) -> Vec<Row> {
	let mut out = vec![
		Row {
			label: format!(
				"Styles · {}",
				if settings.style.is_empty() {
					"none".to_owned()
				} else {
					settings.style.join(", ")
				}
			),
			actions: vec![Action {
				label: "Styles…",
				action: Command::ExportStyles,
				active: false,
			}],
		},
		Row {
			label: format!(
				"Format · {}",
				if settings.format == ExportFormat::Pdf {
					"PDF"
				} else {
					"PNG"
				}
			),
			actions: vec![
				action(
					"PDF",
					settings.format == ExportFormat::Pdf,
					Command::ExportFormat(ExportFormat::Pdf),
				),
				action(
					"PNG",
					settings.format == ExportFormat::Png,
					Command::ExportFormat(ExportFormat::Png),
				),
			],
		},
		Row {
			label: format!("Text size · {} px", settings.font_size),
			actions: vec![
				Action {
					label: "A−",
					action: Command::ExportSize(-1),
					active: false,
				},
				Action {
					label: "A+",
					action: Command::ExportSize(1),
					active: false,
				},
			],
		},
		Row {
			label: format!(
				"Paragraph indent · {}",
				indent_label(settings.paragraph_indent)
			),
			actions: (0..4u8)
				.map(|em| {
					action(
						quote(em),
						(settings.paragraph_indent - f32::from(em)).abs()
							< 0.01,
						Command::ExportIndent(em),
					)
				})
				.collect(),
		},
		Row {
			label: format!("Paper · {}", settings.paper),
			actions: PAPER_PRESETS
				.iter()
				.enumerate()
				.map(|(index, (label, paper))| {
					action(
						label,
						settings.paper.eq_ignore_ascii_case(paper),
						Command::ExportPaper(index as u8),
					)
				})
				.collect(),
		},
		Row {
			label: format!(
				"Orientation · {}",
				if settings.landscape {
					"landscape"
				} else {
					"portrait"
				}
			),
			actions: vec![
				action(
					"Portrait",
					!settings.landscape,
					Command::ExportOrientation(false),
				),
				action(
					"Landscape",
					settings.landscape,
					Command::ExportOrientation(true),
				),
			],
		},
		Row {
			label: format!(
				"Margins · {} mm",
				MARGIN_PRESETS
					.iter()
					.find(|(margin, _)| *margin == settings.margin)
					.map_or_else(
						|| {
							format!(
								"{:.0}/{:.0}",
								settings.margin[0], settings.margin[1]
							)
						},
						|(_, label)| (*label).to_owned(),
					)
			),
			actions: MARGIN_PRESETS
				.iter()
				.enumerate()
				.map(|(index, (margin, label))| {
					action(
						label,
						settings.margin == *margin,
						Command::ExportMargin(index as u8),
					)
				})
				.collect(),
		},
	];
	if settings.format == ExportFormat::Png {
		out.push(Row {
			label: format!("PNG scale · {}×", settings.scale),
			actions: SCALE_PRESETS
				.iter()
				.enumerate()
				.map(|(index, (scale, label))| {
					action(
						label,
						(settings.scale - scale).abs() < 1e-3,
						Command::ExportScale(index as u8),
					)
				})
				.collect(),
		});
	}
	out
}

/// Where one row's label and buttons sit.
struct Placed<'a> {
	row: &'a Row,
	label: Rect,
	buttons: Vec<Rect>,
}

fn row_geometry(rect: Rect, rows: usize) -> (f32, f32) {
	let top = if rect.h < 360.0 { 52.0 } else { 94.0 };
	(top, (rect.h - top - 48.0) / rows.max(1) as f32)
}

fn place(rect: Rect, rows: &[Row]) -> Vec<Placed<'_>> {
	let (top, height) = row_geometry(rect, rows.len());
	let button_height = (height - 4.0).min(32.0);
	let right = rect.x + rect.w - 20.0;
	rows.iter()
		.enumerate()
		.map(|(index, row)| {
			let y = rect.y + top + index as f32 * height;
			let count = row.actions.len().max(1) as f32;
			let width = (BUTTONS - GAP * (count - 1.0)) / count;
			let buttons = (0..row.actions.len())
				.map(|slot| Rect {
					x: right - BUTTONS + slot as f32 * (width + GAP),
					y,
					w: width,
					h: button_height,
				})
				.collect();
			Placed {
				row,
				label: Rect {
					x: rect.x + 20.0,
					y,
					w: (right - BUTTONS - 12.0) - (rect.x + 20.0),
					h: button_height,
				},
				buttons,
			}
		})
		.collect()
}

pub(super) fn export_controls(
	shaper: &mut TextShaper,
	settings: &ExportSettings,
	width: f32,
	height: f32,
) -> Vec<Button> {
	let rect = panel_rect(width, height);
	let rows = rows(settings);
	let mut out = vec![Button {
		label: "Close",
		icon: Some(icons::CLOSE),
		active: false,
		action: Command::Export,
		rect: Rect {
			x: rect.x + rect.w - 20.0 - ICON_BUTTON,
			y: rect.y + 16.0,
			w: ICON_BUTTON,
			h: 28.0,
		},
	}];
	for placed in place(rect, &rows) {
		for (action, rect) in placed.row.actions.iter().zip(placed.buttons) {
			out.push(Button {
				label: action.label,
				icon: None,
				active: action.active,
				action: action.action,
				rect,
			});
		}
	}
	let run_width = button_width(shaper, "Export…", 13.0);
	let watch_width = button_width(shaper, "Export and Watch…", 13.0);
	let right = rect.x + rect.w - 20.0;
	let y = rect.y + rect.h - 38.0;
	out.push(Button {
		label: "Export and Watch…",
		icon: None,
		active: false,
		action: Command::ExportAndWatch,
		rect: Rect {
			x: right - run_width - GAP - watch_width,
			y,
			w: watch_width,
			h: 28.0,
		},
	});
	out.push(Button {
		label: "Export…",
		icon: None,
		active: false,
		action: Command::ExportRun,
		rect: Rect {
			x: right - run_width,
			y,
			w: run_width,
			h: 28.0,
		},
	});
	out
}

pub(super) fn draw_export(
	shaper: &mut TextShaper,
	settings: &ExportSettings,
	interaction: &InteractionState,
	document: &str,
	watching: bool,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	shaper.appearance = shaper.stylesheet.text(
		&shaper
			.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Panel,
	);
	let rect = panel_rect(width, height);
	let mut out = vec![
		Draw::Rect(
			Rect {
				x: 0.0,
				y: 0.0,
				w: width,
				h: height,
			},
			Paint::Scrim,
		),
		Draw::Rect(
			Rect {
				x: rect.x - 5.0,
				y: rect.y + 6.0,
				w: rect.w + 10.0,
				h: rect.h + 4.0,
			},
			Paint::Shadow,
		),
		Draw::Box {
			rect,
			chain: Condition::Panel.chain(),
			condition: Condition::Panel,
			radius: 0.,
			border: 1.,
			left_only: false,
		},
		Draw::Rect(
			Rect {
				x: rect.x,
				y: rect.y,
				w: 3.0,
				h: rect.h,
			},
			Paint::Styled(Condition::Panel, C::BorderColor),
		),
	];
	let x = rect.x + 20.0;
	out.extend(shaper.label(
		"Export",
		22.0,
		x,
		rect.y + 36.0,
		Paint::Styled(Condition::Panel, C::Color),
	));
	if rect.h >= 360.0 {
		// The document and the measure the current settings derive; a row's
		// own value is never repeated here.
		let (detail, color) = match export::geometry_summary(settings) {
			Ok(summary) => (
				format!(
					"{document} · {summary}{}",
					if watching { " · watching" } else { "" }
				),
				C::Muted,
			),
			Err(error) => (format!("{document} · {error}"), C::Error),
		};
		let detail = shaper.fit(&detail, 12.0, rect.w - 40.0);
		out.extend(shaper.label(
			&detail,
			12.0,
			x,
			rect.y + 61.0,
			Paint::Styled(Condition::Panel, color),
		));
	}
	for placed in place(rect, &rows(settings)) {
		let label = shaper.fit(&placed.row.label, 13.0, placed.label.w);
		out.extend(shaper.label(
			&label,
			13.0,
			placed.label.x,
			placed.label.y + placed.label.h / 2.0 + 4.5,
			Paint::Styled(Condition::Panel, C::Color),
		));
	}
	for button in export_controls(shaper, settings, width, height) {
		out.extend(draw_button(shaper, interaction, &button, true));
	}
	out
}

#[cfg(test)]
mod tests;
