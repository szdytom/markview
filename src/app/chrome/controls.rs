use super::super::Button;
use super::{
	components::{self, Action, ButtonKind, Form, Row, action, button},
	icons,
};
use crate::{
	layout::{Draw, Rect, TextShaper},
	settings::ReaderSettings,
	state::{Command, InteractionState},
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
fn rows(settings: &ReaderSettings) -> Vec<Row> {
	// Choosing a stylesheet is the Styles tab's own job, so this page offers
	// no theme row at all.
	vec![
		Row::new(
			"Text size",
			choices(&[("−", Command::Smaller), ("+", Command::Larger)], None),
		)
		.value(format!("{:.1} px", settings.font_size))
		.section("Reading layout"),
		Row::new(
			"Column width",
			choices(&[("−", Command::Narrower), ("+", Command::Wider)], None),
		)
		.value(format!("{:.0} px", settings.width)),
		Row::new(
			"Alignment",
			vec![action(
				if settings.justify {
					"Justified"
				} else {
					"Left aligned"
				},
				settings.justify,
				Command::Align,
			)],
		),
		Row::new(
			"Paragraph indent",
			choices(
				&[
					("Off", Command::Indent(0)),
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
			"Scroll speed",
			choices(
				&[
					("−", Command::ScrollSpeed(-1)),
					("+", Command::ScrollSpeed(1)),
				],
				None,
			),
		)
		.value(format!("{:.2}×", settings.scroll_speed)),
		Row::new(
			"CJK punctuation",
			choices(
				&[
					("SC", Command::CjkType(CjkType::Sc)),
					("TC", Command::CjkType(CjkType::Tc)),
					("JP", Command::CjkType(CjkType::Jp)),
					("None", Command::CjkType(CjkType::None)),
				],
				Some(Command::CjkType(settings.cjk_type)),
			),
		)
		.section("Language & code"),
		Row::new(
			"English hyphenation",
			vec![action(
				if settings.hyphenate { "On" } else { "Off" },
				settings.hyphenate,
				Command::Hyphens,
			)],
		),
		Row::new(
			"Code block wrapping",
			vec![action(
				if settings.codeblock_wrap { "On" } else { "Off" },
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
	);
	form.preview_control();
	form.footer(
		ui,
		&[
			(
				"Open settings.toml",
				Command::OpenConfig,
				ButtonKind::Standard,
			),
			("Reset defaults", Command::Reset, ButtonKind::Quiet),
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
		toolbar_controls(width, false)
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
pub(super) fn toolbar_controls(width: f32, outline_open: bool) -> Vec<Button> {
	[
		(icons::OPEN, "Open", Command::Open),
		(icons::EXPORT, "Export", Command::Export),
		(icons::SETTINGS, "Settings", Command::Settings),
		(icons::OUTLINE, "Outline", Command::Outline),
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
pub(super) fn draw_controls(
	ui: &mut TextShaper,
	settings: &ReaderSettings,
	interaction: &InteractionState,
	width: f32,
	height: f32,
) -> Vec<Draw> {
	if interaction.panel_open() {
		let form =
			form(ui, settings, interaction.settings_scroll, width, height);
		let rect = form.rect;
		let mut out = form
			.without_header()
			.preview(interaction.settings_preview)
			.draw(
				ui,
				interaction,
				"",
				"Saved automatically",
				C::Muted,
				(width, height),
			);
		out.extend(super::components::draw_settings_header(
			ui,
			interaction,
			rect,
			crate::state::PanelTab::Generic,
			interaction.settings_preview,
		));
		out
	} else {
		draw_toolbar(ui, interaction, width)
	}
}
pub(super) fn draw_toolbar(
	ui: &mut TextShaper,
	interaction: &InteractionState,
	width: f32,
) -> Vec<Draw> {
	components::appearance(ui);
	let idle = InteractionState::default();
	let state = if interaction.panel_open() || interaction.modal.is_some() {
		&idle
	} else {
		interaction
	};
	toolbar_controls(width, interaction.outline_open)
		.iter()
		.flat_map(|b| draw_button(ui, state, b, false))
		.collect()
}

#[cfg(test)]
#[path = "controls_tests.rs"]
mod tests;
