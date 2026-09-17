use super::*;
use std::fs;
#[test]
fn external_edits_merge_pending_ui_fields_and_preserve_comments() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "# Reading\nfont_size = 20.0 # comfortable\nwidth = 800.0\n[extra]\nvalue = 42\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let mut ui = store.settings();
	ui.font_size = 24.0;
	store.changed(&ui, Some(Setting::FontSize));
	fs::write(&path, "# Reading\nfont_size = 21.0 # comfortable\nwidth = 900.0\n[extra]\nvalue = 42\n").unwrap();
	store.flush().unwrap();
	let text = fs::read_to_string(&path).unwrap();
	assert!(text.contains("# Reading"));
	assert!(text.contains("# comfortable"));
	assert!(text.contains("value = 42"));
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert_eq!(loaded.settings().font_size, 24.0);
	assert_eq!(loaded.settings().width, 900.0);
	assert!(!store.reload().unwrap());
}
#[test]
fn invalid_reload_and_deletion_retain_last_good_settings() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "font_size = 22\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	for invalid in [
		"font_size =",
		"font_size = 99",
		"width = nan",
		"paragraph_indent = 5",
		"theme = 'unknown'",
		"version = 2",
	] {
		fs::write(&path, invalid).unwrap();
		assert!(store.reload().is_err());
		assert_eq!(store.settings().font_size, 22.0);
	}
	let mut ui = store.settings();
	ui.justify = false;
	store.changed(&ui, Some(Setting::Justify));
	assert!(store.flush().is_err());
	assert_eq!(fs::read_to_string(&path).unwrap(), "version = 2");
	fs::remove_file(&path).unwrap();
	assert!(store.reload().is_err());
	fs::write(&path, "font_size = 26\n").unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().font_size, 26.0);
	assert!(!store.settings().justify);
}
#[test]
fn legacy_json_is_migrated_without_modifying_original() {
	let dir = tempfile::tempdir().unwrap();
	let legacy = dir.path().join("settings.json");
	let original = r#"{"version":1,"font_size":23,"theme":"dark"}"#;
	fs::write(&legacy, original).unwrap();
	let path = dir.path().join("settings.toml");
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	store.ensure_file().unwrap();
	assert_eq!(fs::read_to_string(legacy).unwrap(), original);
	assert_eq!(store.settings().font_size, 23.0);
	assert_eq!(store.theme_preference(), Some(Theme::Dark));
	assert!(SettingsStore::load(Some(path)).1.is_none());
	store.follow_system();
	store.flush().unwrap();
	assert_eq!(store.theme_preference(), None);
}
#[test]
fn toml_atomic_save_is_watched_and_applied() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	store.ensure_file().unwrap();
	let (tx, rx) = std::sync::mpsc::channel();
	let _watch = crate::watch::FileWatch::new(path.clone(), move || {
		let _ = tx.send(());
	});
	let replacement = dir.path().join("save.tmp");
	fs::write(&replacement, "font_size = 28\njustify = false\n").unwrap();
	fs::rename(replacement, path).unwrap();
	rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().font_size, 28.0);
	assert!(!store.settings().justify);
}
#[test]
fn overrides_do_not_leak_into_saved_fields() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.json");
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	let effective = ReaderSettings {
		font_size: 30.0,
		theme: Theme::Dark,
		..Default::default()
	};
	store.changed(&effective, Some(Setting::Theme));
	store.flush().unwrap();
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert_eq!(loaded.settings().font_size, 18.0);
	assert_eq!(loaded.settings().theme, Theme::Dark);
}
#[test]
fn theme_preference_is_optional_and_only_a_choice_pins_it() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.json");
	let (store, _) = SettingsStore::load(Some(path.clone()));
	assert_eq!(store.theme_preference(), None);
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	store.changed(
		&ReaderSettings {
			theme: Theme::Dark,
			..Default::default()
		},
		Some(Setting::Theme),
	);
	store.flush().unwrap();
	let (loaded, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(loaded.theme_preference(), Some(Theme::Dark));
	assert_eq!(loaded.settings().theme, Theme::Dark);
	// Another field must not turn the system theme into a pinned choice.
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	store.changed(
		&ReaderSettings {
			font_size: 24.0,
			theme: Theme::Dark,
			..Default::default()
		},
		Some(Setting::FontSize),
	);
	store.flush().unwrap();
	let (loaded, _) = SettingsStore::load(Some(path.clone()));
	assert_eq!(loaded.theme_preference(), Some(Theme::Dark));
	assert_eq!(loaded.settings().font_size, 24.0);
	// Reset returns to following the system theme.
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	store.changed(&ReaderSettings::default(), None);
	store.flush().unwrap();
	let (loaded, _) = SettingsStore::load(Some(path));
	assert_eq!(loaded.theme_preference(), None);
}
#[test]
fn paragraph_indent_round_trips_layout_and_bounds() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "paragraph_indent = 2.0\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(store.settings().paragraph_indent, 2.0);
	let mut ui = store.settings();
	ui.paragraph_indent = 1.5;
	store.changed(&ui, Some(Setting::ParagraphIndent));
	store.flush().unwrap();
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert_eq!(loaded.settings().paragraph_indent, 1.5);
	assert_eq!(
		loaded
			.settings()
			.layout_options(900.0, false)
			.paragraph_indent,
		1.5
	);
	for invalid in [-1.0, 4.5, f32::NAN] {
		let settings = ReaderSettings {
			paragraph_indent: invalid,
			..Default::default()
		};
		assert!(settings.validate().is_err(), "{invalid}");
	}
}

#[test]
fn corrupt_configuration_is_preserved_and_defaults_recover() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.json");
	fs::write(&path, b"broken json").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_some());
	assert_eq!(fs::read(&path).unwrap(), b"broken json");
	store.changed(&ReaderSettings::default(), None);
	store.flush().unwrap();
	assert!(
		fs::read_dir(dir.path())
			.unwrap()
			.any(|p| fs::read(p.unwrap().path()).unwrap() == b"broken json")
	);
	assert!(SettingsStore::load(Some(path)).1.is_none());
}

#[test]
fn justification_limits_round_trip_layout_and_bound() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(
		&path,
		"[justification]\nspacing_min = 0.5\nspacing_max = 2.0\ntracking_min = 0.0\ntracking_max = 0.0\n",
	)
	.unwrap();
	let (store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let limits = store.settings().justification;
	assert_eq!(limits.spacing_min, 0.5);
	assert_eq!(limits.spacing_max, 2.0);
	assert_eq!(limits.tracking_max, 0.0);
	// The file reaches layout, and the CJK convention with it, which travels
	// inside the stylesheet because that is what picks the `[cjk]` font.
	let options = store.settings().layout_options(900.0, false);
	assert_eq!(options.justification, limits);
	assert_eq!(options.stylesheet.cjk_type(), store.settings().cjk_type);

	// A partial table keeps the defaults for what it leaves out.
	fs::write(&path, "[justification]\nspacing_max = 2.0\n").unwrap();
	let (store, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	let limits = store.settings().justification;
	assert_eq!(limits.spacing_max, 2.0);

	for invalid in [
		JustificationLimits {
			spacing_min: 0.0,
			..Default::default()
		},
		JustificationLimits {
			spacing_min: 2.0,
			spacing_max: 1.0,
			..Default::default()
		},
		JustificationLimits {
			tracking_min: 0.5,
			..Default::default()
		},
		JustificationLimits {
			tracking_max: -0.5,
			..Default::default()
		},
		JustificationLimits {
			spacing_max: f32::NAN,
			..Default::default()
		},
	] {
		assert!(!invalid.is_valid(), "{invalid:?}");
		let settings = ReaderSettings {
			justification: invalid,
			..Default::default()
		};
		assert!(settings.validate().is_err(), "{invalid:?}");
	}
}
