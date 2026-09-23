use super::super::Button;
use super::{
	components::{self, Action, ButtonKind, Form, Row, action, button},
	icons,
};
use crate::{
	lang::Lang,
	layout::{Draw, Rect, TextShaper},
	settings::ReaderSettings,
	state::{Command, DropdownId, InteractionState, PanelPage, PanelTab},
};
pub(super) use components::{draw_button, panel_rect};
use markview_core::style::{
	CjkType, ColorField as C, Condition, TextAppearance,
};
pub(super) const ICON_BUTTON: f32 = 28.0;
const GAP: f32 = 4.0;

fn choices(
	entries: &[(&'static str, Command)],
	selected: Option<Command>,
) -> Vec<Action> {
	entries
		.iter()
		.map(|&(label, command)| {
			action(label, selected == Some(command), command)
		})
		.collect()
}
/// The interface languages, offered as a list of the current one.
///
/// Following the system comes first and is named in the language in force;
/// every language after it is named in its own, so the list reads the same
/// however the interface is currently drawn.
fn language_options(t: Lang, settings: &ReaderSettings) -> Vec<Action> {
	let mut entries = vec![action(
		t.settings_language_system(),
		settings.lang.is_none(),
		Command::Language(None),
	)];
	entries.extend(Lang::ALL.iter().map(|lang| {
		action(
			lang.language_name(),
			settings.lang == Some(*lang),
			Command::Language(Some(*lang)),
		)
	}));
	entries
}
fn rows(settings: &ReaderSettings) -> Vec<Row> {
	// Choosing a stylesheet is the Styles tab's own job, so this page offers
	// no theme row at all.
	let t = settings.lang();
	vec![
		Row::new(t.settings_language(), vec![])
			.menu(DropdownId::Language, language_options(t, settings))
			.section(t.section_interface()),
		// How far a scroll request moves is a property of the pointing device,
		// not of the type, so it belongs beside the interface language.
		Row::new(
			t.settings_scroll_speed(),
			choices(
				&[
					(t.settings_decrease(), Command::ScrollSpeed(-1)),
					(t.settings_increase(), Command::ScrollSpeed(1)),
				],
				None,
			),
		)
		.value(format!("{:.2}×", settings.scroll_speed)),
		Row::new(
			t.settings_text_size(),
			choices(
				&[
					(t.settings_decrease(), Command::Smaller),
					(t.settings_increase(), Command::Larger),
				],
				None,
			),
		)
		.value(format!("{:.1} px", settings.font_size))
		.section(t.section_reading_layout()),
		Row::new(
			t.settings_column_width(),
			choices(
				&[
					(t.settings_decrease(), Command::Narrower),
					(t.settings_increase(), Command::Wider),
				],
				None,
			),
		)
		.value(format!("{:.0} px", settings.width)),
		Row::new(
			t.settings_alignment(),
			vec![action(
				if settings.justify {
					t.settings_justified()
				} else {
					t.settings_left_aligned()
				},
				settings.justify,
				Command::Align,
			)],
		),
		Row::new(
			t.settings_paragraph_indent(),
			choices(
				&[
					(t.settings_indent_off(), Command::Indent(0)),
					("1 em", Command::Indent(1)),
					("2 em", Command::Indent(2)),
					("3 em", Command::Indent(3)),
				],
				(0..4)
					.find(|n| {
						(settings.paragraph_indent - f32::from(*n)).abs() < 0.01
					})
					.map(Command::Indent),
			),
		),
		Row::new(
			t.settings_cjk_punctuation(),
			choices(
				&[
					("SC", Command::CjkType(CjkType::Sc)),
					("TC", Command::CjkType(CjkType::Tc)),
					("JP", Command::CjkType(CjkType::Jp)),
					(t.settings_cjk_none(), Command::CjkType(CjkType::None)),
				],
				Some(Command::CjkType(settings.cjk_type)),
			),
		)
		.section(t.section_language_code()),
		Row::new(
			t.settings_english_hyphenation(),
			vec![action(
				if settings.hyphenate {
					t.settings_on()
				} else {
					t.settings_off()
				},
				settings.hyphenate,
				Command::Hyphens,
			)],
		),
		Row::new(
			t.settings_codeblock_wrapping(),
			vec![action(
				if settings.codeblock_wrap {
					t.settings_on()
				} else {
					t.settings_off()
				},
				settings.codeblock_wrap,
				Command::CodeWrap,
			)],
		),
	]
}
pub(in crate::app) fn form(
	ui: &mut TextShaper,
	settings: &ReaderSettings,
	scroll: f32,
	width: f32,
	height: f32,
) -> Form {
	let mut form = Form::new(
		width,
		height,
		scroll,
		rows(settings),
		Some(Command::Settings),
		true,
		settings.lang(),
	);
	form.preview_control();
	let t = settings.lang();
	form.footer(
		ui,
		&[
			(
				t.settings_open_config(),
				Command::OpenConfig,
				ButtonKind::Standard,
			),
			(
				t.settings_reset_defaults(),
				Command::Reset,
				ButtonKind::Quiet,
			),
		],
	);
	form
}
#[cfg(test)]
pub(super) fn controls(
	ui: &mut TextShaper,
	settings: &ReaderSettings,
	panel_open: bool,
	width: f32,
	height: f32,
) -> Vec<Button> {
	if panel_open {
		form(ui, settings, 0.0, width, height).visible_buttons()
	} else {
		toolbar_controls(width, false, settings.lang())
	}
}
pub(super) fn button_width(
	shaper: &mut TextShaper,
	label: &str,
	size: f32,
) -> f32 {
	let old = shaper.appearance.clone();
	shaper.appearance = shaper
		.stylesheet
		.text(&TextAppearance::default(), Condition::Ui);
	let width = shaper.text_width(label, size) + 18.0;
	shaper.appearance = old;
	width
}
/// Wraps running text to a column `width` pixels wide.
///
/// Words break at the spaces between them; a word wider than a line is broken
/// between its characters instead. Chinese prose has no spaces to break at, so
/// without that second rule a whole description would run off the panel.
fn wrap(ui: &mut TextShaper, text: &str, size: f32, width: f32) -> Vec<String> {
	let mut lines = Vec::new();
	let mut line = String::new();
	for word in text.split(' ') {
		let joined = if line.is_empty() {
			word.to_owned()
		} else {
			format!("{line} {word}")
		};
		if ui.text_width(&joined, size) <= width {
			line = joined;
			continue;
		}
		if !line.is_empty() {
			lines.push(std::mem::take(&mut line));
		}
		for character in word.chars() {
			if !line.is_empty() {
				let mut widened = line.clone();
				widened.push(character);
				if ui.text_width(&widened, size) > width {
					lines.push(std::mem::take(&mut line));
				}
			}
			line.push(character);
		}
	}
	if !line.is_empty() {
		lines.push(line);
	}
	lines
}
pub(super) fn toolbar_controls(
	width: f32,
	outline_open: bool,
	lang: Lang,
) -> Vec<Button> {
	[
		(icons::OPEN, lang.toolbar_open(), Command::Open),
		(icons::EXPORT, lang.toolbar_export(), Command::Export),
		(icons::SETTINGS, lang.toolbar_settings(), Command::Settings),
		(icons::OUTLINE, lang.toolbar_outline(), Command::Outline),
	]
	.into_iter()
	.enumerate()
	.map(|(i, (icon, label, action))| {
		let mut b = button(
			label,
			action,
			Rect {
				x: toolbar_right_edge(width) + i as f32 * (ICON_BUTTON + GAP),
				y: 6.0,
				w: ICON_BUTTON,
				h: ICON_BUTTON,
			},
		);
		b.icon = Some(icon);
		b.kind = ButtonKind::Quiet;
		b.active = action == Command::Outline && outline_open;
		b
	})
	.collect()
}
pub(super) fn toolbar_right_edge(width: f32) -> f32 {
	width - 4.0 * ICON_BUTTON - 3.0 * GAP - 16.0
}
pub(super) fn settings_form(
	ui: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	width: f32,
	height: f32,
	backend: Option<wgpu::Backend>,
) -> Form {
	if interaction.panel != PanelPage::Settings(PanelTab::About) {
		return form(ui, settings, interaction.settings_scroll, width, height);
	}
	components::appearance(ui);
	let t = settings.lang();
	let width_available = panel_rect(width, height).w - components::INSET * 2.0;
	let mut rows: Vec<_> =
		wrap(ui, t.about_description(), 13.0, width_available)
			.into_iter()
			.map(|line| Row::new(line, vec![]))
			.collect();
	rows.insert(0, Row::icon(icons::APP));
	for (index, (name, value)) in crate::diagnostics::fields(t, backend)
		.into_iter()
		.enumerate()
	{
		let row = Row::new(t.diagnostics_row(name, value), vec![]);
		rows.push(if index == 0 {
			row.section(t.section_diagnostics())
		} else {
			row
		});
	}
	rows.extend([
		Row::new(
			t.about_created_by(
				env!("CARGO_PKG_AUTHORS"),
				env!("CARGO_PKG_LICENSE"),
			),
			vec![],
		)
		.section(t.section_project()),
		Row::link(env!("CARGO_PKG_REPOSITORY"), Command::OpenProject),
	]);
	let mut form = Form::new(
		width,
		height,
		interaction.settings_scroll,
		rows,
		Some(Command::Settings),
		false,
		t,
	);
	form.footer(
		ui,
		&[(
			t.about_copy_diagnostics(),
			Command::CopyDiagnostics,
			ButtonKind::Standard,
		)],
	);
	form.preview_control();
	form
}

pub(super) fn draw_controls(
	ui: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	width: f32,
	height: f32,
	backend: Option<wgpu::Backend>,
) -> Vec<Draw> {
	if interaction.panel_open() {
		let t = settings.lang();
		let form =
			settings_form(ui, settings, interaction, width, height, backend);
		let rect = form.rect;
		// An open option list is only reached through a form, so the form
		// measures it and the same value answers the pointer. Measuring also
		// brings its highlight into view, on a copy: the state is corrected
		// the next time the input paths measure it.
		let menu = interaction
			.dropdown
			.and_then(|mut open| form.menu(&mut open, (width, height)));
		let mut out = form
			.without_header()
			.preview(interaction.settings_preview)
			.draw(
				ui,
				interaction,
				"",
				if rect.h < 300.0
					|| interaction.panel == PanelPage::Settings(PanelTab::About)
				{
					""
				} else {
					t.panel_saved()
				},
				C::Muted,
				(width, height),
			);
		out.extend(super::components::draw_settings_header(
			ui,
			interaction,
			rect,
			if interaction.panel == PanelPage::Settings(PanelTab::About) {
				PanelTab::About
			} else {
				PanelTab::Generic
			},
			interaction.settings_preview,
			t,
		));
		// The list floats over the page, so it goes on after everything the
		// page draws.
		if let Some(menu) = &menu {
			out.extend(super::components::draw_menu(ui, interaction, menu));
		}
		out
	} else {
		draw_toolbar(ui, interaction, width, settings.lang())
	}
}
pub(super) fn draw_toolbar(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	width: f32,
	lang: Lang,
) -> Vec<Draw> {
	components::appearance(ui);
	let idle = InteractionState::default();
	let state = if interaction.panel_open() || interaction.modal.is_some() {
		&idle
	} else {
		interaction
	};
	toolbar_controls(width, interaction.outline_open, lang)
		.iter()
		.flat_map(|b| draw_button(ui, state, b, false))
		.collect()
}

#[cfg(test)]
#[path = "controls_tests.rs"]
mod tests;
