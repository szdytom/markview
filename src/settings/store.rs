//! Transactional configuration reads, merging and durable writes.
use super::{
	ExportSettings, FontDefOverride, ReaderSettings, Setting, default_cjk_type,
};
use crate::render::Theme;
use anyhow::{Context, Result, bail};
use markview_core::JustificationLimits;
use markview_core::style::CjkType;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
	version: u32,
	/// Absent means "follow the system theme"; only a user choice is stored.
	#[serde(skip_serializing_if = "Option::is_none")]
	style: Option<Vec<String>>,
	#[serde(
		rename = "fontdef-override",
		default,
		skip_serializing_if = "Vec::is_empty"
	)]
	fontdef_overrides: Vec<FontDefOverride>,
	#[serde(skip_serializing_if = "Option::is_none")]
	theme: Option<Theme>,
	font_size: f32,
	width: f32,
	justify: bool,
	hyphenate: bool,
	justification: JustificationLimits,
	paragraph_indent: f32,
	#[serde(rename = "cjk-type")]
	cjk_type: Option<CjkType>,
	#[serde(
		rename = "codeblock-theme-override",
		skip_serializing_if = "Option::is_none"
	)]
	codeblock_theme_override: Option<String>,
	#[serde(rename = "codeblock-wrap")]
	codeblock_wrap: bool,
	#[serde(rename = "smooth-scroll")]
	smooth_scroll: bool,
	#[serde(rename = "scroll-speed")]
	scroll_speed: f32,
	/// The reader's export preferences, kept apart from the reading view.
	export: ExportSettings,
}
impl Default for Config {
	fn default() -> Self {
		let settings = ReaderSettings::default();
		Self {
			version: 1,
			theme: None,
			style: None,
			fontdef_overrides: Vec::new(),
			font_size: settings.font_size,
			width: settings.width,
			justify: settings.justify,
			hyphenate: settings.hyphenate,
			justification: settings.justification,
			paragraph_indent: settings.paragraph_indent,
			cjk_type: Some(settings.cjk_type),
			codeblock_theme_override: None,
			codeblock_wrap: settings.codeblock_wrap,
			smooth_scroll: settings.smooth_scroll,
			scroll_speed: settings.scroll_speed,
			export: ExportSettings::default(),
		}
	}
}

impl Config {
	fn reader_settings(&self) -> ReaderSettings {
		let style = self.style.clone().or_else(|| {
			self.theme.map(|theme| {
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
		ReaderSettings {
			theme,
			style,
			fontdef_overrides: self.fontdef_overrides.clone(),
			font_size: self.font_size,
			width: self.width,
			justify: self.justify,
			hyphenate: self.hyphenate,
			justification: self.justification,
			paragraph_indent: self.paragraph_indent,
			cjk_type: self.cjk_type.unwrap_or_else(default_cjk_type),
			codeblock_theme_override: self.codeblock_theme_override.clone(),
			codeblock_wrap: self.codeblock_wrap,
			smooth_scroll: self.smooth_scroll,
			scroll_speed: self.scroll_speed,
			..Default::default()
		}
	}
}

pub struct SettingsStore {
	path: Option<PathBuf>,
	saved: ReaderSettings,
	/// The export preferences, whole-value like the reader settings but written
	/// through their own accessor so the two can never be confused.
	saved_export: ExportSettings,
	pending_export: bool,
	invalid: Option<Vec<u8>>,
	dirty: bool,
	pending: Vec<Setting>,
	source: Option<Vec<u8>>,
}
impl SettingsStore {
	pub fn load(path: Option<PathBuf>) -> (Self, Option<String>) {
		let mut store = Self {
			path,
			saved: ReaderSettings::default(),
			saved_export: ExportSettings::default(),
			pending_export: false,
			invalid: None,
			dirty: false,
			pending: Vec::new(),
			source: None,
		};
		let mut warning = None;
		if let Some(path) = &store.path {
			match fs::read(path) {
				Ok(bytes) => {
					store.source = Some(bytes.clone());
					let result = (|| -> Result<Config> {
						let config: Config = toml_edit::de::from_str(
							std::str::from_utf8(&bytes)?,
						)?;
						if config.version != 1 {
							bail!(
								"Unsupported settings version {}",
								config.version
							);
						}
						Ok(config)
					})();
					match result {
						Ok(config) => {
							store.saved = config.reader_settings();
							if let Err(error) = store.saved.validate() {
								warning = Some(format!(
									"Settings: {error}; using defaults"
								));
								store.saved = ReaderSettings::default();

								store.invalid = Some(bytes);
							}
							// A bad `[export]` table never discards the reading
							// settings around it; only the export falls back.
							match config.export.validate() {
								Ok(()) => store.saved_export = config.export,
								Err(error) => {
									warning = Some(format!(
										"Export settings: {error}; using defaults"
									));
								}
							}
						}
						Err(error) => {
							warning = Some(format!(
								"Settings: {error}; using defaults"
							));
							store.invalid = Some(bytes);
						}
					}
				}
				Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
				Err(error) => {
					warning = Some(format!("Cannot read settings: {error}"))
				}
			}
		}
		(store, warning)
	}
	pub fn path(&self) -> Option<&std::path::Path> {
		self.path.as_deref()
	}
	pub fn has_pending_changes(&self) -> bool {
		self.dirty
	}
	/// Invalid or temporarily missing files never replace the last good values.
	pub fn reload(&mut self) -> Result<bool> {
		let Some(path) = &self.path else {
			return Ok(false);
		};
		let bytes = fs::read(path)
			.context("Cannot read settings; keeping current values")?;
		if self.source.as_ref() == Some(&bytes) {
			return Ok(false);
		}
		let config: Config =
			toml_edit::de::from_str(std::str::from_utf8(&bytes)?)
				.context("Invalid settings; keeping current values")?;
		if config.version != 1 {
			bail!(
				"Unsupported settings version {}; keeping current values",
				config.version
			);
		}
		let saved = config.reader_settings();
		saved
			.validate()
			.context("Invalid settings; keeping current values")?;
		config
			.export
			.validate()
			.context("Invalid export settings; keeping current values")?;
		let saved_export = if self.pending_export {
			self.saved_export.clone()
		} else {
			config.export
		};
		let mut next = Self {
			path: self.path.clone(),
			saved,
			saved_export,
			pending_export: self.pending_export,
			invalid: None,
			dirty: self.dirty,
			pending: self.pending.clone(),
			source: Some(bytes),
		};
		for field in &self.pending {
			next.saved.copy_field(&self.saved, *field);
		}
		next.pending = self.pending.clone();
		next.dirty = self.dirty;
		*self = next;
		Ok(true)
	}
	pub fn ensure_file(&mut self) -> Result<()> {
		let path = self
			.path
			.as_ref()
			.context("No user configuration directory available")?;
		if !path.exists() {
			let legacy = path.with_extension("json");
			if self.source.is_none() && !self.dirty && legacy.exists() {
				let config: Config =
					serde_json::from_slice(&fs::read(&legacy)?)?;
				if config.version != 1 {
					bail!("Unsupported legacy settings version");
				}
				let saved = config.reader_settings();
				saved.validate()?;
				self.saved = saved;
				self.saved_export = config.export;
			}
			self.dirty = true;
			self.flush()?;
		}
		Ok(())
	}
	pub fn settings(&self) -> ReaderSettings {
		self.saved.clone()
	}
	pub fn export(&self) -> ExportSettings {
		self.saved_export.clone()
	}
	/// Replaces the export preferences. It never touches the reading settings,
	/// so the window cannot reflow because of an export change.
	pub fn set_export(&mut self, export: ExportSettings) {
		self.saved_export = export;
		self.pending_export = true;
		self.dirty = true;
	}
	/// The saved theme, or `None` while the reader still follows the system.
	pub fn theme_preference(&self) -> Option<Theme> {
		self.saved.style.as_ref().map(|ids| {
			if ids.first().is_some_and(|id| id == "dark") {
				Theme::Dark
			} else {
				Theme::Light
			}
		})
	}
	pub fn follow_system(&mut self) {
		self.saved.style = None;
		if !self.pending.contains(&Setting::Theme) {
			self.pending.push(Setting::Theme);
		}
		self.dirty = true;
	}
	pub fn changed(
		&mut self,
		effective: &ReaderSettings,
		field: Option<Setting>,
	) {
		match field {
			Some(field) => {
				if !self.pending.contains(&field) {
					self.pending.push(field);
				}
				self.saved.copy_field(effective, field);
				if field == Setting::Theme {
					self.saved.style = effective.style.clone().or_else(|| {
						Some(vec![
							if effective.theme == Theme::Dark {
								"dark"
							} else {
								"light"
							}
							.into(),
						])
					});
				}
			}
			None => {
				self.pending = vec![
					Setting::Theme,
					Setting::FontSize,
					Setting::Width,
					Setting::Justify,
					Setting::Hyphenate,
					Setting::ParagraphIndent,
					Setting::CjkType,
					Setting::CodeblockWrap,
					Setting::SmoothScroll,
					Setting::ScrollSpeed,
				];
				self.saved = effective.clone();
				self.saved.style = None;
			}
		}
		self.dirty = true;
	}
	pub fn flush(&mut self) -> Result<()> {
		if !self.dirty {
			return Ok(());
		}
		// Merge external edits before saving a pending UI change.
		if self.path.as_ref().is_some_and(|p| p.exists()) {
			self.reload()?;
		}
		let path = self
			.path
			.as_ref()
			.context("No user configuration directory available")?;
		let parent = path.parent().context("Invalid configuration path")?;
		fs::create_dir_all(parent)?;
		if let Some(bytes) = &self.invalid {
			let mut backup = tempfile::Builder::new()
				.prefix("settings-invalid-")
				.suffix(".toml")
				.tempfile_in(parent)?;
			backup.write_all(bytes)?;
			backup.as_file().sync_all()?;
			backup.keep()?;
			self.invalid = None;
		}
		self.saved.validate()?;
		self.saved_export.validate()?;
		let config = Config {
			version: 1,
			theme: None,
			style: self.saved.style.clone(),
			fontdef_overrides: self.saved.fontdef_overrides.clone(),
			font_size: self.saved.font_size,
			width: self.saved.width,
			justify: self.saved.justify,
			hyphenate: self.saved.hyphenate,
			justification: self.saved.justification,
			paragraph_indent: self.saved.paragraph_indent,
			cjk_type: Some(self.saved.cjk_type),
			codeblock_theme_override: self
				.saved
				.codeblock_theme_override
				.clone(),
			codeblock_wrap: self.saved.codeblock_wrap,
			smooth_scroll: self.saved.smooth_scroll,
			scroll_speed: self.saved.scroll_speed,
			export: self.saved_export.clone(),
		};
		let mut values = toml_edit::ser::to_document(&config)?;
		// `[export]` reads as its own section; the serializer would otherwise
		// write one long inline table.
		if let Some(item) = values.remove("export") {
			let item = match item {
				toml_edit::Item::Value(toml_edit::Value::InlineTable(
					inline,
				)) => toml_edit::Item::Table(inline.into_table()),
				other => other,
			};
			values.insert("export", item);
		}
		let mut document = self
			.source
			.as_ref()
			.and_then(|b| std::str::from_utf8(b).ok())
			.and_then(|s| s.parse::<toml_edit::DocumentMut>().ok())
			.unwrap_or_default();
		for (key, value) in values.iter() {
			let mut value = value.clone();
			if let (Some(old), Some(new)) = (
				document.get(key).and_then(|v| v.as_value()),
				value.as_value_mut(),
			) {
				*new.decor_mut() = old.decor().clone();
			}
			document[key] = value;
		}
		let mut comments = String::new();
		for key in [Some("theme"), config.style.is_none().then_some("style")]
			.into_iter()
			.flatten()
		{
			if let Some(prefix) = document
				.key(key)
				.and_then(|k| k.leaf_decor().prefix())
				.and_then(|p| p.as_str())
				&& prefix.contains('#')
			{
				comments.push_str(prefix);
				if !prefix.ends_with('\n') {
					comments.push('\n');
				}
			}
			if let Some(suffix) = document
				.get(key)
				.and_then(|v| v.as_value())
				.and_then(|v| v.decor().suffix())
				.and_then(|p| p.as_str())
				&& suffix.contains('#')
			{
				comments.push_str(suffix.trim_start());
				if !suffix.ends_with('\n') {
					comments.push('\n');
				}
			}
			document.remove(key);
		}
		let bytes = format!("{comments}{document}").into_bytes();
		let mut temp = tempfile::NamedTempFile::new_in(parent)?;
		temp.write_all(&bytes)?;
		temp.as_file().sync_all()?;
		temp.persist(path)?;
		// Durability of the rename itself needs the directory entry flushed.
		if let Ok(dir) = fs::File::open(parent) {
			let _ = dir.sync_all();
		}
		self.dirty = false;
		self.pending.clear();
		self.pending_export = false;
		self.source = Some(bytes);
		Ok(())
	}
}
