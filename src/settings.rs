//! Reader preferences, isolated from launch flags and document state.
use crate::{layout::LayoutOptions, render::Theme};
use anyhow::{Result, bail};
use markview_core::JustificationLimits;
use markview_core::style::CjkType;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod store;
pub use store::SettingsStore;

#[derive(Clone, Debug, PartialEq)]
pub struct ReaderSettings {
	pub theme: Theme,
	pub style: Option<Vec<String>>,
	pub fontdef_overrides: Vec<FontDefOverride>,
	pub stylesheet: std::sync::Arc<markview_core::style::Stylesheet>,
	pub font_size: f32,
	pub width: f32,
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
}
impl Default for ReaderSettings {
	fn default() -> Self {
		Self {
			theme: Theme::default(),
			style: None,
			fontdef_overrides: Vec::new(),
			stylesheet: markview_core::style::Stylesheet::bundled(false),
			font_size: 18.0,
			width: 760.0,
			justify: true,
			hyphenate: true,
			justification: JustificationLimits::default(),
			paragraph_indent: 0.0,
			cjk_type: default_cjk_type(),
			codeblock_theme_override: None,
			codeblock_wrap: false,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
	Theme,
	FontSize,
	Width,
	Justify,
	Hyphenate,
	ParagraphIndent,
	CjkType,
	CodeblockWrap,
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
			margin: markview_core::style::PageStyle::DEFAULT_MARGIN_MM,
			scale: 2.0,
			style: vec!["print".into()],
		}
	}
}
impl ExportSettings {
	/// The body size an export defaults to: 12 pt on paper. A layout pixel is a
	/// ninety-sixth of an inch and a PDF point a seventy-second, so the two
	/// differ by [`markview_core::paginate::PT_PER_PX`].
	pub const DEFAULT_FONT_SIZE_PX: f32 = 16.0;

	pub fn validate(&self) -> Result<()> {
		if !self.font_size.is_finite()
			|| !(10.0..=40.0).contains(&self.font_size)
			|| !self.paragraph_indent.is_finite()
			|| !(0.0..=4.0).contains(&self.paragraph_indent)
			|| !self.scale.is_finite()
			|| !(0.5..=4.0).contains(&self.scale)
			|| self.margin.iter().any(|v| !v.is_finite() || *v < 0.0)
		{
			bail!("Export settings are out of range");
		}
		if markview_core::style::parse_paper_size(&self.paper).is_none() {
			bail!("Export paper {:?} is not a size", self.paper);
		}
		for id in &self.style {
			crate::stylesheet::validate_id(id)?;
		}
		Ok(())
	}
}

impl ReaderSettings {
	/// The stylesheet with this reader's CJK variant applied.
	///
	/// The variant picks which `[cjk]` font definition exists at all, so a
	/// stylesheet that has not been told about it would set Han text in a
	/// system fallback face. Applying it here rather than at each call site
	/// keeps the two from drifting apart.
	fn styled(&self) -> std::sync::Arc<markview_core::style::Stylesheet> {
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
		fonts: &markview_core::fonts::FontConfig,
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
			limits: markview_core::limits::Limits::default(),
		}
	}
	pub fn validate(&self) -> Result<()> {
		if let Some(ids) = &self.style {
			for id in ids {
				crate::stylesheet::validate_id(id)?;
			}
		}
		if !self.font_size.is_finite()
			|| !(10.0..=40.0).contains(&self.font_size)
			|| !self.width.is_finite()
			|| !(240.0..=1600.0).contains(&self.width)
			|| !self.paragraph_indent.is_finite()
			|| !(0.0..=4.0).contains(&self.paragraph_indent)
		{
			bail!("Reader settings are out of range");
		}
		if !self.justification.is_valid() {
			bail!("Justification limits are out of range");
		}
		Ok(())
	}
	pub fn copy_field(&mut self, other: &Self, field: Setting) {
		match field {
			Setting::Theme => {
				self.theme = other.theme;
				self.style = other.style.clone();
			}
			Setting::FontSize => self.font_size = other.font_size,
			Setting::Width => self.width = other.width,
			Setting::Justify => self.justify = other.justify,
			Setting::Hyphenate => self.hyphenate = other.hyphenate,
			Setting::ParagraphIndent => {
				self.paragraph_indent = other.paragraph_indent
			}
			Setting::CjkType => self.cjk_type = other.cjk_type,
			Setting::CodeblockWrap => {
				self.codeblock_wrap = other.codeblock_wrap
			}
		}
	}
}
fn default_cjk_type() -> CjkType {
	let Some(locale) = sys_locale::get_locale() else {
		return CjkType::Sc;
	};
	let locale = locale.to_ascii_lowercase().replace('_', "-");
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
	#[cfg(target_os = "windows")]
	let base = std::env::var_os("APPDATA").map(PathBuf::from);
	#[cfg(target_os = "macos")]
	let base = std::env::var_os("HOME")
		.map(|p| PathBuf::from(p).join("Library/Application Support"));
	#[cfg(not(any(target_os = "windows", target_os = "macos")))]
	let base = std::env::var_os("XDG_CONFIG_HOME")
		.map(PathBuf::from)
		.filter(|p| p.is_absolute())
		.or_else(|| {
			std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config"))
		});
	base.map(|p| p.join("markview/settings.toml"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stylesheet_tests;
