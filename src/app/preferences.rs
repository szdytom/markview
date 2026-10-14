//! Effective preferences and transactional stylesheet updates.
use crate::{
	cli::{LaunchOptions, Mode},
	lang::Lang,
	layout::TextShaper,
	render::Theme,
	settings::{
		ExportSettings, ReaderSettings, Setting, SettingsStore,
		SettingsWarning, WindowLayout,
	},
};
use std::{
	path::Path,
	sync::Arc,
	time::{Duration, Instant},
};

/// Why the stylesheet in force is not the one the reader asked for.
///
/// Like [`SettingsWarning`], it is kept as the failure rather than as text, so
/// it can be spelled in whichever interface language is in force when it is
/// finally drawn.
#[derive(Debug)]
pub(super) enum StyleWarning {
	/// The chosen styles were read, but loading them failed.
	Load(anyhow::Error),
	/// Applying the font overrides on top of them failed.
	Override(anyhow::Error),
	/// The result was not a stylesheet this build can use.
	Invalid(anyhow::Error),
}
impl StyleWarning {
	/// The warning in `lang`, spelled as late as possible: the failure only
	/// costs an allocation when there is one to spell.
	pub(super) fn text(&self, lang: Lang) -> std::borrow::Cow<'static, str> {
		use std::borrow::Cow;
		match self {
			Self::Load(error)
			| Self::Override(error)
			| Self::Invalid(error) => {
				Cow::Owned(lang.status_styles_failed(format!("{error:#}")))
			}
		}
	}
}

pub(super) struct Preferences {
	pub(super) ui: markview_ui_core::Runtime<super::ui_model::Model>,
	/// The export panel's own settings. They are never part of `values`, so
	/// changing one cannot reflow the document.
	store: SettingsStore,
	pub(super) style_entries: Vec<crate::stylesheet::Entry>,
	pub(super) style_warning: Option<StyleWarning>,
	pub(super) settings_warning: Option<SettingsWarning>,
	save_at: Option<Instant>,
	launch_window_layout: WindowLayout,
}
impl std::ops::Deref for Preferences {
	type Target = super::ui_model::Model;
	fn deref(&self) -> &Self::Target {
		&self.ui.component
	}
}
impl std::ops::DerefMut for Preferences {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.ui.component
	}
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
			settings_warning = Some(SettingsWarning::InitFailed(error));
		}
		let mut settings = settings_store.settings();
		// A diagnostic image cannot be scrolled sideways, so its code blocks
		// wrap by default.
		if args.mode.wraps_code_blocks() {
			settings.codeblock_wrap = true;
		}
		let explicit = ReaderSettings {
			window_layout: settings.window_layout,
			tab_style: settings.tab_style,
			fontdef_overrides: settings.fontdef_overrides.clone(),
			style: args.style.clone().or_else(|| {
				args.theme.map(|t| {
					vec![if t == Theme::Dark { "dark" } else { "light" }.into()]
				})
			}),
			lang: settings.lang,
			user_agent: settings.user_agent.clone(),
			accept_language: settings.accept_language.clone(),
			stylesheet: args.options.stylesheet.clone(),
			theme: args.theme.unwrap_or_default(),
			font_size: args.options.font_size,
			width: args.options.width,
			folder_width: settings.folder_width,
			outline_width: settings.outline_width,
			tabs_width: settings.tabs_width,
			justify: args.options.justify,
			hyphenate: args.options.hyphenate,
			justification: settings.justification,
			paragraph_indent: args.options.paragraph_indent,
			cjk_type: args.cjk_type.unwrap_or(settings.cjk_type),
			markdown_flavor: settings.markdown_flavor,
			hard_breaks: settings.hard_breaks,
			highlight_marks: settings.highlight_marks,
			codeblock_theme_override: settings.codeblock_theme_override.clone(),
			codeblock_wrap: settings.codeblock_wrap,
			scroll_speed: settings.scroll_speed,
			single_instance: settings.single_instance,
			restore_session: settings.restore_session,
			footer_path: settings.footer_path,
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
				Err(e) => style_warning = Some(StyleWarning::Load(e)),
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
				Err(e) => style_warning = Some(StyleWarning::Override(e)),
			}
		}
		if style_warning.is_none()
			&& let Err(error) = ui.validate_stylesheet(&settings.stylesheet)
		{
			style_warning = Some(StyleWarning::Invalid(error));
		}
		ui.set_stylesheet(settings.stylesheet.clone());
		ui.appearance = settings.stylesheet.text(
			&markview_layout::style::TextAppearance::default(),
			markview_layout::style::Condition::Ui,
		);
		let export = settings_store.export();
		Self {
			launch_window_layout: settings.window_layout,
			ui: markview_ui_core::Runtime::new(super::ui_model::Model {
				settings: super::settings_panel::Model::new(settings),
				export,
				fonts: Default::default(),
			}),
			store: settings_store,
			style_entries,
			style_warning,
			settings_warning,
			save_at: None,
		}
	}
	pub(super) fn reload_styles(
		&mut self,
		ui: &mut TextShaper,
	) -> Option<bool> {
		let ids = self.ui.component.values.style.clone().unwrap_or_else(|| {
			vec![
				if self.ui.component.values.theme == Theme::Dark {
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
			self.ui.component.values.cjk_type,
		) {
			Ok(sheet) => {
				let mut sheet = (*sheet).clone();
				sheet.set_media(self.ui.component.values.stylesheet.media());
				let sheet = Arc::new(sheet);
				let result = crate::stylesheet::apply_font_overrides(
					sheet,
					&self.ui.component.values.fontdef_overrides,
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
							.rule(markview_layout::style::Condition::CodeBlock)
							.theme
							.clone();
						let new_codeblock_theme = sheet
							.rule(markview_layout::style::Condition::CodeBlock)
							.theme
							.clone();
						let reflow = sheet.layout_key()
							!= self.ui.component.values.stylesheet.layout_key()
							|| old_codeblock_theme != new_codeblock_theme
							// Diagrams are pixels, so a new diagram theme
							// needs a layout request to redraw them. The key
							// follows the font definitions the table names.
							|| self.ui.component.values.stylesheet.diagram_key()
								!= sheet.diagram_key();
						self.ui.component.values.stylesheet = sheet.clone();
						ui.set_stylesheet(sheet.clone());
						ui.appearance = sheet.text(
							&markview_layout::style::TextAppearance::default(),
							markview_layout::style::Condition::Ui,
						);
						self.style_warning = None;
						return Some(reflow);
					}
					Err(e) => {
						self.style_warning = Some(StyleWarning::Override(e))
					}
				}
			}
			Err(e) => self.style_warning = Some(StyleWarning::Load(e)),
		}
		None
	}

	pub(super) fn restart_pending(&self) -> bool {
		self.ui.component.values.window_layout != self.launch_window_layout
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
		self.ui.component.export = export.clone();
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
		self.store.changed(&self.ui.component.values, field);
		self.schedule_save();
	}
	pub(super) fn reload(&mut self) -> bool {
		match self.store.reload() {
			Ok(_) => {
				self.settings_warning = None;
				self.ui.component.export = self.store.export();
				if self.store.has_pending_changes() {
					self.save_at =
						Some(Instant::now() + Duration::from_millis(250));
				}
				true
			}
			Err(error) => {
				self.settings_warning = Some(SettingsWarning::Invalid(error));
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
					Some(SettingsWarning::SaveFailed(error));
				false
			}
		}
	}
}

#[cfg(test)]
mod tests;
