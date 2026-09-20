//! Effective preferences and transactional stylesheet updates.
use crate::{
	cli::{LaunchOptions, Mode},
	layout::TextShaper,
	render::Theme,
	settings::{ExportSettings, ReaderSettings, Setting, SettingsStore},
};
use std::{
	path::Path,
	sync::Arc,
	time::{Duration, Instant},
};

pub(super) struct Preferences {
	pub(super) values: ReaderSettings,
	/// The export panel's own settings. They are never part of `values`, so
	/// changing one cannot reflow the document.
	pub(super) export: ExportSettings,
	store: SettingsStore,
	pub(super) style_entries: Vec<crate::stylesheet::Entry>,
	pub(super) style_page: usize,
	pub(super) style_warning: Option<String>,
	pub(super) settings_warning: Option<String>,
	save_at: Option<Instant>,
}
impl Preferences {
	pub(super) fn new(args: &LaunchOptions, ui: &mut TextShaper) -> Self {
		let (mut settings_store, mut settings_warning) =
			SettingsStore::load(if args.mode == Mode::Window {
				crate::settings::config_path()
			} else {
				None
			});
		if args.mode == Mode::Window
			&& settings_store.path().is_some()
			&& let Err(error) = settings_store.ensure_file()
		{
			settings_warning =
				Some(format!("Cannot initialize settings: {error}"));
		}
		let mut settings = settings_store.settings();
		// A diagnostic image cannot be scrolled sideways, so its code blocks
		// wrap by default.
		if args.mode.wraps_code_blocks() {
			settings.codeblock_wrap = true;
		}
		let explicit = ReaderSettings {
			fontdef_overrides: settings.fontdef_overrides.clone(),
			style: args.style.clone().or_else(|| {
				args.theme.map(|t| {
					vec![if t == Theme::Dark { "dark" } else { "light" }.into()]
				})
			}),
			stylesheet: args.options.stylesheet.clone(),
			theme: args.theme.unwrap_or_default(),
			font_size: args.options.font_size,
			width: args.options.width,
			justify: args.options.justify,
			hyphenate: args.options.hyphenate,
			justification: settings.justification,
			paragraph_indent: args.options.paragraph_indent,
			cjk_type: args.cjk_type.unwrap_or(settings.cjk_type),
			codeblock_theme_override: settings.codeblock_theme_override.clone(),
			codeblock_wrap: settings.codeblock_wrap,
			smooth_scroll: settings.smooth_scroll,
			scroll_speed: settings.scroll_speed,
		};
		for field in &args.overrides {
			settings.copy_field(&explicit, *field);
		}
		let directory = crate::stylesheet::directory();
		let style_entries = crate::stylesheet::catalog(
			directory.as_deref(),
			settings.style.as_deref(),
		);
		let mut style_warning = None;
		if let Some(ids) = &settings.style {
			match crate::stylesheet::load_with_cjk_type(
				ids,
				crate::stylesheet::directory().as_deref(),
				settings.cjk_type,
			) {
				Ok(sheet) => settings.stylesheet = sheet,
				Err(e) => style_warning = Some(format!("Styles: {e:#}")),
			}
		}
		if settings.style.is_none() {
			let mut sheet = (*settings.stylesheet).clone();
			sheet.set_cjk_type(settings.cjk_type);
			settings.stylesheet = Arc::new(sheet);
		}
		if style_warning.is_none() {
			match crate::stylesheet::apply_font_overrides(
				settings.stylesheet.clone(),
				&settings.fontdef_overrides,
			) {
				Ok(sheet) => settings.stylesheet = sheet,
				Err(e) => style_warning = Some(format!("Styles: {e:#}")),
			}
		}
		if style_warning.is_none()
			&& let Err(error) = ui.validate_stylesheet(&settings.stylesheet)
		{
			style_warning = Some(format!("Styles: {error:#}"));
		}
		ui.set_stylesheet(settings.stylesheet.clone());
		ui.appearance = settings.stylesheet.text(
			&markview_core::style::TextAppearance::default(),
			markview_core::style::Condition::Ui,
		);
		let export = settings_store.export();
		Self {
			values: settings,
			export,
			store: settings_store,
			style_entries,
			style_page: 0,
			style_warning,
			settings_warning,
			save_at: None,
		}
	}
	pub(super) fn reload_styles(
		&mut self,
		ui: &mut TextShaper,
	) -> Option<bool> {
		self.style_entries = crate::stylesheet::catalog(
			crate::stylesheet::directory().as_deref(),
			self.values.style.as_deref(),
		);
		let ids = self.values.style.clone().unwrap_or_else(|| {
			vec![
				if self.values.theme == Theme::Dark {
					"dark"
				} else {
					"light"
				}
				.into(),
			]
		});
		match crate::stylesheet::load_with_cjk_type(
			&ids,
			crate::stylesheet::directory().as_deref(),
			self.values.cjk_type,
		) {
			Ok(sheet) => {
				let result = crate::stylesheet::apply_font_overrides(
					sheet,
					&self.values.fontdef_overrides,
				)
				.and_then(|sheet| {
					ui.validate_stylesheet(&sheet)?;
					Ok(sheet)
				});
				match result {
					Ok(sheet) => {
						let old_codeblock_theme = self
							.values
							.stylesheet
							.rule(markview_core::style::Condition::CodeBlock)
							.theme
							.clone();
						let new_codeblock_theme = sheet
							.rule(markview_core::style::Condition::CodeBlock)
							.theme
							.clone();
						let reflow = sheet.layout_key()
							!= self.values.stylesheet.layout_key()
							|| old_codeblock_theme != new_codeblock_theme
							// Diagrams are pixels, so a new diagram theme
							// needs a layout request to redraw them. The key
							// follows the font definitions the table names.
							|| self.values.stylesheet.diagram_key()
								!= sheet.diagram_key();
						self.values.stylesheet = sheet.clone();
						ui.set_stylesheet(sheet.clone());
						ui.appearance = sheet.text(
							&markview_core::style::TextAppearance::default(),
							markview_core::style::Condition::Ui,
						);
						self.style_warning = None;
						return Some(reflow);
					}
					Err(e) => {
						self.style_warning = Some(format!("Styles: {e:#}"))
					}
				}
			}
			Err(e) => self.style_warning = Some(format!("Styles: {e:#}")),
		}
		None
	}

	pub(super) fn path(&self) -> Option<&Path> {
		self.store.path()
	}
	pub(super) fn ensure_file(&mut self) -> anyhow::Result<()> {
		self.store.ensure_file()
	}
	pub(super) fn theme_preference(&self) -> Option<Theme> {
		self.store.theme_preference()
	}
	pub(super) fn stored_settings(&self) -> ReaderSettings {
		self.store.settings()
	}
	/// Applies and schedules one export-panel change.
	pub(super) fn set_export(&mut self, export: ExportSettings) {
		self.export = export.clone();
		self.store.set_export(export);
		self.schedule_save();
	}
	pub(super) fn stored_export(&self) -> ExportSettings {
		self.store.export()
	}
	pub(super) fn follow_system(&mut self) {
		self.store.follow_system();
	}
	pub(super) fn save_deadline(&self) -> Option<Instant> {
		self.save_at
	}
	pub(super) fn schedule_save(&mut self) {
		self.save_at = Some(Instant::now() + Duration::from_millis(250));
	}
	pub(super) fn changed(&mut self, field: Option<Setting>) {
		self.store.changed(&self.values, field);
		self.schedule_save();
	}
	pub(super) fn reload(&mut self) -> bool {
		match self.store.reload() {
			Ok(_) => {
				self.settings_warning = None;
				self.export = self.store.export();
				if self.store.has_pending_changes() {
					self.save_at =
						Some(Instant::now() + Duration::from_millis(250));
				}
				true
			}
			Err(error) => {
				self.settings_warning = Some(format!("{error:#}"));
				false
			}
		}
	}
	pub(super) fn flush(&mut self) -> bool {
		self.save_at = None;
		match self.store.flush() {
			Ok(()) => {
				self.settings_warning = None;
				true
			}
			Err(error) => {
				self.settings_warning =
					Some(format!("Cannot save settings: {error}"));
				false
			}
		}
	}
}

#[cfg(test)]
mod tests;
