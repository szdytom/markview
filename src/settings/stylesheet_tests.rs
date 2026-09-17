use super::*;
use std::fs;
#[test]
fn stylesheet_lists_migrate_merge_and_preserve_personal_preferences() {
	let tmp = tempfile::tempdir().unwrap();
	let path = tmp.path().join("settings.toml");
	fs::write(
		&path,
		"# Preferences\ntheme='dark'\nfont_size=23\nwidth=900\n",
	)
	.unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(store.settings().style, Some(vec!["dark".into()]));
	let mut ui = store.settings();
	ui.style = Some(vec!["paper".into(), "dark".into()]);
	store.changed(&ui, Some(Setting::Theme));
	fs::write(
		&path,
		"# Preferences\ntheme='light'\nstyle=['external']\nfont_size=25\nwidth=960\n",
	)
	.unwrap();
	store.flush().unwrap();
	let source = fs::read_to_string(&path).unwrap();
	assert!(source.contains("# Preferences"));
	assert!(!source.contains("theme"));
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert_eq!(loaded.settings().style, ui.style);
	assert_eq!(loaded.settings().font_size, 25.);
	assert_eq!(loaded.settings().width, 960.);
}
#[test]
fn empty_style_overrides_legacy_theme_and_survives_save() {
	let tmp = tempfile::tempdir().unwrap();
	let path = tmp.path().join("settings.toml");
	fs::write(&path, "theme='dark'\nstyle=[]").unwrap();
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	assert_eq!(store.settings().style, Some(vec![]));
	assert_eq!(store.settings().theme, Theme::Light);
	let mut ui = store.settings();
	ui.font_size = 22.;
	store.changed(&ui, Some(Setting::FontSize));
	store.flush().unwrap();
	assert_eq!(
		SettingsStore::load(Some(path)).0.settings().style,
		Some(vec![])
	);
}

#[test]
fn a_run_selects_a_cjk_variant_whatever_styles_it_loads() {
	// A variant has to be selected for the `[cjk]` font definitions to exist at
	// all: leaving it unselected made every diagnostic render draw CJK text in
	// a system fallback face instead of the configured one.
	for cjk in [CjkType::Sc, CjkType::Tc, CjkType::Jp] {
		let sheet = crate::stylesheet::load_for_run(None, None, cjk).unwrap();
		assert_eq!(sheet.cjk_type(), cjk);
		assert!(sheet.fontdefs.contains_key("serif[cjk]"), "{cjk:?}");
	}
	// Loading without a variant stays available, and stays unselected.
	let bare = crate::stylesheet::load_with_cjk_type(&[], None, CjkType::None)
		.unwrap();
	assert_eq!(bare.cjk_type(), CjkType::None);
	assert!(!bare.fontdefs.contains_key("serif[cjk]"));
}
