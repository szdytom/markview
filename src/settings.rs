//! Reader preferences, isolated from launch flags and document state.
use crate::{lang::Lang, layout::LayoutOptions, render::Theme};
use anyhow::{Result, bail};
use markview_layout::JustificationLimits;
use markview_layout::style::CjkType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod schema;
mod store;
pub use store::{SettingsStore, SettingsWarning};

/// The scroll-speed multiplier a reader may choose between, and the step its
/// buttons move by.
pub const SCROLL_SPEED_MIN: f32 = 0.5;
pub const SCROLL_SPEED_MAX: f32 = 2.0;
pub const SCROLL_SPEED_STEP: f32 = 0.25;

pub const SIDEBAR_WIDTH_MIN: f32 = 200.0;
pub const SIDEBAR_WIDTH_MAX: f32 = 600.0;
pub const SIDEBAR_WIDTH_STEP: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sidebar {
	Folder,
	Outline,
	Tabs,
}

/// Window chrome preference, applied when the next window is created.
#[derive(
	Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum WindowLayout {
	#[default]
	System,
	Macos,
	Windows,
	Linux,
}
impl WindowLayout {
	pub fn resolved(self) -> Self {
		if self != Self::System {
			return self;
		}
		match markview_platform::window_buttons() {
			markview_platform::WindowButtons::Windows => Self::Windows,
			markview_platform::WindowButtons::Macos => Self::Macos,
			markview_platform::WindowButtons::Linux => Self::Linux,
		}
	}
}

/// The reader tab silhouette, independent of document layout and themes.
#[derive(
	Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum TabStyle {
	#[default]
	Underline,
	Connected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReaderSettings {
	pub window_layout: WindowLayout,
	pub tab_style: TabStyle,
	pub theme: Theme,
	pub style: Option<Vec<String>>,
	/// The interface language; absent means "follow the system", so a locale
	/// change reaches the next launch. Use [`ReaderSettings::lang`].
	pub lang: Option<Lang>,
	/// Optional `User-Agent` override for web pages and images.
	pub user_agent: Option<String>,
	/// Optional `Accept-Language` override; absent follows `lang()`.
	pub accept_language: Option<String>,
	pub fontdef_overrides: Vec<FontDefOverride>,
	pub stylesheet: std::sync::Arc<markview_layout::style::Stylesheet>,
	pub font_size: f32,
	pub width: f32,
	pub folder_width: f32,
	pub outline_width: f32,
	pub tabs_width: f32,
	pub justify: bool,
	pub hyphenate: bool,
	/// How far word spacing and letter spacing may move while justifying.
	pub justification: JustificationLimits,
	/// Indent in multiples of the text size: the opening line of prose
	/// paragraphs, and the whole of a list, markers included. Zero disables it.
	pub paragraph_indent: f32,
	pub cjk_type: CjkType,
	pub codeblock_theme_override: Option<String>,
	/// Hard-wrap code block lines at the reading column instead of scrolling.
	pub codeblock_wrap: bool,
	/// Multiplies every scroll request. The desktop's own speed is the
	/// baseline; this is the only handle where the platform reports none.
	pub scroll_speed: f32,
	pub single_instance: bool,
	pub restore_session: bool,
	/// Whether the idle footer shows the open document's absolute path and
	/// copies it when clicked. Off by default.
	pub footer_path: bool,
}
impl Default for ReaderSettings {
	fn default() -> Self {
		Self {
			window_layout: WindowLayout::System,
			tab_style: TabStyle::default(),
			theme: Theme::default(),
			style: None,
			lang: None,
			user_agent: None,
			accept_language: None,
			fontdef_overrides: Vec::new(),
			stylesheet: markview_layout::style::Stylesheet::bundled(false),
			font_size: 18.0,
			width: 760.0,
			folder_width: 260.0,
			outline_width: 300.0,
			tabs_width: 320.0,
			justify: true,
			hyphenate: true,
			justification: JustificationLimits::default(),
			paragraph_indent: 0.0,
			cjk_type: default_cjk_type(),
			codeblock_theme_override: None,
			codeblock_wrap: false,
			scroll_speed: 1.0,
			single_instance: markview_platform::mobile_session(),
			restore_session: markview_platform::mobile_session(),
			footer_path: false,
		}
	}
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontDefOverride {
	pub id: String,
	#[serde(rename = "override")]
	pub replacement: String,
}

/// A font definition a reader may pick a family for.
///
/// A role names one `fontdef` of the stylesheet in force, so picking a family
/// for it overrides that definition's own candidate chain and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontRole {
	Serif,
	SansSerif,
	Monospace,
	/// The same three roles for Han text. A stylesheet carries one definition
	/// per CJK variant and resolves the one in force, so a family chosen here
	/// follows a change of variant.
	SerifHan,
	SansSerifHan,
	MonospaceHan,
}
impl FontRole {
	/// The `fontdef` id this role overrides.
	pub fn id(self) -> &'static str {
		match self {
			Self::Serif => "serif",
			Self::SansSerif => "sans-serif",
			Self::Monospace => "monospace",
			Self::SerifHan => "serif[cjk]",
			Self::SansSerifHan => "sans-serif[cjk]",
			Self::MonospaceHan => "monospace[cjk]",
		}
	}
	/// Whether the role shapes Han text, whose chooser offers only the
	/// families that cover a Han ideograph.
	pub fn han(self) -> bool {
		matches!(
			self,
			Self::SerifHan | Self::SansSerifHan | Self::MonospaceHan
		)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
	WindowLayout,
	TabStyle,
	Theme,
	FontSize,
	Width,
	SidebarWidth(Sidebar),
	Justify,
	Hyphenate,
	ParagraphIndent,
	CjkType,
	Language,
	CodeblockWrap,
	ScrollSpeed,
	SingleInstance,
	RestoreSession,
	FooterPath,
	/// Every per-role font family, which is one list of overrides.
	FontFamily,
}

/// Which document an export writes to disk.
#[derive(
	Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
	#[default]
	Pdf,
	Png,
}

/// The reader's export preferences.
///
/// They are deliberately separate from [`ReaderSettings`]: an export lays the
/// document out again at its own size and paper, so changing a field here never
/// reflows the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportSettings {
	pub format: ExportFormat,
	/// Body text size in layout pixels, the same unit the reader uses.
	pub font_size: f32,
	/// First-line indent in multiples of the text size.
	pub paragraph_indent: f32,
	/// A named paper size, or `WIDTHxHEIGHT` in millimetres.
	pub paper: String,
	pub landscape: bool,
	/// Top, right, bottom, left, in millimetres.
	pub margin: [f32; 4],
	/// PNG device pixels per layout pixel.
	pub scale: f32,
	/// Stylesheets layered on the bundled print sheet, highest priority first.
	/// The default names the sheet itself.
	pub style: Vec<String>,
}
impl Default for ExportSettings {
	fn default() -> Self {
		Self {
			format: ExportFormat::Pdf,
			font_size: Self::DEFAULT_FONT_SIZE_PX,
			paragraph_indent: 0.0,
			paper: "a4".into(),
			landscape: false,
			margin: markview_layout::style::PageStyle::DEFAULT_MARGIN_MM,
			scale: 2.0,
			style: vec!["print".into()],
		}
	}
}
impl ExportSettings {
	/// The body size an export defaults to: 12 pt on paper. A layout pixel is a
	/// ninety-sixth of an inch and a PDF point a seventy-second, so the two
	/// differ by [`markview_layout::paginate::PT_PER_PX`].
	pub const DEFAULT_FONT_SIZE_PX: f32 = 16.0;

	pub fn validate(&self) -> Result<()> {
		crate::export::config::validate_font_size(self.font_size)?;
		crate::export::config::validate_indent(self.paragraph_indent)?;
		crate::export::config::validate_scale(self.scale)?;
		crate::export::geometry(self)?;
		for id in &self.style {
			crate::stylesheet::validate_id(id)?;
		}
		Ok(())
	}
}

impl ReaderSettings {
	/// The language the interface is drawn in.
	pub fn lang(&self) -> Lang {
		self.lang.unwrap_or_default()
	}

	/// The stylesheet with this reader's CJK variant applied.
	///
	/// The variant picks which `[cjk]` font definition exists at all, so a
	/// stylesheet that has not been told about it would set Han text in a
	/// system fallback face. Applying it here rather than at each call site
	/// keeps the two from drifting apart.
	pub(crate) fn styled(
		&self,
	) -> std::sync::Arc<markview_layout::style::Stylesheet> {
		if self.stylesheet.cjk_type() == self.cjk_type {
			return self.stylesheet.clone();
		}
		let mut sheet = (*self.stylesheet).clone();
		sheet.set_cjk_type(self.cjk_type);
		std::sync::Arc::new(sheet)
	}

	pub fn layout_options(
		&self,
		viewport_width: f32,
		greedy: bool,
		fonts: &markview_layout::fonts::FontConfig,
	) -> LayoutOptions {
		LayoutOptions {
			width: self.width.min(viewport_width - 40.0).max(80.0),
			font_size: self.font_size,
			justify: self.justify,
			hyphenate: self.hyphenate,
			justification: self.justification,
			paragraph_indent: self.paragraph_indent,
			greedy,
			stylesheet: self.styled(),
			fonts: fonts.clone(),
			codeblock_theme_override: self.codeblock_theme_override.clone(),
			codeblock_wrap: self.codeblock_wrap,
			details_open: Default::default(),
			force_open: false,
			front_matter_label: self.lang().front_matter().into(),
			hide_front_matter: false,
			limits: markview_layout::limits::Limits::default(),
		}
	}
	pub fn validate(&self) -> Result<()> {
		crate::net::browser_headers(self)?;
		if let Some(ids) = &self.style {
			for id in ids {
				crate::stylesheet::validate_id(id)?;
			}
		}
		if !self.font_size.is_finite()
			|| !(10.0..=40.0).contains(&self.font_size)
			|| !self.width.is_finite()
			|| !(240.0..=1600.0).contains(&self.width)
			|| [self.folder_width, self.outline_width, self.tabs_width]
				.into_iter()
				.any(|width| {
					!(SIDEBAR_WIDTH_MIN..=SIDEBAR_WIDTH_MAX).contains(&width)
				}) || !self.paragraph_indent.is_finite()
			|| !(0.0..=4.0).contains(&self.paragraph_indent)
			|| !(SCROLL_SPEED_MIN..=SCROLL_SPEED_MAX)
				.contains(&self.scroll_speed)
		{
			bail!("Reader settings are out of range");
		}
		if !self.justification.is_valid() {
			bail!("Justification limits are out of range");
		}
		Ok(())
	}
	pub fn sidebar_width(&self, sidebar: Sidebar) -> f32 {
		match sidebar {
			Sidebar::Folder => self.folder_width,
			Sidebar::Outline => self.outline_width,
			Sidebar::Tabs => self.tabs_width,
		}
	}
	pub fn set_sidebar_width(&mut self, sidebar: Sidebar, width: f32) {
		let value = match sidebar {
			Sidebar::Folder => &mut self.folder_width,
			Sidebar::Outline => &mut self.outline_width,
			Sidebar::Tabs => &mut self.tabs_width,
		};
		*value = width.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
	}
	/// Steps the scroll-speed multiplier by whole steps, clamped to its range.
	pub fn step_scroll_speed(&mut self, steps: i8) {
		self.scroll_speed = (self.scroll_speed
			+ f32::from(steps) * SCROLL_SPEED_STEP)
			.clamp(SCROLL_SPEED_MIN, SCROLL_SPEED_MAX);
	}
	/// The family chosen for `role`, or `None` while the stylesheet's own
	/// candidate chain applies.
	pub fn font_family(&self, role: FontRole) -> Option<&str> {
		self.fontdef_overrides
			.iter()
			.find(|over| over.id == role.id())
			.map(|over| over.replacement.as_str())
	}

	/// Picks `family` for `role`, or restores the stylesheet's own candidate
	/// chain when `family` is `None`.
	pub fn set_font_family(&mut self, role: FontRole, family: Option<String>) {
		let id = role.id();
		self.fontdef_overrides.retain(|over| over.id != id);
		if let Some(replacement) = family {
			self.fontdef_overrides.push(FontDefOverride {
				id: id.to_owned(),
				replacement,
			});
		}
	}
}
fn default_cjk_type() -> CjkType {
	let Some(locale) = crate::lang::system_locale() else {
		return CjkType::Sc;
	};
	if locale.starts_with("ja-") || locale == "ja" {
		CjkType::Jp
	} else if locale.starts_with("zh-")
		&& ["tw", "hk", "mo", "hant"]
			.iter()
			.any(|part| locale.split('-').any(|item| item == *part))
	{
		CjkType::Tc
	} else {
		CjkType::Sc
	}
}
pub fn config_path() -> Option<PathBuf> {
	markview_platform::config_base().map(|p| p.join("markview/settings.toml"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stylesheet_tests;
