//! Export preferences remain independent of the reading view.
use super::components::{ButtonKind, Form, Row, action};
use crate::state::ExportCommand;
use crate::{
	app::export::{MARGIN_PRESETS, PAPER_PRESETS, SCALE_PRESETS},
	export,
	lang::Lang,
	layout::{Draw, TextShaper},
	settings::{ExportFormat, ExportSettings},
	state::Command,
};
use markview_layout::style::ColorField as C;

fn rows(settings: &ExportSettings, lang: Lang) -> Vec<Row> {
	let mut rows = vec![
		Row::new(
			lang.export_format(),
			vec![
				action(
					lang.export_pdf(),
					settings.format == ExportFormat::Pdf,
					Command::Export(ExportCommand::ExportFormat(
						ExportFormat::Pdf,
					)),
				),
				action(
					lang.export_png(),
					settings.format == ExportFormat::Png,
					Command::Export(ExportCommand::ExportFormat(
						ExportFormat::Png,
					)),
				),
			],
		)
		.section(lang.export_output()),
	];
	if settings.format == ExportFormat::Png {
		rows.push(Row::new(
			lang.export_scale(),
			SCALE_PRESETS
				.iter()
				.enumerate()
				.map(|(i, (scale, label))| {
					action(
						*label,
						(settings.scale - scale).abs() < 1e-3,
						Command::Export(ExportCommand::ExportScale(i as u8)),
					)
				})
				.collect(),
		));
	}
	rows.extend([
		Row::new(
			lang.export_paper(&settings.paper),
			PAPER_PRESETS
				.iter()
				.enumerate()
				.map(|(i, (label, paper))| {
					action(
						*label,
						settings.paper.eq_ignore_ascii_case(paper),
						Command::Export(ExportCommand::ExportPaper(i as u8)),
					)
				})
				.collect(),
		)
		.section(lang.export_page()),
		Row::new(
			lang.export_orientation(),
			vec![
				action(
					lang.export_portrait(),
					!settings.landscape,
					Command::Export(ExportCommand::ExportOrientation(false)),
				),
				action(
					lang.export_landscape(),
					settings.landscape,
					Command::Export(ExportCommand::ExportOrientation(true)),
				),
			],
		),
		Row::new(
			lang.export_margins(),
			MARGIN_PRESETS
				.iter()
				.enumerate()
				.map(|(i, (margin, label))| {
					action(
						*label,
						settings.margin == *margin,
						Command::Export(ExportCommand::ExportMargin(i as u8)),
					)
				})
				.collect(),
		),
		Row::new(
			lang.export_styles(if settings.style.is_empty() {
				lang.export_styles_none().to_owned()
			} else {
				settings.style.join(lang.export_style_separator())
			}),
			vec![action(
				lang.export_styles_open(),
				false,
				Command::Export(ExportCommand::ExportStyles),
			)],
		)
		.section(lang.export_typography()),
		Row::new(
			lang.export_text_size(),
			vec![
				action(
					lang.export_decrease(),
					false,
					Command::Export(ExportCommand::ExportSize(-1)),
				),
				action(
					lang.export_increase(),
					false,
					Command::Export(ExportCommand::ExportSize(1)),
				),
			],
		)
		.value(format!("{} px", settings.font_size)),
		Row::new(
			lang.export_paragraph_indent(),
			[lang.export_indent_off(), "1 em", "2 em", "3 em"]
				.into_iter()
				.enumerate()
				.map(|(i, label)| {
					action(
						label,
						(settings.paragraph_indent - i as f32).abs() < 0.01,
						Command::Export(ExportCommand::ExportIndent(i as u8)),
					)
				})
				.collect(),
		),
	]);
	if settings.format == ExportFormat::Pdf {
		rows.push(
			Row::new(lang.export_document_title(), vec![])
				.input(crate::state::TextField::ExportTitle),
		);
	}
	rows
}

pub(in crate::app) fn form(
	ui: &mut TextShaper,
	settings: &ExportSettings,
	scroll: f32,
	width: f32,
	height: f32,
	lang: Lang,
) -> Form {
	let mut form = Form::new(
		width,
		height,
		scroll,
		rows(settings, lang),
		Some(Command::Export(ExportCommand::Export)),
		false,
		lang,
	);
	form.footer(
		ui,
		&[
			(
				lang.export_and_watch(),
				Command::Export(ExportCommand::ExportAndWatch),
				ButtonKind::Standard,
			),
			(
				lang.export_run(),
				Command::Export(ExportCommand::ExportRun),
				ButtonKind::Primary,
			),
		],
	);
	if export::geometry(settings).is_err() {
		for button in &mut form.buttons {
			if matches!(
				button.action,
				Command::Export(ExportCommand::ExportRun)
					| Command::Export(ExportCommand::ExportAndWatch)
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
	lang: Lang,
) -> Vec<crate::app::Button> {
	form(ui, settings, 0.0, width, height, lang).visible_buttons()
}

#[cfg(test)]
pub(super) fn draw_export(
	ui: &mut TextShaper,
	settings: &ExportSettings,
	interaction: &crate::app::widgets::Drawn<'_>,
	document: &str,
	watching: bool,
	size: (f32, f32),
	lang: Lang,
) -> Vec<Draw> {
	let form = form(
		ui,
		settings,
		interaction.panels.export_scroll().offset(),
		size.0,
		size.1,
		lang,
	);
	draw_export_form(
		ui,
		settings,
		interaction,
		(document, watching),
		size,
		lang,
		&form,
	)
}
pub(super) fn draw_export_form(
	ui: &mut TextShaper,
	settings: &ExportSettings,
	interaction: &crate::app::widgets::Drawn<'_>,
	status: (&str, bool),
	size: (f32, f32),
	lang: Lang,
	form: &Form,
) -> Vec<Draw> {
	let (document, watching) = status;
	let (width, height) = size;
	let (detail, color) = match export::geometry_summary(settings, lang) {
		Ok(summary) if watching => {
			(lang.export_status_watching(document, summary), C::Muted)
		}
		Ok(summary) => (lang.export_status(document, summary), C::Muted),
		Err(error) => (lang.export_status_failed(document, error), C::Error),
	};
	form.draw(
		ui,
		interaction,
		lang.export_title(),
		&detail,
		color,
		(width, height),
	)
}

#[cfg(test)]
mod tests;
