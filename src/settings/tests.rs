use super::store::Config;
use super::*;
use crate::lang::Lang;
use std::fs;

#[test]
fn sidebar_widths_default_merge_reload_validate_and_reset_independently() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "# Reader\nfont_size = 20\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let defaults = store.settings();
	assert_eq!(
		(
			defaults.folder_width,
			defaults.outline_width,
			defaults.tabs_width
		),
		(260., 300., 320.)
	);
	let mut settings = defaults;
	settings.set_sidebar_width(Sidebar::Folder, 380.);
	store.changed(&settings, Some(Setting::SidebarWidth(Sidebar::Folder)));
	fs::write(&path, "# Reader\noutline-width = 420\ntabs-width = 460\n")
		.unwrap();
	store.flush().unwrap();
	let (loaded, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let settings = loaded.settings();
	assert_eq!(
		(
			settings.folder_width,
			settings.outline_width,
			settings.tabs_width
		),
		(380., 420., 460.)
	);
	assert!(fs::read_to_string(&path).unwrap().contains("# Reader"));
	for key in ["folder-width", "outline-width", "tabs-width"] {
		for invalid in ["199", "601", "nan", "inf"] {
			fs::write(&path, format!("{key} = {invalid}\n")).unwrap();
			assert!(store.reload().is_err());
			assert_eq!(store.settings(), settings);
		}
	}
	fs::write(
		&path,
		"folder-width = 600\noutline-width = 200\ntabs-width = 500\n",
	)
	.unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().folder_width, 600.);
	store.changed(&ReaderSettings::default(), None);
	store.flush().unwrap();
	assert_eq!(
		SettingsStore::load(Some(path)).0.settings(),
		ReaderSettings::default()
	);
}

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
fn tab_style_round_trips_reloads_and_merges_pending_changes() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "# Reader\nfont_size = 20\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(store.settings().tab_style, TabStyle::Underline);
	for style in [TabStyle::Connected, TabStyle::Underline] {
		let mut settings = store.settings();
		settings.tab_style = style;
		store.changed(&settings, Some(Setting::TabStyle));
		fs::write(&path, "# Reader\nfont_size = 24\n").unwrap();
		store.flush().unwrap();
		let (loaded, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_none());
		assert_eq!(loaded.settings().tab_style, style);
		assert_eq!(loaded.settings().font_size, 24.0);
		assert_eq!(loaded.theme_preference(), None);
		assert!(fs::read_to_string(&path).unwrap().contains("# Reader"));
	}
	fs::write(&path, "tab-style = 'connected'\n").unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().tab_style, TabStyle::Connected);
	fs::write(&path, "tab-style = 'unknown'\n").unwrap();
	assert!(store.reload().is_err());
	assert_eq!(store.settings().tab_style, TabStyle::Connected);
	fs::write(&path, "tab-style = 'connected'\n").unwrap();
	store.changed(&ReaderSettings::default(), None);
	store.flush().unwrap();
	assert_eq!(
		SettingsStore::load(Some(path)).0.settings().tab_style,
		TabStyle::Underline
	);
}

#[test]
fn window_layout_round_trips_and_merges_without_pinning_the_theme() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "# Reader\nfont_size = 20\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(store.settings().window_layout, WindowLayout::System);
	for layout in [
		WindowLayout::Macos,
		WindowLayout::Windows,
		WindowLayout::Linux,
		WindowLayout::System,
	] {
		let mut settings = store.settings();
		settings.window_layout = layout;
		store.changed(&settings, Some(Setting::WindowLayout));
		fs::write(&path, "# Reader\nfont_size = 24\n[extra]\nvalue = 42\n")
			.unwrap();
		store.flush().unwrap();
		let (loaded, warning) = SettingsStore::load(Some(path.clone()));
		assert!(warning.is_none());
		assert_eq!(loaded.settings().window_layout, layout);
		assert_eq!(loaded.settings().font_size, 24.0);
		assert_eq!(loaded.theme_preference(), None);
		assert!(fs::read_to_string(&path).unwrap().contains("# Reader"));
	}
	let mut settings = store.settings();
	settings.window_layout = WindowLayout::Windows;
	store.changed(&settings, Some(Setting::WindowLayout));
	store.flush().unwrap();
	store.changed(&ReaderSettings::default(), None);
	store.flush().unwrap();
	assert_eq!(
		SettingsStore::load(Some(path)).0.settings().window_layout,
		WindowLayout::System
	);
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
		"window-layout = 'unknown'",
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
			.layout_options(900.0, false, &crate::test_support::fonts())
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
fn codeblock_wrap_round_trips_settings_and_layout() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "codeblock-wrap = true\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert!(store.settings().codeblock_wrap);
	assert!(
		store
			.settings()
			.layout_options(900.0, false, &crate::test_support::fonts())
			.codeblock_wrap
	);
	let mut ui = store.settings();
	ui.codeblock_wrap = false;
	store.changed(&ui, Some(Setting::CodeblockWrap));
	store.flush().unwrap();
	assert!(
		fs::read_to_string(&path)
			.unwrap()
			.contains("codeblock-wrap")
	);
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert!(!loaded.settings().codeblock_wrap);
	assert!(
		!loaded
			.settings()
			.layout_options(900.0, false, &crate::test_support::fonts())
			.codeblock_wrap
	);
}

/// The dialect a parse uses follows the preset: GFM keeps the stored toggles,
/// while OFM lite forces highlight on and hard breaks off whatever is stored.
#[test]
fn markdown_dialect_defaults_and_ofm_lite_forces_its_toggles() {
	let settings = ReaderSettings::default();
	assert_eq!(settings.markdown_flavor, MarkdownFlavor::Gfm);
	let dialect = settings.effective_dialect();
	assert!(!dialect.hard_breaks);
	assert!(!dialect.highlight_marks);
	let mut settings = settings;
	settings.hard_breaks = true;
	settings.highlight_marks = true;
	let dialect = settings.effective_dialect();
	assert!(dialect.hard_breaks);
	assert!(dialect.highlight_marks);
	settings.markdown_flavor = MarkdownFlavor::OfmLite;
	let dialect = settings.effective_dialect();
	assert!(!dialect.hard_breaks);
	assert!(dialect.highlight_marks);
}

/// End-to-end fixture matrix for the four reading cases ticket 05 checks.
///
/// One source carries both a soft newline and a `==mark==`, so each case is
/// the same bytes under a different `ReaderSettings` → `effective_dialect()`
/// → parse path. Wikilinks stay out of scope.
#[test]
fn dialect_fixture_matrix_covers_the_four_reading_cases() {
	use markview_commonmark::{Dialect, parse_with};
	use markview_document::{BlockKind, InlineKind, plain_text};

	const FIXTURE: &str = "line one\nline two and ==marked== text\n";

	fn paragraph(
		doc: &markview_document::Document,
	) -> &markview_document::RichText {
		let BlockKind::Paragraph(rich) = &doc.blocks[0].kind else {
			panic!("expected a paragraph");
		};
		rich
	}
	fn parse_settings(
		settings: &ReaderSettings,
	) -> markview_document::Document {
		let dialect = settings.effective_dialect();
		parse_with(
			FIXTURE,
			Dialect {
				hard_breaks: dialect.hard_breaks,
				highlight_marks: dialect.highlight_marks,
			},
		)
	}
	fn has_hard_break(rich: &markview_document::RichText) -> bool {
		rich.iter()
			.any(|i| matches!(i.kind, InlineKind::LineBreak { .. }))
	}
	fn has_mark(rich: &markview_document::RichText) -> bool {
		rich.iter().any(|i| i.style.mark)
	}

	// 1) GFM default: soft break → space, `==` stays literal.
	let gfm = ReaderSettings::default();
	assert_eq!(gfm.markdown_flavor, MarkdownFlavor::Gfm);
	assert!(!gfm.hard_breaks && !gfm.highlight_marks);
	let doc = parse_settings(&gfm);
	let rich = paragraph(&doc);
	assert_eq!(plain_text(rich), "line one line two and ==marked== text");
	assert!(!has_hard_break(rich));
	assert!(!has_mark(rich));

	// 2) GFM + hard breaks: the same newline ends the line.
	let gfm_hard = ReaderSettings {
		hard_breaks: true,
		..Default::default()
	};
	let doc = parse_settings(&gfm_hard);
	let rich = paragraph(&doc);
	assert_eq!(plain_text(rich), "line one\nline two and ==marked== text");
	assert!(has_hard_break(rich));
	assert!(!has_mark(rich));

	// 3) GFM + highlight: `==marked==` becomes a mark; soft breaks stay spaces.
	let gfm_mark = ReaderSettings {
		highlight_marks: true,
		..Default::default()
	};
	let doc = parse_settings(&gfm_mark);
	let rich = paragraph(&doc);
	assert_eq!(plain_text(rich), "line one line two and marked text");
	assert!(!has_hard_break(rich));
	assert!(has_mark(rich));

	// 4) OFM lite: highlight on and hard breaks off even when stored toggles
	// say otherwise (the panel hides those toggles under this preset).
	let mut ofm = ReaderSettings {
		markdown_flavor: MarkdownFlavor::OfmLite,
		hard_breaks: true,
		highlight_marks: false,
		..Default::default()
	};
	let dialect = ofm.effective_dialect();
	assert!(!dialect.hard_breaks);
	assert!(dialect.highlight_marks);
	let doc = parse_settings(&ofm);
	let rich = paragraph(&doc);
	assert_eq!(plain_text(rich), "line one line two and marked text");
	assert!(!has_hard_break(rich));
	assert!(has_mark(rich));

	// Switching the stored flavor back to GFM restores the stored toggles.
	ofm.markdown_flavor = MarkdownFlavor::Gfm;
	let dialect = ofm.effective_dialect();
	assert!(dialect.hard_breaks);
	assert!(!dialect.highlight_marks);
}

/// The parse the layout options describe is the effective dialect: OFM lite
/// keeps a stored toggle from ever reaching a parse.
#[test]
fn layout_options_carry_the_effective_dialect() {
	let fonts = crate::test_support::fonts();
	let mut settings = ReaderSettings::default();
	let options = |settings: &ReaderSettings| {
		settings.layout_options(900.0, false, &fonts)
	};
	assert!(!options(&settings).hard_breaks);
	assert!(!options(&settings).highlight_marks);
	settings.hard_breaks = true;
	settings.highlight_marks = true;
	assert!(options(&settings).hard_breaks);
	assert!(options(&settings).highlight_marks);
	settings.markdown_flavor = MarkdownFlavor::OfmLite;
	assert!(!options(&settings).hard_breaks);
	assert!(options(&settings).highlight_marks);
}

#[test]
fn markdown_dialect_round_trips_and_old_files_still_load() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	// A file from before the dialect existed keeps today's reading behavior.
	fs::write(&path, "font_size = 20\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(store.settings().markdown_flavor, MarkdownFlavor::Gfm);
	let dialect = store.settings().effective_dialect();
	assert!(!dialect.hard_breaks);
	assert!(!dialect.highlight_marks);

	let mut ui = store.settings();
	ui.markdown_flavor = MarkdownFlavor::OfmLite;
	// The stored toggle survives the preset, ready for a switch back to GFM.
	ui.hard_breaks = true;
	store.changed(&ui, Some(Setting::Dialect));
	store.flush().unwrap();
	let text = fs::read_to_string(&path).unwrap();
	assert!(text.contains("markdown-flavor"), "{text}");
	assert!(text.contains("hard-breaks = true"), "{text}");
	let (loaded, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let settings = loaded.settings();
	assert_eq!(settings.markdown_flavor, MarkdownFlavor::OfmLite);
	assert!(settings.hard_breaks);
	let dialect = settings.effective_dialect();
	assert!(!dialect.hard_breaks);
	assert!(dialect.highlight_marks);
	// A flavor the reader does not know is rejected, like any other enum.
	fs::write(&path, "markdown-flavor = 'commonmark'\n").unwrap();
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_some());
	assert_eq!(loaded.settings().markdown_flavor, MarkdownFlavor::Gfm);
}

#[test]
fn footer_path_defaults_off_and_round_trips() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert!(!store.settings().footer_path);
	let mut settings = store.settings();
	settings.footer_path = true;
	store.changed(&settings, Some(Setting::FooterPath));
	store.flush().unwrap();
	assert!(
		fs::read_to_string(&path)
			.unwrap()
			.contains("footer-path = true")
	);
	let (mut loaded, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert!(loaded.settings().footer_path);
	loaded.changed(&ReaderSettings::default(), None);
	loaded.flush().unwrap();
	assert!(!SettingsStore::load(Some(path)).0.settings().footer_path);
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
	let options = store.settings().layout_options(
		900.0,
		false,
		&crate::test_support::fonts(),
	);
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

#[test]
fn export_settings_round_trip_and_leave_the_reader_alone() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "font_size = 20.0\n").unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(store.export(), ExportSettings::default());
	let export = ExportSettings {
		format: ExportFormat::Png,
		font_size: 24.0,
		paper: "letter".into(),
		scale: 1.0,
		style: vec!["print".into(), "dark".into()],
		..Default::default()
	};
	store.set_export(export.clone());
	store.flush().unwrap();
	let text = fs::read_to_string(&path).unwrap();
	assert!(text.contains("[export]"), "{text}");
	assert!(text.contains("style = [\"print\", \"dark\"]"), "{text}");
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert_eq!(loaded.export(), export);
	// The reading view kept its own size.
	assert_eq!(loaded.settings().font_size, 20.0);
}

#[test]
fn a_bad_export_table_only_resets_the_export() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "font_size = 21.0\n[export]\nfont_size = 201.0\n")
		.unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert_eq!(store.settings().font_size, 21.0);
	assert_eq!(store.export(), ExportSettings::default());
	let warning = warning.expect("export warning");
	let english = warning.text(Lang::En);
	assert!(english.starts_with("Export settings:"), "{english}");
	assert!(english.ends_with("; using defaults"), "{english}");
	// The same failure reads in the reader's own language, which is why the
	// store keeps it as a failure instead of as text.
	let chinese = warning.text(Lang::ZhHans);
	assert!(chinese.starts_with("导出设置："), "{chinese}");
	assert!(chinese.ends_with("；改用默认值"), "{chinese}");
	// A live reload keeps the last good values when the table is invalid.
	fs::write(&path, "font_size = 21.0\n[export]\npaper = 'nonsense'\n")
		.unwrap();
	assert!(store.reload().is_err());
	assert_eq!(store.settings().font_size, 21.0);
}

#[test]
fn a_pending_export_edit_survives_an_external_reload() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	fs::write(&path, "font_size = 20.0\n").unwrap();
	let (mut store, _) = SettingsStore::load(Some(path.clone()));
	let export = ExportSettings {
		scale: 1.0,
		..Default::default()
	};
	store.set_export(export.clone());
	// An external writer changes an unrelated reader field.
	fs::write(&path, "font_size = 22.0\n").unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().font_size, 22.0);
	assert_eq!(store.export(), export);
}

#[test]
fn an_export_defaults_to_twelve_point_body_text() {
	let export = ExportSettings::default();
	assert_eq!(export.format, ExportFormat::Pdf);
	assert_eq!(
		export.font_size * markview_layout::paginate::PT_PER_PX,
		12.0
	);
}

#[test]
fn scroll_speed_round_trips_and_stays_in_range() {
	let mut settings = ReaderSettings::default();
	assert_eq!(settings.scroll_speed, 1.0);
	// The buttons step a quarter at a time and clamp at both ends.
	for _ in 0..40 {
		settings.step_scroll_speed(1);
	}
	assert_eq!(settings.scroll_speed, SCROLL_SPEED_MAX);
	for _ in 0..40 {
		settings.step_scroll_speed(-1);
	}
	assert_eq!(settings.scroll_speed, SCROLL_SPEED_MIN);

	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	let mut settings = ReaderSettings::default();
	settings.step_scroll_speed(2);
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	store.changed(&settings, Some(Setting::ScrollSpeed));
	store.flush().unwrap();
	assert!(
		fs::read_to_string(&path)
			.unwrap()
			.contains("scroll-speed = 1.5")
	);
	let (loaded, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(loaded.settings().scroll_speed, 1.5);
	// A speed outside the range is rejected and the defaults recover.
	fs::write(&path, "scroll-speed = 4.0\n").unwrap();
	let (loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_some());
	assert_eq!(loaded.settings().scroll_speed, 1.0);
}

/// A family picked for one role is one override in the file, so it survives a
/// restart, and picking the default entry takes it back out.
#[test]
fn a_font_family_round_trips_and_the_default_removes_the_override() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let mut settings = store.settings();
	assert_eq!(settings.font_family(FontRole::Serif), None);
	settings.set_font_family(FontRole::Serif, Some("Noto Sans".to_owned()));
	// One role's pick leaves the other roles on the stylesheet's own chain.
	assert_eq!(settings.font_family(FontRole::Monospace), None);
	store.changed(&settings, Some(Setting::FontFamily));
	store.flush().unwrap();

	let (mut observer, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let loaded = observer.settings();
	assert_eq!(loaded.font_family(FontRole::Serif), Some("Noto Sans"));
	assert_eq!(loaded.font_family(FontRole::Monospace), None);
	// The override reaches the stylesheet in force, replacing that role's own
	// candidate chain and nothing else.
	let sheet = crate::stylesheet::apply_font_overrides(
		loaded.stylesheet.clone(),
		&loaded.fontdef_overrides,
	)
	.unwrap();
	assert_eq!(
		sheet.fontdefs["serif"].lookfor,
		vec!["Noto Sans".to_owned()]
	);
	assert_ne!(
		sheet.fontdefs["monospace"].lookfor,
		vec!["Noto Sans".to_owned()]
	);

	let mut reset = loaded.clone();
	reset.set_font_family(FontRole::Serif, None);
	assert_eq!(reset.font_family(FontRole::Serif), None);
	assert!(reset.fontdef_overrides.is_empty());
	store.changed(&reset, Some(Setting::FontFamily));
	store.flush().unwrap();
	assert!(
		!fs::read_to_string(&path)
			.unwrap()
			.contains("fontdef-override")
	);
	assert!(observer.reload().unwrap());
	assert!(observer.settings().fontdef_overrides.is_empty());
	let (restarted, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert!(restarted.settings().fontdef_overrides.is_empty());
	assert_eq!(
		crate::stylesheet::apply_font_overrides(
			reset.stylesheet.clone(),
			&reset.fontdef_overrides
		)
		.unwrap()
		.fontdefs["serif"]
			.lookfor,
		loaded.stylesheet.fontdefs["serif"].lookfor
	);
}

/// A Han role names one definition whatever CJK variant is in force, so a
/// family picked for it follows a change of variant.
#[test]
fn a_han_family_follows_a_change_of_cjk_variant() {
	let mut settings = ReaderSettings::default();
	settings.set_font_family(
		FontRole::SerifHan,
		Some("Noto Serif CJK SC".to_owned()),
	);
	for cjk_type in [CjkType::Sc, CjkType::Tc, CjkType::Jp] {
		let mut sheet = (*settings.stylesheet).clone();
		sheet.set_cjk_type(cjk_type);
		let sheet = crate::stylesheet::apply_font_overrides(
			std::sync::Arc::new(sheet),
			&settings.fontdef_overrides,
		)
		.unwrap();
		assert_eq!(
			sheet.fontdefs["serif[cjk]"].lookfor,
			vec!["Noto Serif CJK SC".to_owned()],
			"{cjk_type:?}"
		);
	}
}

#[test]
fn single_instance_platform_default_and_settings_merge() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(
		store.settings().single_instance,
		markview_platform::touch_layout()
	);
	store.ensure_file().unwrap();
	let mut settings = store.settings();
	settings.single_instance = true;
	store.changed(&settings, Some(Setting::SingleInstance));
	fs::write(&path, "width = 900\nsingle-instance = false\n").unwrap();
	store.flush().unwrap();
	let (mut loaded, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none());
	assert!(loaded.settings().single_instance);
	assert_eq!(loaded.settings().width, 900.0);
	loaded.changed(&ReaderSettings::default(), None);
	loaded.flush().unwrap();
	assert_eq!(
		loaded.settings().single_instance,
		markview_platform::touch_layout()
	);
}

#[test]
fn restore_session_defaults_legacy_conflicts_reload_and_save() {
	let defaults = ReaderSettings::default();
	assert_eq!(defaults.restore_session, markview_platform::touch_layout());
	assert_eq!(defaults.single_instance, markview_platform::touch_layout());
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	std::fs::write(
		&path,
		"version = 1\nsingle-instance = false\nrestore-session = true\n",
	)
	.unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert!(
		store.settings().restore_session && store.settings().single_instance
	);
	store.changed(&store.settings(), Some(Setting::RestoreSession));
	store.flush().unwrap();
	assert!(
		std::fs::read_to_string(&path)
			.unwrap()
			.contains("single-instance = true")
	);
	std::fs::write(
		&path,
		"version = 1\nsingle-instance = false\nrestore-session = true\nwidth = 500\n",
	)
	.unwrap();
	assert!(store.reload().unwrap());
	assert!(store.settings().single_instance);
	std::fs::write(&path, "version = 1\n").unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().restore_session, defaults.restore_session);
	store.changed(&defaults, None);
	store.flush().unwrap();
	assert_eq!(
		SettingsStore::load(Some(path)).0.settings().restore_session,
		defaults.restore_session
	);
}

#[test]
fn http_overrides_survive_ui_saves_and_external_reload() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("settings.toml");
	let config = "user-agent = 'Custom/1' # browser\naccept-language = 'de,en;q=0.8'\nlanguage = 'ja'\n";
	fs::write(&path, config).unwrap();
	let (mut store, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	let mut settings = store.settings();
	assert_eq!(settings.user_agent.as_deref(), Some("Custom/1"));
	assert_eq!(settings.accept_language.as_deref(), Some("de,en;q=0.8"));
	settings.font_size = 24.0;
	store.changed(&settings, Some(Setting::FontSize));
	store.flush().unwrap();
	let saved = fs::read_to_string(&path).unwrap();
	assert!(saved.contains("# browser"));
	let (loaded, warning) = SettingsStore::load(Some(path.clone()));
	assert!(warning.is_none());
	assert_eq!(loaded.settings().user_agent, settings.user_agent);
	assert_eq!(loaded.settings().accept_language, settings.accept_language);
	fs::write(&path, "user-agent = 'Custom/2'\n").unwrap();
	assert!(store.reload().unwrap());
	assert_eq!(store.settings().user_agent.as_deref(), Some("Custom/2"));
	assert!(store.settings().accept_language.is_none());
	fs::write(&path, "user-agent = \"bad\\nheader\"\n").unwrap();
	assert!(store.reload().is_err());
	assert_eq!(store.settings().user_agent.as_deref(), Some("Custom/2"));
	fs::write(&path, "user-agent = 'Custom/2'\n").unwrap();
	store.changed(&ReaderSettings::default(), None);
	store.flush().unwrap();
	let saved = fs::read_to_string(&path).unwrap();
	assert!(!saved.contains("user-agent ="));
	assert!(!saved.contains("accept-language ="));
}

fn settings_differing_from_default() -> ReaderSettings {
	let default = ReaderSettings::default();
	ReaderSettings {
		window_layout: WindowLayout::Linux,
		tab_style: TabStyle::Connected,
		theme: Theme::Dark,
		style: Some(vec!["dark".into()]),
		lang: Some(Lang::Ja),
		user_agent: Some("Fixture/1".into()),
		accept_language: Some("ja,en;q=0.8".into()),
		fontdef_overrides: vec![FontDefOverride {
			id: "serif".into(),
			replacement: "Fixture Serif".into(),
		}],
		font_size: 21.5,
		width: 888.0,
		folder_width: 321.0,
		outline_width: 333.0,
		tabs_width: 345.0,
		justify: !default.justify,
		hyphenate: !default.hyphenate,
		justification: JustificationLimits {
			spacing_min: 0.5,
			spacing_max: 2.0,
			tracking_min: -0.02,
			tracking_max: 0.02,
		},
		paragraph_indent: 1.5,
		cjk_type: CjkType::None,
		codeblock_theme_override: Some("fixture".into()),
		codeblock_wrap: !default.codeblock_wrap,
		markdown_flavor: MarkdownFlavor::OfmLite,
		hard_breaks: !default.hard_breaks,
		highlight_marks: !default.highlight_marks,
		scroll_speed: 1.5,
		single_instance: !default.single_instance,
		restore_session: !default.restore_session,
		footer_path: !default.footer_path,
		..default
	}
}

fn changed_fields(
	before: &ReaderSettings,
	after: &ReaderSettings,
) -> Vec<&'static str> {
	let mut changed = Vec::new();
	macro_rules! changed {
		($($field:ident),* $(,)?) => {
			$(
				if before.$field != after.$field {
					changed.push(stringify!($field));
				}
			)*
		};
	}
	changed!(
		window_layout,
		tab_style,
		theme,
		style,
		lang,
		user_agent,
		accept_language,
		fontdef_overrides,
		stylesheet,
		font_size,
		width,
		folder_width,
		outline_width,
		tabs_width,
		justify,
		hyphenate,
		justification,
		paragraph_indent,
		cjk_type,
		codeblock_theme_override,
		codeblock_wrap,
		markdown_flavor,
		hard_breaks,
		highlight_marks,
		scroll_speed,
		single_instance,
		restore_session,
		footer_path,
	);
	changed
}

fn expected_changes(field: Setting) -> Vec<&'static str> {
	match field {
		Setting::WindowLayout => vec!["window_layout"],
		Setting::TabStyle => vec!["tab_style"],
		Setting::Theme => vec!["theme", "style"],
		Setting::FontSize => vec!["font_size"],
		Setting::Width => vec!["width"],
		Setting::SidebarWidth(sidebar) => vec![match sidebar {
			Sidebar::Folder => "folder_width",
			Sidebar::Outline => "outline_width",
			Sidebar::Tabs => "tabs_width",
		}],
		Setting::Justify => vec!["justify"],
		Setting::Hyphenate => vec!["hyphenate"],
		Setting::ParagraphIndent => vec!["paragraph_indent"],
		Setting::CjkType => vec!["cjk_type"],
		Setting::Language => vec!["lang"],
		Setting::CodeblockWrap => vec!["codeblock_wrap"],
		Setting::Dialect => {
			vec!["markdown_flavor", "hard_breaks", "highlight_marks"]
		}
		Setting::ScrollSpeed => vec!["scroll_speed"],
		Setting::SingleInstance => vec!["single_instance"],
		Setting::RestoreSession => vec!["restore_session"],
		Setting::FooterPath => vec!["footer_path"],
		Setting::FontFamily => vec!["fontdef_overrides"],
	}
}

fn assert_copied(from: &ReaderSettings, to: &ReaderSettings, field: Setting) {
	match field {
		Setting::WindowLayout => {
			assert_eq!(to.window_layout, from.window_layout)
		}
		Setting::TabStyle => assert_eq!(to.tab_style, from.tab_style),
		Setting::Theme => {
			assert_eq!(to.theme, from.theme);
			assert_eq!(to.style, from.style);
		}
		Setting::FontSize => assert_eq!(to.font_size, from.font_size),
		Setting::Width => assert_eq!(to.width, from.width),
		Setting::SidebarWidth(sidebar) => {
			assert_eq!(to.sidebar_width(sidebar), from.sidebar_width(sidebar));
		}
		Setting::Justify => assert_eq!(to.justify, from.justify),
		Setting::Hyphenate => assert_eq!(to.hyphenate, from.hyphenate),
		Setting::ParagraphIndent => {
			assert_eq!(to.paragraph_indent, from.paragraph_indent);
		}
		Setting::CjkType => assert_eq!(to.cjk_type, from.cjk_type),
		Setting::Language => assert_eq!(to.lang, from.lang),
		Setting::CodeblockWrap => {
			assert_eq!(to.codeblock_wrap, from.codeblock_wrap);
		}
		Setting::Dialect => {
			assert_eq!(to.markdown_flavor, from.markdown_flavor);
			assert_eq!(to.hard_breaks, from.hard_breaks);
			assert_eq!(to.highlight_marks, from.highlight_marks);
		}
		Setting::ScrollSpeed => assert_eq!(to.scroll_speed, from.scroll_speed),
		Setting::SingleInstance => {
			assert_eq!(to.single_instance, from.single_instance);
		}
		Setting::RestoreSession => {
			assert_eq!(to.restore_session, from.restore_session);
		}
		Setting::FooterPath => assert_eq!(to.footer_path, from.footer_path),
		Setting::FontFamily => {
			assert_eq!(to.fontdef_overrides, from.fontdef_overrides);
		}
	}
}

#[test]
fn setting_all_keeps_the_persisted_order_and_count() {
	assert_eq!(Setting::COUNT, 18);
	assert_eq!(
		Setting::ALL,
		[
			Setting::WindowLayout,
			Setting::TabStyle,
			Setting::Theme,
			Setting::FontSize,
			Setting::Width,
			Setting::SidebarWidth(Sidebar::Folder),
			Setting::SidebarWidth(Sidebar::Outline),
			Setting::SidebarWidth(Sidebar::Tabs),
			Setting::Justify,
			Setting::Hyphenate,
			Setting::ParagraphIndent,
			Setting::CjkType,
			Setting::Language,
			Setting::CodeblockWrap,
			Setting::Dialect,
			Setting::ScrollSpeed,
			Setting::SingleInstance,
			Setting::RestoreSession,
			Setting::FooterPath,
			Setting::FontFamily,
		]
	);
}

#[test]
fn copy_field_changes_only_the_named_setting() {
	let before = ReaderSettings::default();
	let other = settings_differing_from_default();
	for field in Setting::ALL {
		let mut after = before.clone();
		after.copy_field(&other, field);
		assert_eq!(
			changed_fields(&before, &after),
			expected_changes(field),
			"{field:?}"
		);
		assert_copied(&other, &after, field);
	}
}

#[test]
fn config_defaults_match_reader_defaults() {
	assert_eq!(
		Config::default().reader_settings(),
		ReaderSettings::default()
	);
}

#[test]
fn reader_settings_round_trip_through_config_for_non_option_fields() {
	let settings = settings_differing_from_default();
	let back = Config::from_settings(&settings, ExportSettings::default())
		.reader_settings();
	assert_eq!(back.window_layout, settings.window_layout);
	assert_eq!(back.tab_style, settings.tab_style);
	assert_eq!(back.theme, settings.theme);
	assert_eq!(back.fontdef_overrides, settings.fontdef_overrides);
	assert_eq!(back.stylesheet, settings.stylesheet);
	assert_eq!(back.font_size, settings.font_size);
	assert_eq!(back.width, settings.width);
	assert_eq!(back.folder_width, settings.folder_width);
	assert_eq!(back.outline_width, settings.outline_width);
	assert_eq!(back.tabs_width, settings.tabs_width);
	assert_eq!(back.justify, settings.justify);
	assert_eq!(back.hyphenate, settings.hyphenate);
	assert_eq!(back.justification, settings.justification);
	assert_eq!(back.paragraph_indent, settings.paragraph_indent);
	assert_eq!(back.cjk_type, settings.cjk_type);
	assert_eq!(back.codeblock_wrap, settings.codeblock_wrap);
	assert_eq!(back.scroll_speed, settings.scroll_speed);
	assert_eq!(back.single_instance, settings.single_instance);
	assert_eq!(back.restore_session, settings.restore_session);
	assert_eq!(back.footer_path, settings.footer_path);
}

#[test]
fn version_one_fixture_honours_every_persisted_key() {
	let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/settings-v1.toml");
	let source = fs::read_to_string(&path).unwrap();
	let raw: Config = toml_edit::de::from_str(&source).unwrap();
	assert_eq!(raw.version, 1);
	let (store, warning) = SettingsStore::load(Some(path));
	assert!(warning.is_none(), "{warning:?}");
	let settings = store.settings();
	assert_eq!(settings.window_layout, WindowLayout::Macos);
	assert_eq!(settings.tab_style, TabStyle::Connected);
	assert_eq!(settings.theme, Theme::Dark);
	assert_eq!(settings.style, Some(vec!["dark".into()]));
	assert_eq!(store.theme_preference(), Some(Theme::Dark));
	assert_eq!(settings.font_size, 21.0);
	assert_eq!(settings.width, 900.0);
	assert_eq!(settings.folder_width, 340.0);
	assert_eq!(settings.outline_width, 360.0);
	assert_eq!(settings.tabs_width, 380.0);
	assert!(!settings.justify);
	assert!(!settings.hyphenate);
	assert_eq!(settings.paragraph_indent, 1.5);
	assert_eq!(settings.cjk_type, CjkType::Jp);
	assert_eq!(settings.lang, Some(Lang::Ja));
	assert_eq!(settings.user_agent.as_deref(), Some("MarkviewFixture/1"));
	assert_eq!(settings.accept_language.as_deref(), Some("ja,en;q=0.8"));
	assert_eq!(
		settings.codeblock_theme_override.as_deref(),
		Some("fixture-code")
	);
	assert!(settings.codeblock_wrap);
	assert_eq!(settings.scroll_speed, 1.5);
	assert!(settings.single_instance);
	assert!(settings.restore_session);
	assert!(settings.footer_path);
	assert_eq!(
		settings.fontdef_overrides,
		vec![FontDefOverride {
			id: "serif".into(),
			replacement: "Fixture Serif".into(),
		}]
	);
	assert_eq!(settings.font_family(FontRole::Serif), Some("Fixture Serif"));
	assert_eq!(settings.justification.spacing_min, 0.5);
	assert_eq!(settings.justification.spacing_max, 2.0);
	assert_eq!(settings.justification.tracking_min, -0.02);
	assert_eq!(settings.justification.tracking_max, 0.02);
	let export = store.export();
	assert_eq!(export.format, ExportFormat::Png);
	assert_eq!(export.font_size, 24.0);
	assert_eq!(export.paragraph_indent, 1.0);
	assert_eq!(export.paper, "letter");
	assert!(export.landscape);
	assert_eq!(export.margin, [10.0, 11.0, 12.0, 13.0]);
	assert_eq!(export.scale, 1.0);
	assert_eq!(export.style, vec!["print".to_owned(), "dark".to_owned()]);
}
