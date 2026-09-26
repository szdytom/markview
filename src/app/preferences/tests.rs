use super::Preferences;
use crate::cli::{LaunchOptions, Mode};
use std::sync::Arc;

#[test]
fn image_export_wraps_code_blocks_by_default() {
	let args = LaunchOptions {
		mode: Mode::Smoke,
		options: crate::test_support::options(),
		..Default::default()
	};
	let mut ui = crate::test_support::shaper();
	let preferences = Preferences::new(&args, &mut ui);
	assert!(preferences.values.codeblock_wrap);
	assert!(
		preferences
			.values
			.layout_options(900.0, false, &crate::test_support::fonts())
			.codeblock_wrap
	);
}

#[test]
fn invalid_stylesheet_update_preserves_effective_sheet_and_ui() {
	let args = LaunchOptions {
		mode: Mode::Smoke,
		options: crate::test_support::options(),
		..Default::default()
	};
	let mut ui = crate::test_support::shaper();
	let mut preferences = Preferences::new(&args, &mut ui);
	let before = preferences.values.stylesheet.clone();
	let width = ui.text_width("Reader settings", 14.0);
	preferences.values.style = Some(vec!["../invalid-id".into()]);
	assert_eq!(preferences.reload_styles(&mut ui), None);
	assert!(Arc::ptr_eq(&before, &preferences.values.stylesheet));
	assert!(Arc::ptr_eq(&before, &ui.stylesheet));
	assert_eq!(width, ui.text_width("Reader settings", 14.0));
	assert!(preferences.style_warning.is_some());
	assert!(preferences.path().is_none());
	assert!(preferences.save_deadline().is_none());
}
