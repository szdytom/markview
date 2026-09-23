use super::*;
use crate::app::TOP;
use crate::lang::Lang;
use crate::state::{PanelPage, PanelTab};
#[test]
fn panel_exposes_first_line_indent_presets() {
	let mut shaper = crate::test_support::shaper();
	let buttons =
		controls(&mut shaper, &ReaderSettings::default(), true, 1200.0, 800.0);
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
	let mut shaper = crate::test_support::shaper();
	let mut label = |wrap| {
		let settings = ReaderSettings {
			codeblock_wrap: wrap,
			..Default::default()
		};
		let initial = form(&mut shaper, &settings, 0.0, 1200.0, 800.0);
		// The list is longer than the panel, so the last row is only on
		// screen once something has revealed it.
		let scroll = initial.reveal(Command::CodeWrap);
		form(&mut shaper, &settings, scroll, 1200.0, 800.0)
			.visible_buttons()
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
	for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)] {
		let mut shaper = crate::test_support::shaper();
		let panel = panel_rect(width, height);
		assert!(panel.y >= TOP, "panel must not cover the toolbar");
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
			vec![
				Command::Open,
				Command::Export,
				Command::Settings,
				Command::Outline
			]
		);
		let last = toolbar.last().expect("a toolbar button");
		assert_eq!(last.rect.x + last.rect.w, width - 16.0);
		assert!(toolbar.iter().all(|b| b.rect.y + b.rect.h < TOP));
	}
}
/// Chinese labels are narrower than their English counterparts, but nothing
/// relies on that: the panel is a fixed grid, so the same bounds hold.
#[test]
fn the_chinese_panel_keeps_its_controls_inside_the_panel() {
	let settings = ReaderSettings {
		lang: Some(Lang::ZhHans),
		..Default::default()
	};
	for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)] {
		let mut shaper = crate::test_support::shaper();
		let panel = panel_rect(width, height);
		let buttons = controls(&mut shaper, &settings, true, width, height);
		for button in &buttons {
			assert!(
				panel.contains(button.rect.x, button.rect.y)
					&& panel.contains(
						button.rect.x + button.rect.w,
						button.rect.y + button.rect.h
					),
				"{width}x{height}: {:?} leaves the panel",
				button.action
			);
		}
		for (index, a) in buttons.iter().enumerate() {
			for b in &buttons[index + 1..] {
				assert!(
					a.rect.intersect(b.rect).is_none(),
					"{width}x{height}: {:?} overlaps {:?}",
					a.action,
					b.action
				);
			}
		}
	}
}
/// Chinese prose has no spaces, so the About page's wrapper has to break
/// between characters; at the word boundaries English offers it would run the
/// whole description past the panel edge.
#[test]
fn a_chinese_description_wraps_inside_the_column() {
	let mut shaper = crate::test_support::shaper();
	let description = Lang::ZhHans.about_description();
	for width in [100.0, 160.0, 240.0] {
		let lines = wrap(&mut shaper, description, 13.0, width);
		assert!(lines.len() > 1, "a {width}-pixel column breaks it");
		for line in &lines {
			assert!(
				shaper.text_width(line, 13.0) <= width,
				"{line:?} is wider than {width}"
			);
		}
		// Breaking moves text around; it never drops or reorders a character.
		assert_eq!(
			lines.concat().replace(' ', ""),
			description.replace(' ', "")
		);
	}
}
#[test]
fn the_outline_button_carries_its_icon_and_toggled_state() {
	let outline = |open: bool| -> Button {
		toolbar_controls(1200.0, open, Lang::En)
			.into_iter()
			.find(|b| b.action == Command::Outline)
			.expect("the toolbar has an outline button")
	};
	let closed = outline(false);
	assert!(closed.icon.is_some());
	assert!(!closed.active);
	assert!(outline(true).active);
	// The drawer state reaches the drawn toolbar through interaction state.
	let draws = draw_toolbar(
		&mut crate::test_support::shaper(),
		&InteractionState {
			outline_open: true,
			..Default::default()
		},
		1200.0,
		Lang::En,
	);
	assert!(draws.iter().any(|draw| matches!(draw, Draw::Icon { .. })));
	for draw in draw_toolbar(
		&mut crate::test_support::shaper(),
		&InteractionState::default(),
		1200.0,
		Lang::En,
	) {
		if let Draw::Icon { x, .. } = draw {
			assert!(x >= 0.0 && x + 20.0 <= 1200.0);
		}
	}
}

#[test]
fn the_toolbar_and_panel_close_buttons_carry_icons() {
	let mut shaper = crate::test_support::shaper();
	let toolbar = controls(
		&mut shaper,
		&ReaderSettings::default(),
		false,
		1200.0,
		800.0,
	);
	assert!(toolbar.iter().all(|button| button.icon.is_some()));
	let panel =
		controls(&mut shaper, &ReaderSettings::default(), true, 1200.0, 800.0);
	let close = panel
		.iter()
		.find(|button| button.action == Command::Settings)
		.expect("panel close");
	assert_eq!(close.label, "Close");
	assert!(close.icon.is_some());
	let mut icons = 0;
	for draw in draw_controls(
		&mut shaper,
		&ReaderSettings::default(),
		&InteractionState {
			panel: PanelPage::Settings(PanelTab::Generic),
			..Default::default()
		},
		1200.0,
		800.0,
		None,
	) {
		if matches!(draw, Draw::Icon { .. }) {
			icons += 1;
		}
	}
	assert_eq!(icons, 2, "the panel draws its close and preview icons");
}
/// The compiled buffers promise a unit box, and the renderer scales every
/// coordinate by the drawn size without clamping.
#[test]
fn compiled_icons_stay_inside_the_unit_box() {
	for icon in [
		icons::OPEN,
		icons::EXPORT,
		icons::DOWNLOAD,
		icons::REDOWNLOAD,
		icons::SETTINGS,
		icons::OUTLINE,
		icons::CLOSE,
		icons::BACK,
		icons::UP,
		icons::DOWN,
		icons::MINUS,
		icons::PLUS,
		icons::EYE,
		icons::EYE_OFF,
	] {
		assert!(!icon.is_empty());
		for figure in icon {
			assert!(!figure.commands.is_empty());
			assert!(figure.fill || figure.stroke_width > 0.0);
			for (x, y) in coordinates(figure.commands) {
				assert!(
					(0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
					"icon left the unit box at {x}, {y}"
				);
			}
		}
	}
}
fn coordinates(
	commands: &[markview_core::scene::PathCommand],
) -> Vec<(f64, f64)> {
	use markview_core::scene::PathCommand as P;
	commands
		.iter()
		.flat_map(|command| match *command {
			P::MoveTo { x, y } | P::LineTo { x, y } => vec![(x, y)],
			P::QuadTo { x1, y1, x, y } => vec![(x1, y1), (x, y)],
			P::CubicTo {
				x1,
				y1,
				x2,
				y2,
				x,
				y,
			} => {
				vec![(x1, y1), (x2, y2), (x, y)]
			}
			P::Close => Vec::new(),
		})
		.collect()
}

#[test]
fn preview_keeps_controls_reachable_and_exit_icon_opaque() {
	let mut ui = crate::test_support::shaper();
	let settings = ReaderSettings::default();
	let normal = form(&mut ui, &settings, 0.0, 1200.0, 800.0);
	let preview = form(&mut ui, &settings, 0.0, 1200.0, 800.0).preview(true);
	for (a, b) in normal
		.visible_buttons()
		.iter()
		.zip(preview.visible_buttons())
	{
		assert_eq!(a.action, b.action);
		assert_eq!(
			(a.rect.x, a.rect.y, a.rect.w, a.rect.h),
			(b.rect.x, b.rect.y, b.rect.w, b.rect.h)
		);
	}
	let draws = draw_controls(
		&mut ui,
		&settings,
		&InteractionState {
			panel: PanelPage::Settings(PanelTab::Generic),
			settings_preview: true,
			..Default::default()
		},
		1200.0,
		800.0,
		None,
	);
	let Draw::Rect(_, paint) = draws[0] else {
		panic!("preview panel background");
	};
	assert!((ui.stylesheet.paint(paint)[3] - 0.25).abs() < 0.005);
	let alphas: Vec<_> = draws
		.iter()
		.filter_map(|draw| match draw {
			Draw::Icon { paint, .. } => Some(ui.stylesheet.paint(*paint)[3]),
			_ => None,
		})
		.collect();
	assert_eq!(alphas.len(), 2);
	assert!(alphas.iter().all(|alpha| *alpha == 1.0));
	assert!(!draws.iter().any(|draw| matches!(draw, Draw::Box { .. })));
}
#[test]
fn panel_steps_the_scroll_speed_between_its_bounds() {
	let mut shaper = crate::test_support::shaper();
	let buttons =
		controls(&mut shaper, &ReaderSettings::default(), true, 1200.0, 800.0);
	for action in [Command::ScrollSpeed(-1), Command::ScrollSpeed(1)] {
		assert!(
			buttons.iter().any(|b| b.action == action),
			"missing {action:?}"
		);
	}
}

#[test]
fn about_tab_keeps_navigation_and_scrolls_on_short_windows() {
	assert_eq!(
		icons::APP.len(),
		4,
		"both pages and both halves of the book"
	);
	assert!(icons::APP[1].commands.iter().any(|command| matches!(command, markview_core::scene::PathCommand::MoveTo { x, .. } if *x > 0.5)));
	let mut ui = crate::test_support::shaper();
	let settings = ReaderSettings::default();
	let interaction = InteractionState {
		panel: PanelPage::Settings(PanelTab::About),
		..Default::default()
	};
	for (width, height) in [(500.0, 300.0), (820.0, 600.0), (820.0, 800.0)] {
		let form = settings_form(
			&mut ui,
			&settings,
			&interaction,
			width,
			height,
			None,
		);
		assert_eq!(form.max_scroll > 0.0, height == 300.0);
		assert!(form.buttons.iter().all(|b| matches!(
			b.action,
			Command::Settings
				| Command::SettingsPreview
				| Command::CopyDiagnostics
				| Command::OpenProject
		)));
		let copy = form
			.buttons
			.iter()
			.find(|b| b.action == Command::CopyDiagnostics)
			.unwrap();
		assert!(copy.icon.is_some());
		assert!(
			form.visible_buttons()
				.iter()
				.any(|b| b.action == Command::CopyDiagnostics)
		);
		let project = form
			.buttons
			.iter()
			.find(|b| b.action == Command::OpenProject)
			.unwrap();
		assert_eq!(project.label, env!("CARGO_PKG_REPOSITORY"));
		assert_eq!(
			form.visible_buttons()
				.iter()
				.any(|b| b.action == Command::OpenProject),
			height != 300.0
		);
		let scrolled = InteractionState {
			panel: interaction.panel,
			settings_scroll: form.reveal(Command::OpenProject),
			..Default::default()
		};
		let bottom =
			settings_form(&mut ui, &settings, &scrolled, width, height, None);
		assert!(
			bottom
				.visible_buttons()
				.iter()
				.any(|b| b.action == Command::OpenProject)
		);
		let tabs =
			components::tab_controls(form.rect, PanelTab::About, Lang::En);
		assert_eq!(tabs.len(), 4);
		assert_eq!(tabs.iter().filter(|b| b.active).count(), 1);
		assert!(
			tabs.iter().any(|b| b.active
				&& b.action == Command::SettingsTab(PanelTab::About))
		);
		assert!(tabs.iter().all(|b| {
			form.rect.contains(b.rect.x + b.rect.w, b.rect.y + b.rect.h)
		}));
		let draws = draw_controls(
			&mut ui,
			&settings,
			&interaction,
			width,
			height,
			None,
		);
		let icon = draws
			.iter()
			.find_map(|draw| {
				let Draw::Clipped { draws, .. } = draw else {
					return None;
				};
				draws.iter().find_map(|draw| match draw {
					Draw::Icon { x, size, .. } if *size == 64.0 => {
						Some((*x, *size))
					}
					_ => None,
				})
			})
			.expect("application icon in the scrolling body");
		assert!(
			(icon.0 + icon.1 / 2.0 - (form.rect.x + form.rect.w / 2.0)).abs()
				< 0.01
		);
	}
}

/// The interface's own language row, which is the reason the list exists: it
/// offers every language the build carries, so adding one costs a locale file
/// and a variant rather than an edit here.
#[test]
fn the_language_row_offers_every_language_the_build_carries() {
	let settings = ReaderSettings::default();
	let t = settings.lang();
	let entries = language_options(t, &settings);
	assert_eq!(entries.len(), 1 + Lang::ALL.len());
	assert_eq!(entries.iter().filter(|entry| entry.active).count(), 1);
	// Following the system comes first and is named in the language in force.
	assert_eq!(entries[0].label, t.settings_language_system());
	assert_eq!(entries[0].action, Command::Language(None));
	assert!(entries[0].active);
	// Every language names itself.
	for (entry, lang) in entries[1..].iter().zip(Lang::ALL) {
		assert_eq!(entry.label, lang.language_name());
		assert_eq!(entry.action, Command::Language(Some(*lang)));
		assert!(!entry.active);
	}

	// A chosen language marks itself rather than the system.
	let settings = ReaderSettings {
		lang: Some(Lang::ZhHans),
		..Default::default()
	};
	let entries = language_options(settings.lang(), &settings);
	assert!(!entries[0].active);
	assert!(entries.iter().any(|entry| {
		entry.active && entry.action == Command::Language(Some(Lang::ZhHans))
	}));
}

/// The closed control shows the language in force, and is marked as opening a
/// list rather than stepping through options.
#[test]
fn the_language_control_shows_the_language_in_force() {
	for (lang, expected) in [
		(None, Lang::En.settings_language_system()),
		(Some(Lang::ZhHans), Lang::ZhHans.language_name()),
		(Some(Lang::En), Lang::En.language_name()),
	] {
		let settings = ReaderSettings {
			lang,
			..Default::default()
		};
		let buttons = controls(
			&mut crate::test_support::shaper(),
			&settings,
			true,
			1200.0,
			800.0,
		);
		let control = buttons
			.iter()
			.find(|button| {
				matches!(
					button.action,
					Command::ToggleDropdown(DropdownId::Language, _)
				)
			})
			.expect("the language control");
		assert_eq!(control.label, expected);
		assert!(control.marker.is_some(), "the control is marked");
		// Opening the list starts on the option in force.
		let Command::ToggleDropdown(_, highlight) = control.action else {
			unreachable!()
		};
		let entries = language_options(settings.lang(), &settings);
		assert!(entries[highlight].active);
	}
}
