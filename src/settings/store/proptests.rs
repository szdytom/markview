use super::*;
use crate::settings::Sidebar;
use proptest::prelude::*;

const FIELDS: [Setting; 12] = [
	Setting::FontSize,
	Setting::Width,
	Setting::SidebarWidth(Sidebar::Folder),
	Setting::SidebarWidth(Sidebar::Outline),
	Setting::SidebarWidth(Sidebar::Tabs),
	Setting::ParagraphIndent,
	Setting::ScrollSpeed,
	Setting::Justify,
	Setting::Hyphenate,
	Setting::CodeblockWrap,
	Setting::FooterPath,
	Setting::CodeblockCopy,
];
const KEYS: [&str; 12] = [
	"font_size",
	"width",
	"folder-width",
	"outline-width",
	"tabs-width",
	"paragraph_indent",
	"scroll-speed",
	"justify",
	"hyphenate",
	"codeblock-wrap",
	"footer-path",
	"codeblock-copy",
];

fn values(seed: u16) -> [f32; 12] {
	let flag = |bit: u32| f32::from(u8::from(seed & (1u16 << bit) != 0u16));
	[
		10.0 + f32::from(seed % 31),
		240.0 + f32::from(seed % 1361),
		200.0 + f32::from(seed % 401),
		200.0 + f32::from(seed.wrapping_add(67) % 401),
		200.0 + f32::from(seed.wrapping_add(131) % 401),
		f32::from(seed % 17) / 4.0,
		f32::from(2 + seed % 7) / 4.0,
		flag(0),
		flag(1),
		flag(2),
		flag(3),
		flag(4),
	]
}

fn reader(values: [f32; 12]) -> ReaderSettings {
	ReaderSettings {
		font_size: values[0],
		width: values[1],
		folder_width: values[2],
		outline_width: values[3],
		tabs_width: values[4],
		paragraph_indent: values[5],
		scroll_speed: values[6],
		justify: values[7] != 0.0,
		hyphenate: values[8] != 0.0,
		codeblock_wrap: values[9] != 0.0,
		footer_path: values[10] != 0.0,
		codeblock_copy: values[11] != 0.0,
		..Default::default()
	}
}

fn observed(settings: &ReaderSettings) -> [f32; 12] {
	[
		settings.font_size,
		settings.width,
		settings.folder_width,
		settings.outline_width,
		settings.tabs_width,
		settings.paragraph_indent,
		settings.scroll_speed,
		f32::from(u8::from(settings.justify)),
		f32::from(u8::from(settings.hyphenate)),
		f32::from(u8::from(settings.codeblock_wrap)),
		f32::from(u8::from(settings.footer_path)),
		f32::from(u8::from(settings.codeblock_copy)),
	]
}

fn export(seed: u16) -> ExportSettings {
	ExportSettings {
		font_size: 10.0 + f32::from(seed % 31),
		landscape: seed.is_multiple_of(2),
		..Default::default()
	}
}

#[derive(Clone)]
enum Disk {
	Valid([f32; 12], ExportSettings, u32),
	Invalid,
	Missing,
}

fn write(path: &Path, values: [f32; 12], export: &ExportSettings, marker: u32) {
	let mut text =
		format!("# owner-note-{marker}\ncustom = {marker} # unrelated\n");
	for (index, key) in KEYS.iter().enumerate() {
		let value = if index >= 7 {
			(values[index] != 0.0).to_string()
		} else {
			values[index].to_string()
		};
		text.push_str(&format!("{key} = {value}\n"));
	}
	text.push_str(&format!(
		"[export]\nfont_size = {}\nlandscape = {}\n",
		export.font_size, export.landscape
	));
	fs::write(path, text).unwrap();
}

fn compound(seed: u16, family: &str) -> ReaderSettings {
	use crate::{
		lang::Lang,
		render::Theme,
		settings::{FontRole, MarkdownFlavor},
	};
	let dark = seed & 1 != 0;
	let style = (seed & 2 != 0).then(|| {
		vec![
			if dark { "dark" } else { "light" }.to_owned(),
			format!("custom-{seed}"),
		]
	});
	let mut settings = ReaderSettings {
		theme: if style.is_some() && dark {
			Theme::Dark
		} else {
			Theme::Light
		},
		style,
		markdown_flavor: if seed & 4 != 0 {
			MarkdownFlavor::Obsidian
		} else {
			MarkdownFlavor::Gfm
		},
		hard_breaks: seed & 8 != 0,
		highlight_marks: seed & 16 != 0,
		lang: [
			None,
			Some(Lang::En),
			Some(Lang::ZhHans),
			Some(Lang::ZhHant),
			Some(Lang::Ja),
		][usize::from(seed % 5)],
		..Default::default()
	};
	for (index, role) in [
		FontRole::Serif,
		FontRole::SansSerif,
		FontRole::Monospace,
		FontRole::SerifHan,
		FontRole::SansSerifHan,
		FontRole::MonospaceHan,
	]
	.into_iter()
	.enumerate()
	{
		if seed & (1 << (index + 5)) != 0 {
			settings.set_font_family(
				role,
				Some(format!("{index}:{family}:{seed}")),
			);
		}
	}
	settings
}

fn merge_compound(
	target: &mut ReaderSettings,
	source: &ReaderSettings,
	fields: [bool; 4],
) {
	if fields[0] {
		target.theme = source.theme;
		target.style = source.style.clone();
	}
	if fields[1] {
		target.markdown_flavor = source.markdown_flavor;
		target.hard_breaks = source.hard_breaks;
		target.highlight_marks = source.highlight_marks;
	}
	if fields[2] {
		target.fontdef_overrides = source.fontdef_overrides.clone();
	}
	if fields[3] {
		target.lang = source.lang;
	}
}

proptest! {
	#[test]
	fn pending_compound_preferences_merge_as_whole_groups_and_keep_optional_choices(
		initial in any::<u16>(), external in prop::array::uniform2(any::<u16>()),
		operations in prop::collection::vec((0usize..4, any::<u16>()), 0..16),
		family in prop::collection::vec(any::<char>(), 0..16), reload_before_flush in any::<bool>(),
	) {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("settings.toml");
		let family: String = family.into_iter().collect();
		let save = |settings: &ReaderSettings| fs::write(&path, toml_edit::ser::to_document(&Config::from_settings(settings, ExportSettings::default())).unwrap().to_string()).unwrap();
		let mut expected = compound(initial, &family);
		save(&expected);
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		prop_assert!(warning.is_none());
		let mut pending = [false; 4];
		for (index, seed) in operations {
			let next = compound(seed, &family);
			if index == 0 && seed % 4 == 3 { store.follow_system(); expected.style = None; }
			else {
				store.changed(&next, Some([Setting::Theme, Setting::Dialect, Setting::FontFamily, Setting::Language][index]));
				let mut field = [false; 4]; field[index] = true;
				merge_compound(&mut expected, &next, field);
				if index == 0 && expected.style.is_none() { expected.style = Some(vec!["light".to_owned()]); }
			}
			pending[index] = true;
			prop_assert_eq!(store.settings(), expected.clone());
		}
		let local = expected.clone();
		let outside = compound(external[0], &family);
		save(&outside);
		store.reload().unwrap();
		expected = outside;
		merge_compound(&mut expected, &local, pending);
		prop_assert_eq!(store.settings(), expected.clone());
		prop_assert_eq!(store.has_pending_changes(), pending.contains(&true));
		// A final local edit guarantees that `flush` must merge the second file.
		store.changed(&local, Some(Setting::Dialect));
		pending[1] = true;
		let outside = compound(external[1], &family);
		save(&outside);
		expected = outside;
		merge_compound(&mut expected, &local, pending);
		if reload_before_flush { store.reload().unwrap(); prop_assert_eq!(store.settings(), expected.clone()); }
		store.flush().unwrap();
		prop_assert_eq!(store.settings(), expected.clone());
		prop_assert!(!store.has_pending_changes());
		let (loaded, warning) = SettingsStore::load(Some(path));
		prop_assert!(warning.is_none());
		if expected.style.is_none() { expected.theme = crate::render::Theme::default(); }
		prop_assert_eq!(loaded.settings(), expected);
	}

	#[test]
	fn replacing_an_initially_invalid_settings_file_preserves_its_exact_bytes_once(
		payload in prop::collection::vec(any::<u8>(), 0..128), seed in any::<u16>(),
	) {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("settings.toml");
		let mut invalid = vec![0xff];
		invalid.extend(payload);
		fs::write(&path, &invalid).unwrap();
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		prop_assert!(matches!(warning, Some(SettingsWarning::Invalid(_))));
		prop_assert_eq!(store.settings(), ReaderSettings::default());
		let next = reader(values(seed));
		store.changed(&next, Some(Setting::FontSize));
		store.set_export(export(seed));
		store.flush().unwrap();
		store.flush().unwrap();
		prop_assert!(!store.reload().unwrap());
		let (loaded, warning) = SettingsStore::load(Some(path.clone()));
		prop_assert!(warning.is_none());
		let expected = ReaderSettings { font_size: next.font_size, ..Default::default() };
		prop_assert_eq!(loaded.settings(), expected);
		prop_assert_eq!(loaded.export(), export(seed));
		let backups: Vec<_> = fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.file_name().unwrap().to_str().unwrap().starts_with("settings-invalid-")).collect();
		prop_assert_eq!(backups.len(), 1);
		prop_assert_eq!(fs::read(&backups[0]).unwrap(), invalid);
	}

	#[test]
	fn settings_transactions_merge_only_unedited_fields_and_preserve_the_last_good_state(
		operations in prop::collection::vec((0u8..8, 0usize..12, any::<u16>()), 0..16),
	) {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("settings.toml");
		let mut current = values(1);
		let mut current_export = export(1);
		write(&path, current, &current_export, 0);
		let (mut store, warning) = SettingsStore::load(Some(path.clone()));
		prop_assert!(warning.is_none());
		let mut seen = fs::read(&path).unwrap();
		let mut disk = Disk::Valid(current, current_export.clone(), 0);
		let mut marker = 0;
		let mut pending = [false; 12];
		let mut pending_export = false;
		let mut dirty = false;
		for (step, (operation, index, seed)) in [(0, 0, 3), (2, 0, 5), (4, 0, 0), (5, 0, 0)].into_iter().chain(operations).chain([(2, 0, 7), (5, 0, 0)]).enumerate() {
			match operation {
				0 => { let next = values(seed); store.changed(&reader(next), Some(FIELDS[index])); current[index] = next[index]; pending[index] = true; dirty = true; }
				1 => { current_export = export(seed); store.set_export(current_export.clone()); pending_export = true; dirty = true; }
				2 => { let values = values(seed); let export = export(seed); let next_marker = step as u32 + 1; write(&path, values, &export, next_marker); disk = Disk::Valid(values, export, next_marker); }
				3 => { fs::write(&path, "width = [").unwrap(); disk = Disk::Invalid; }
				4 => {
					let result = store.reload();
					if let Disk::Valid(values, export, next_marker) = &disk {
						let bytes = fs::read(&path).unwrap();
						prop_assert_eq!(result.unwrap(), bytes != seen);
						if bytes != seen {
							for index in 0..12 { if !pending[index] { current[index] = values[index]; } }
							if !pending_export { current_export = export.clone(); }
							marker = *next_marker; seen = bytes;
						}
					} else { prop_assert!(result.is_err()); }
				}
				5 => {
					let result = store.flush();
					if dirty && matches!(disk, Disk::Invalid) { prop_assert!(result.is_err()); }
					else {
						prop_assert!(result.is_ok());
						if dirty {
							if let Disk::Valid(values, export, next_marker) = &disk
								&& fs::read(&path).unwrap() != seen {
									for index in 0..12 { if !pending[index] { current[index] = values[index]; } }
									if !pending_export { current_export = export.clone(); }
									marker = *next_marker;
							}
							seen = fs::read(&path).unwrap();
							let text = std::str::from_utf8(&seen).unwrap();
							let document = text.parse::<toml_edit::DocumentMut>().unwrap();
							prop_assert_eq!(document["custom"].as_integer(), Some(i64::from(marker)));
							prop_assert!(text.contains(&format!("# owner-note-{marker}")), "the last accepted file comment must survive");
							prop_assert!(text.contains("# unrelated"));
							let (loaded, warning) = SettingsStore::load(Some(path.clone()));
							prop_assert!(warning.is_none());
							prop_assert_eq!(observed(&loaded.settings()), current);
							prop_assert_eq!(loaded.export(), current_export.clone());
							disk = Disk::Valid(current, current_export.clone(), marker);
							dirty = false; pending.fill(false); pending_export = false;
						}
					}
				}
				6 => { if path.exists() { fs::remove_file(&path).unwrap(); } disk = Disk::Missing; }
				_ => { let defaults = ReaderSettings::default(); store.changed(&defaults, None); current = observed(&defaults); pending.fill(true); dirty = true; }
			}
			prop_assert_eq!(observed(&store.settings()), current);
			prop_assert_eq!(store.export(), current_export.clone());
			prop_assert_eq!(store.has_pending_changes(), dirty);
		}
	}
}
