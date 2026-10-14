//! Transactional configuration reads, merging and durable writes.
use super::{
	ExportSettings, FontDefOverride, MarkdownFlavor, ReaderSettings, Setting,
	TabStyle, WindowLayout,
};
use crate::lang::Lang;
use crate::render::Theme;
use anyhow::{Context, Result, bail};
use markview_layout::JustificationLimits;
use markview_layout::style::CjkType;
use serde::{Deserialize, Serialize};
use std::{
	fs,
	io::Write,
	path::{Path, PathBuf},
};

/// What went wrong while reading the configuration, kept as the failure it is
/// rather than as text.
///
/// The file is read before the interface knows which language it is drawn in,
/// so a message spelled here could never follow the reader's choice of
/// interface language. The value carries the failure until
/// [`SettingsWarning::text`] is asked for it, in the language in force by
/// then.
#[derive(Debug)]
pub enum SettingsWarning {
	/// The file was read, but its contents did not make sense.
	Invalid(anyhow::Error),
	/// The export half was rejected while the reading half survived.
	ExportInvalid(anyhow::Error),
	/// The file exists but could not be read at all.
	Unreadable(anyhow::Error),
	/// A write of the settings in force did not reach the disk.
	SaveFailed(anyhow::Error),
	/// The settings file could not be handed to the operating system.
	OpenFailed(anyhow::Error),
	/// The settings file could not be created where it belongs.
	InitFailed(anyhow::Error),
}
impl SettingsWarning {
	/// The warning in `lang`. The failure is spelled into the message, so the
	/// result borrows the text only when there is none to spell.
	pub fn text(&self, lang: Lang) -> std::borrow::Cow<'static, str> {
		use std::borrow::Cow;
		match self {
			Self::Invalid(error) => {
				Cow::Owned(lang.status_settings_invalid(format!("{error:#}")))
			}
			Self::ExportInvalid(error) => Cow::Owned(
				lang.status_export_saw_settings_failed(format!("{error:#}")),
			),
			Self::Unreadable(error) => Cow::Owned(
				lang.status_settings_unreadable(format!("{error:#}")),
			),
			Self::SaveFailed(error) => Cow::Owned(
				lang.status_settings_save_failed(format!("{error:#}")),
			),
			Self::OpenFailed(error) => Cow::Owned(
				lang.status_settings_open_failed(format!("{error:#}")),
			),
			Self::InitFailed(error) => Cow::Owned(
				lang.status_settings_init_failed(format!("{error:#}")),
			),
		}
	}
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Config {
	pub(super) version: u32,
	#[serde(rename = "window-layout")]
	pub(super) window_layout: WindowLayout,
	#[serde(rename = "tab-style")]
	pub(super) tab_style: TabStyle,
	/// Absent means "follow the system theme"; only a user choice is stored.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) style: Option<Vec<String>>,
	#[serde(
		rename = "fontdef-override",
		default,
		skip_serializing_if = "Vec::is_empty"
	)]
	pub(super) fontdef_overrides: Vec<FontDefOverride>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) theme: Option<Theme>,
	/// Absent means "follow the system language"; only a user choice is
	/// stored, so a locale change reaches the next launch.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) language: Option<Lang>,
	#[serde(rename = "user-agent", skip_serializing_if = "Option::is_none")]
	pub(super) user_agent: Option<String>,
	#[serde(
		rename = "accept-language",
		skip_serializing_if = "Option::is_none"
	)]
	pub(super) accept_language: Option<String>,
	pub(super) font_size: f32,
	pub(super) width: f32,
	#[serde(rename = "folder-width")]
	pub(super) folder_width: f32,
	#[serde(rename = "outline-width")]
	pub(super) outline_width: f32,
	#[serde(rename = "tabs-width")]
	pub(super) tabs_width: f32,
	pub(super) justify: bool,
	pub(super) hyphenate: bool,
	pub(super) justification: JustificationLimits,
	pub(super) paragraph_indent: f32,
	#[serde(rename = "cjk-type")]
	pub(super) cjk_type: Option<CjkType>,
	#[serde(rename = "markdown-flavor")]
	pub(super) markdown_flavor: MarkdownFlavor,
	#[serde(rename = "hard-breaks")]
	pub(super) hard_breaks: bool,
	#[serde(rename = "highlight-marks")]
	pub(super) highlight_marks: bool,
	#[serde(
		rename = "codeblock-theme-override",
		skip_serializing_if = "Option::is_none"
	)]
	pub(super) codeblock_theme_override: Option<String>,
	#[serde(rename = "codeblock-wrap")]
	pub(super) codeblock_wrap: bool,
	#[serde(rename = "scroll-speed")]
	pub(super) scroll_speed: f32,
	#[serde(rename = "single-instance")]
	pub(super) single_instance: bool,
	#[serde(rename = "restore-session")]
	pub(super) restore_session: bool,
	#[serde(rename = "footer-path")]
	pub(super) footer_path: bool,
	/// The reader's export preferences, kept apart from the reading view.
	pub(super) export: ExportSettings,
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
	pub fn load(path: Option<PathBuf>) -> (Self, Option<SettingsWarning>) {
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
								warning = Some(SettingsWarning::Invalid(error));
								store.saved = ReaderSettings::default();

								store.invalid = Some(bytes);
							}
							// A bad `[export]` table never discards the reading
							// settings around it; only the export falls back.
							match config.export.validate() {
								Ok(()) => store.saved_export = config.export,
								Err(error) => {
									warning = Some(
										SettingsWarning::ExportInvalid(error),
									);
								}
							}
						}
						Err(error) => {
							warning = Some(SettingsWarning::Invalid(error));
							store.invalid = Some(bytes);
						}
					}
				}
				Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
				Err(error) => {
					warning = Some(SettingsWarning::Unreadable(error.into()))
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
		next.saved.single_instance |= next.saved.restore_session;
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
				self.pending = Setting::ALL.to_vec();
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
			backup_invalid_settings(parent, bytes)?;
			self.invalid = None;
		}
		self.saved.validate()?;
		self.saved_export.validate()?;
		let bytes = self.encoded_settings()?;
		persist_settings(path, parent, &bytes)?;
		self.dirty = false;
		self.pending.clear();
		self.pending_export = false;
		self.source = Some(bytes);
		Ok(())
	}
	fn encoded_settings(&self) -> Result<Vec<u8>> {
		let config =
			Config::from_settings(&self.saved, self.saved_export.clone());
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
		for key in [
			Some("theme"),
			config.user_agent.is_none().then_some("user-agent"),
			config
				.accept_language
				.is_none()
				.then_some("accept-language"),
			config.style.is_none().then_some("style"),
			config.language.is_none().then_some("language"),
			config
				.fontdef_overrides
				.is_empty()
				.then_some("fontdef-override"),
		]
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
		Ok(bytes)
	}
}

fn backup_invalid_settings(parent: &Path, bytes: &[u8]) -> Result<()> {
	let mut backup = tempfile::Builder::new()
		.prefix("settings-invalid-")
		.suffix(".toml")
		.tempfile_in(parent)?;
	backup.write_all(bytes)?;
	backup.as_file().sync_all()?;
	backup.keep()?;
	Ok(())
}

fn persist_settings(path: &Path, parent: &Path, bytes: &[u8]) -> Result<()> {
	let mut temp = tempfile::NamedTempFile::new_in(parent)?;
	temp.write_all(bytes)?;
	temp.as_file().sync_all()?;
	temp.persist(path)?;
	// Durability of the rename itself needs the directory entry flushed.
	if let Ok(dir) = fs::File::open(parent) {
		let _ = dir.sync_all();
	}
	Ok(())
}
