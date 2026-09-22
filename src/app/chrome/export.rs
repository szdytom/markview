//! Export preferences remain independent of the reading view.
use super::components::{ButtonKind, Form, Row, action};
use crate::{
	app::export::{MARGIN_PRESETS, PAPER_PRESETS, SCALE_PRESETS},
	export,
	layout::{Draw, TextShaper},
	settings::{ExportFormat, ExportSettings},
	state::{Command, InteractionState},
};
use markview_core::style::ColorField as C;

fn rows(settings: &ExportSettings) -> Vec<Row> {
	let mut rows = vec![
		Row::new(
			"Format",
			vec![
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
		)
		.section("Output"),
	];
	if settings.format == ExportFormat::Png {
		rows.push(Row::new(
			"PNG scale",
			SCALE_PRESETS
				.iter()
				.enumerate()
				.map(|(i, (scale, label))| {
					action(
						label,
						(settings.scale - scale).abs() < 1e-3,
						Command::ExportScale(i as u8),
					)
				})
				.collect(),
		));
	}
	rows.extend([
		Row::new(
			format!("Paper · {}", settings.paper),
			PAPER_PRESETS
				.iter()
				.enumerate()
				.map(|(i, (label, paper))| {
					action(
						label,
						settings.paper.eq_ignore_ascii_case(paper),
						Command::ExportPaper(i as u8),
					)
				})
				.collect(),
		)
		.section("Page"),
		Row::new(
			"Orientation",
			vec![
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
		),
		Row::new(
			"Margins",
			MARGIN_PRESETS
				.iter()
				.enumerate()
				.map(|(i, (margin, label))| {
					action(
						label,
						settings.margin == *margin,
						Command::ExportMargin(i as u8),
					)
				})
				.collect(),
		),
		Row::new(
			format!(
				"Styles · {}",
				if settings.style.is_empty() {
					"none".into()
				} else {
					settings.style.join(", ")
				}
			),
			vec![action("Styles…", false, Command::ExportStyles)],
		)
		.section("Typography"),
		Row::new(
			"Text size",
			vec![
				action("Decrease", false, Command::ExportSize(-1)),
				action("Increase", false, Command::ExportSize(1)),
			],
		)
		.value(format!("{} px", settings.font_size)),
		Row::new(
			"Paragraph indent",
			["Off", "1 em", "2 em", "3 em"]
				.into_iter()
				.enumerate()
				.map(|(i, label)| {
					action(
						label,
						(settings.paragraph_indent - i as f32).abs() < 0.01,
						Command::ExportIndent(i as u8),
					)
				})
				.collect(),
		),
	]);
	rows
}

pub(in crate::app) fn form(
	ui: &mut TextShaper,
	settings: &ExportSettings,
	scroll: f32,
	width: f32,
	height: f32,
) -> Form {
	let mut form = Form::new(
		width,
		height,
		scroll,
		rows(settings),
		Some(Command::Export),
		false,
	);
	form.footer(
		ui,
		&[
			(
				"Export and Watch…",
				Command::ExportAndWatch,
				ButtonKind::Standard,
			),
			("Export…", Command::ExportRun, ButtonKind::Primary),
		],
	);
	if export::geometry(settings).is_err() {
		for button in &mut form.buttons {
			if matches!(
				button.action,
				Command::ExportRun | Command::ExportAndWatch
			) {
				button.enabled = false;
			}
		}
	}
	form
}

#[cfg(test)]
fn export_controls(
	ui: &mut TextShaper,
	settings: &ExportSettings,
	width: f32,
	height: f32,
) -> Vec<crate::app::Button> {
	form(ui, settings, 0.0, width, height).visible_buttons()
}

pub(super) fn draw_export(
	ui: &mut TextShaper,
	settings: &ExportSettings,
	interaction: &InteractionState,
	document: &str,
	watching: bool,
	width: f32,
	height: f32,
) -> Vec<Draw> {
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
	form(ui, settings, interaction.export_scroll, width, height).draw(
		ui,
		interaction,
		"Export document",
		&detail,
		color,
		(width, height),
	)
}

#[cfg(test)]
mod tests;
