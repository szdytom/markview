//! The one field table every reader-settings mapping is derived from.
//!
//! Each persisted field is written once below. The macro expands the table
//! into `Setting::ALL`, `ReaderSettings::copy_field`, `Config::default`,
//! `Config::reader_settings` and `Config::from_settings`. Rows marked
//! `special:` call a hook instead of assigning one field directly; their
//! `Config` shape differs from the reader shape.

use super::{
	ExportSettings, ReaderSettings, Setting, Sidebar, default_cjk_type,
	store::Config,
};
use crate::render::Theme;

/// Reads one field unchanged, or clones it when the field is not `Copy`.
macro_rules! value {
	($source:ident, $field:ident, copy) => {
		$source.$field
	};
	($source:ident, $field:ident, clone) => {
		$source.$field.clone()
	};
}

/// `ReaderSettings` to `Config`, for fields whose config shape differs.
macro_rules! encode_value {
	(Theme, $source:ident, $field:ident, $kind:ident) => {
		None
	};
	(CjkType, $source:ident, $field:ident, $kind:ident) => {
		Some($source.cjk_type)
	};
	(SingleInstance, $source:ident, $field:ident, $kind:ident) => {
		$source.single_instance
	};
	($variant:ident, $source:ident, $field:ident, $kind:ident) => {
		value!($source, $field, $kind)
	};
}

/// `Config` to `ReaderSettings`, for fields whose config shape differs.
macro_rules! decode_assign {
	(Theme, $source:ident, $target:ident, $field:ident, $kind:ident, $config:ident) => {};
	(CjkType, $source:ident, $target:ident, $field:ident, $kind:ident, $config:ident) => {
		$target.cjk_type = $source.cjk_type.unwrap_or_else(default_cjk_type);
	};
	(SingleInstance, $source:ident, $target:ident, $field:ident, $kind:ident, $config:ident) => {
		$target.single_instance =
			$source.single_instance || $source.restore_session;
	};
	($variant:ident, $source:ident, $target:ident, $field:ident, $kind:ident, $config:ident) => {
		$target.$field = value!($source, $config, $kind);
	};
}

/// The body of one `copy_field` arm.
macro_rules! copy_body {
	($field:ident, $kind:ident, $target:ident, $source:ident, special: $hook:ident, $arg:path) => {
		$hook($target, $source, $arg)
	};
	($field:ident, $kind:ident, $target:ident, $source:ident, special: $hook:ident) => {
		$hook($target, $source)
	};
	($field:ident, copy, $target:ident, $source:ident) => {
		$target.$field = $source.$field
	};
	($field:ident, clone, $target:ident, $source:ident) => {
		$target.$field = $source.$field.clone()
	};
}

macro_rules! settings_schema {
	(
		settings {
			$(
				$variant:ident $(($arg:path))? => $field:ident : $config:ident, $kind:ident $(, special: $hook:ident)?;
			)*
		}
		config {
			$( $config_field:ident, $config_kind:ident; )*
		}
	) => {
		impl Setting {
			/// Every persisted setting, in the order the settings page resets.
			pub const ALL: [Self; 20] =
				[ $( Setting::$variant $(($arg))? ),* ];
			/// The number of `Setting` variants.
			#[allow(dead_code, reason = "The settings tests assert this count.")]
			pub const COUNT: usize = 18;
		}
		impl ReaderSettings {
			pub fn copy_field(&mut self, other: &Self, field: Setting) {
				match field {
					$(
						Setting::$variant $(($arg))? => {
							copy_body!($field, $kind, self, other $(, special: $hook)? $(, $arg)?)
						}
					)*
				}
			}
		}
		impl Default for Config {
			fn default() -> Self {
				let settings = ReaderSettings::default();
				Self {
					version: 1,
					$(
						$config: encode_value!($variant, settings, $field, $kind),
					)*
					$( $config_field: value!(settings, $config_field, $config_kind), )*
					export: ExportSettings::default(),
				}
			}
		}
		impl Config {
			pub(super) fn reader_settings(&self) -> ReaderSettings {
				let mut settings = ReaderSettings::default();
				$(
					decode_assign!($variant, self, settings, $field, $kind, $config);
				)*
				$( settings.$config_field = value!(self, $config_field, $config_kind); )*
				let (theme, style) =
					read_theme(settings.style.take(), self.theme);
				settings.theme = theme;
				settings.style = style;
				settings
			}
			pub(super) fn from_settings(
				settings: &ReaderSettings,
				export: ExportSettings,
			) -> Self {
				Self {
					version: 1,
					$(
						$config: encode_value!($variant, settings, $field, $kind),
					)*
					$( $config_field: value!(settings, $config_field, $config_kind), )*
					export,
				}
			}
		}
	};
}

settings_schema! {
	settings {
		WindowLayout => window_layout : window_layout, copy;
		TabStyle => tab_style : tab_style, copy;
		Theme => theme : theme, copy, special: copy_theme;
		FontSize => font_size : font_size, copy;
		Width => width : width, copy;
		SidebarWidth(Sidebar::Folder) => folder_width : folder_width, copy,
			special: copy_sidebar;
		SidebarWidth(Sidebar::Outline) => outline_width : outline_width, copy,
			special: copy_sidebar;
		SidebarWidth(Sidebar::Tabs) => tabs_width : tabs_width, copy,
			special: copy_sidebar;
		Justify => justify : justify, copy;
		Hyphenate => hyphenate : hyphenate, copy;
		ParagraphIndent => paragraph_indent : paragraph_indent, copy;
		CjkType => cjk_type : cjk_type, copy, special: copy_cjk_type;
		Language => lang : language, copy;
		CodeblockWrap => codeblock_wrap : codeblock_wrap, copy;
		Dialect => markdown_flavor : markdown_flavor, copy, special: copy_dialect;
		ScrollSpeed => scroll_speed : scroll_speed, copy;
		SingleInstance => single_instance : single_instance, copy,
			special: copy_single_instance;
		RestoreSession => restore_session : restore_session, copy;
		FooterPath => footer_path : footer_path, copy;
		FontFamily => fontdef_overrides : fontdef_overrides, clone,
			special: copy_font_family;
	}
	config {
		hard_breaks, copy;
		highlight_marks, copy;
		style, clone;
		user_agent, clone;
		accept_language, clone;
		justification, copy;
		codeblock_theme_override, clone;
	}
}

/// Splits a saved theme into the effective style list and the theme it names.
fn read_theme(
	style: Option<Vec<String>>,
	legacy: Option<Theme>,
) -> (Theme, Option<Vec<String>>) {
	let style = style.or_else(|| {
		legacy.map(|theme| {
			vec![
				if theme == Theme::Dark {
					"dark"
				} else {
					"light"
				}
				.into(),
			]
		})
	});
	let theme = style
		.as_ref()
		.map(|ids| {
			if ids.first().is_some_and(|id| id == "dark") {
				Theme::Dark
			} else {
				Theme::Light
			}
		})
		.unwrap_or_default();
	(theme, style)
}

fn copy_theme(to: &mut ReaderSettings, from: &ReaderSettings) {
	to.theme = from.theme;
	to.style = from.style.clone();
}

fn copy_sidebar(
	to: &mut ReaderSettings,
	from: &ReaderSettings,
	sidebar: Sidebar,
) {
	to.set_sidebar_width(sidebar, from.sidebar_width(sidebar));
}

fn copy_cjk_type(to: &mut ReaderSettings, from: &ReaderSettings) {
	to.cjk_type = from.cjk_type;
}

fn copy_single_instance(to: &mut ReaderSettings, from: &ReaderSettings) {
	to.single_instance = from.single_instance;
}

fn copy_font_family(to: &mut ReaderSettings, from: &ReaderSettings) {
	to.fontdef_overrides = from.fontdef_overrides.clone();
}

fn copy_dialect(to: &mut ReaderSettings, from: &ReaderSettings) {
	to.markdown_flavor = from.markdown_flavor;
	to.hard_breaks = from.hard_breaks;
	to.highlight_marks = from.highlight_marks;
}
