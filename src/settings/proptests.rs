use super::*;
use proptest::prelude::*;

const ROLES: [FontRole; 6] = [
	FontRole::Serif,
	FontRole::SansSerif,
	FontRole::Monospace,
	FontRole::SerifHan,
	FontRole::SansSerifHan,
	FontRole::MonospaceHan,
];

proptest! {
	#[test]
	fn preference_sequences_keep_independent_sidebar_font_and_scroll_values(
		operations in prop::collection::vec((0u8..3, 0usize..6, -127i16..128, prop::option::of(prop::collection::vec(any::<char>(), 0..16))), 0..32),
	) {
		let mut settings = ReaderSettings::default();
		let mut widths = [260i16, 300, 320];
		let mut speed = 4i16;
		let mut families: [Option<String>; 6] = Default::default();
		let sidebars = [Sidebar::Folder, Sidebar::Outline, Sidebar::Tabs];
		for (operation, index, value, family) in operations {
			match operation {
				0 => { settings.set_sidebar_width(sidebars[index % 3], f32::from(value * 8)); widths[index % 3] = (value * 8).clamp(200, 600); }
				1 => { settings.step_scroll_speed(value as i8); speed = (speed + value).clamp(2, 8); }
				_ => { let family = family.map(|chars| chars.into_iter().collect::<String>()); settings.set_font_family(ROLES[index], family.clone()); families[index] = family; }
			}
			for (index, &sidebar) in sidebars.iter().enumerate() { prop_assert_eq!(settings.sidebar_width(sidebar), f32::from(widths[index])); }
			prop_assert_eq!(settings.scroll_speed * 4.0, f32::from(speed));
			for (index, &role) in ROLES.iter().enumerate() { prop_assert_eq!(settings.font_family(role), families[index].as_deref()); }
			prop_assert_eq!(settings.fontdef_overrides.len(), families.iter().flatten().count());
		}
	}

	#[test]
	fn numeric_validation_accepts_exactly_the_documented_finite_ranges(value in any::<f32>()) {
		let base = ReaderSettings::default();
		for (index, (low, high)) in [(10.0, 40.0), (240.0, 1600.0), (200.0, 600.0), (200.0, 600.0), (200.0, 600.0), (0.0, 4.0), (0.5, 2.0)].into_iter().enumerate() {
			for value in [value, low, high, low - 1.0, high + 1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
				let mut settings = base.clone();
				let field = match index { 0 => &mut settings.font_size, 1 => &mut settings.width, 2 => &mut settings.folder_width, 3 => &mut settings.outline_width, 4 => &mut settings.tabs_width, 5 => &mut settings.paragraph_indent, _ => &mut settings.scroll_speed };
				*field = value;
				prop_assert_eq!(settings.validate().is_ok(), value.is_finite() && value >= low && value <= high, "field {}, value {}", index, value);
			}
		}
	}

	#[test]
	fn persisted_preferences_preserve_values_and_produce_the_requested_layout_dialect(
		width in 240u16..1601, size in 10u8..41, viewport in -40i16..1801, flags in prop::array::uniform8(any::<bool>()), cjk in 0usize..4,
		family in prop::collection::vec(any::<char>(), 0..24),
	) {
		let [obsidian, hard_breaks, highlight_marks, justify, hyphenate, wrap, restore, single] = flags;
		let mut settings = ReaderSettings { width: f32::from(width), font_size: f32::from(size), markdown_flavor: if obsidian { MarkdownFlavor::Obsidian } else { MarkdownFlavor::Gfm }, hard_breaks, highlight_marks, justify, hyphenate, codeblock_wrap: wrap, restore_session: restore, single_instance: single, cjk_type: [CjkType::Sc, CjkType::Tc, CjkType::Jp, CjkType::None][cjk], ..Default::default() };
		settings.set_font_family(FontRole::Serif, Some(family.into_iter().collect()));
		let config = store::Config::from_settings(&settings, ExportSettings::default());
		let encoded = toml_edit::ser::to_document(&config).unwrap().to_string();
		let decoded: store::Config = toml_edit::de::from_str(&encoded).unwrap();
		let restored = decoded.reader_settings();
		// Session restoration enables the single-instance setting when loading.
		let mut expected = settings.clone();
		expected.single_instance = single || restore;
		prop_assert_eq!(&restored, &expected);
		let options = restored.layout_options(f32::from(viewport), true, &Default::default());
		let available = viewport - 40;
		let expected_width = if available < 80 { 80 } else if available < width as i16 { available } else { width as i16 };
		prop_assert_eq!(options.width, f32::from(expected_width));
		prop_assert_eq!(options.font_size, f32::from(size));
		prop_assert_eq!((options.ofm, options.hard_breaks, options.highlight_marks), (obsidian, hard_breaks, obsidian || highlight_marks));
		prop_assert_eq!((options.justify, options.hyphenate, options.codeblock_wrap, options.greedy), (justify, hyphenate, wrap, true));
		prop_assert_eq!(options.stylesheet.cjk_type(), settings.cjk_type);
	}
}
