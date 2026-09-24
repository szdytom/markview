use super::*;
use crate::app::chrome::panel_rect;
use crate::lang::Lang;
use crate::state::PanelPage;

fn settings(format: ExportFormat) -> ExportSettings {
	ExportSettings {
		format,
		..Default::default()
	}
}

#[test]
fn export_panel_buttons_fit_and_offer_the_run_action() {
	let mut shaper = crate::test_support::shaper();
	for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)] {
		let panel = panel_rect(width, height);
		let buttons = export_controls(
			&mut shaper,
			&ExportSettings::default(),
			width,
			height,
			Lang::En,
		);
		assert!(buttons.iter().any(|b| b.action == Command::ExportRun));
		assert!(buttons.iter().all(|b| b.action != Command::Open));
		for button in &buttons {
			assert!(panel.contains(button.rect.x, button.rect.y));
			assert!(panel.contains(
				button.rect.x + button.rect.w,
				button.rect.y + button.rect.h
			));
		}
	}
}

#[test]
fn the_png_scale_row_only_exists_for_png() {
	let pdf = rows(&settings(ExportFormat::Pdf), Lang::En);
	assert!(!pdf.iter().any(|row| row.label.starts_with("PNG scale")));
	let png = rows(&settings(ExportFormat::Png), Lang::En);
	assert!(png.iter().any(|row| row.label.starts_with("PNG scale")));
}

#[test]
fn the_active_choice_is_marked() {
	let png = rows(&settings(ExportFormat::Png), Lang::En);
	let format = png
		.iter()
		.find(|row| row.label.starts_with("Format"))
		.expect("format row");
	assert_eq!(format.actions[0].label, "PDF");
	assert!(!format.actions[0].active);
	assert_eq!(format.actions[1].label, "PNG");
	assert!(format.actions[1].active);
	// The default paper keeps the print sheet's margins selected.
	let paper = rows(&settings(ExportFormat::Pdf), Lang::En)
		.into_iter()
		.find(|row| row.label.starts_with("Paper"))
		.expect("paper row");
	assert_eq!(paper.actions[0].label, "A4");
	assert!(paper.actions[0].active);
}

#[test]
fn the_panel_offers_the_export_styles() {
	let panel = rows(&settings(ExportFormat::Pdf), Lang::En);
	let styles = panel
		.iter()
		.find(|row| row.label.starts_with("Styles"))
		.expect("styles row");
	assert_eq!(styles.label, "Styles · print");
	assert_eq!(styles.actions[0].action, Command::ExportStyles);
	// Watching is an action, not a setting, so it has no row of its own.
	assert!(!panel.iter().any(|row| row.label.starts_with("Watch")));
}

#[test]
fn the_plain_export_sits_right_of_the_watching_one() {
	let mut shaper = crate::test_support::shaper();
	let buttons = export_controls(
		&mut shaper,
		&ExportSettings::default(),
		1200.0,
		800.0,
		Lang::En,
	);
	let watch = buttons
		.iter()
		.find(|b| b.action == Command::ExportAndWatch)
		.expect("watch button");
	assert_eq!(watch.label, "Export and Watch…");
	let run = buttons
		.iter()
		.find(|b| b.action == Command::ExportRun)
		.expect("run button");
	assert!(watch.rect.x + watch.rect.w <= run.rect.x);
}

#[test]
fn the_drawn_panel_keeps_the_panel_geometry() {
	let mut shaper = crate::test_support::shaper();
	let draws = draw_export(
		&mut shaper,
		&ExportSettings::default(),
		&InteractionState {
			panel: PanelPage::Export,
			..Default::default()
		},
		"doc.md",
		false,
		(1200.0, 800.0),
		Lang::En,
	);
	let panel = panel_rect(1200.0, 800.0);
	let framed = draws.iter().any(|draw| match draw {
		Draw::Box { rect, .. } => {
			rect.x == panel.x
				&& rect.y == panel.y
				&& rect.w == panel.w
				&& rect.h == panel.h
		}
		_ => false,
	});
	assert!(framed, "the panel frame is drawn");
}

#[test]
fn panel_buttons_never_overlap() {
	let mut shaper = crate::test_support::shaper();
	// The panel's grid does not move for a longer label, so the translations
	// have to hold inside the same slots the English ones do.
	for &lang in Lang::ALL {
		for (width, height) in [(500.0, 300.0), (820.0, 600.0), (1200.0, 800.0)]
		{
			let buttons = export_controls(
				&mut shaper,
				&settings(ExportFormat::Png),
				width,
				height,
				lang,
			);
			for (index, a) in buttons.iter().enumerate() {
				for b in &buttons[index + 1..] {
					let overlap = a.rect.x < b.rect.x + b.rect.w
						&& b.rect.x < a.rect.x + a.rect.w
						&& a.rect.y < b.rect.y + b.rect.h
						&& b.rect.y < a.rect.y + a.rect.h;
					assert!(
						!overlap,
						"{lang:?} buttons overlap at {width}×{height}"
					);
				}
			}
		}
	}
}
