use super::*;
use crate::app::TOP;
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
#[test]
fn the_outline_button_carries_its_icon_and_toggled_state() {
	let outline = |open: bool| -> Button {
		toolbar_controls(1200.0, open)
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
	);
	assert!(draws.iter().any(|draw| matches!(draw, Draw::Icon { .. })));
	for draw in draw_toolbar(
		&mut crate::test_support::shaper(),
		&InteractionState::default(),
		1200.0,
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
		icons::SETTINGS,
		icons::OUTLINE,
		icons::CLOSE,
		icons::BACK,
		icons::UP,
		icons::DOWN,
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
