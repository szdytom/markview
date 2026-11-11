//! Settings state transitions; persistence and reflow belong to the host.
use crate::state::SettingsCommand;
use crate::{
	settings::{
		FontRole, MarkdownFlavor, ReaderSettings, Setting, WindowLayout,
	},
	state::Command,
};
use markview_interaction::Scroll;
use markview_ui_core::{Component, Invalidation, Update};
use std::ops::Deref;

#[derive(Default)]
pub(super) struct Model {
	pub values: ReaderSettings,
	pub settings_preview: bool,
	/// The settings page's own list offset.
	settings_scroll: Scroll,
}
impl Model {
	pub(super) fn new(values: ReaderSettings) -> Self {
		Self {
			values,
			..Default::default()
		}
	}

	pub(super) fn settings_scroll(&self) -> &Scroll {
		&self.settings_scroll
	}

	pub(super) fn settings_scroll_mut(&mut self) -> &mut Scroll {
		&mut self.settings_scroll
	}

	/// Clears the page's transient state when it is opened afresh.
	pub(super) fn reset_page(&mut self) {
		self.settings_scroll.reset();
		self.settings_preview = false;
	}
}
impl Deref for Model {
	type Target = ReaderSettings;
	fn deref(&self) -> &ReaderSettings {
		&self.values
	}
}
pub(super) enum Message {
	TogglePreview,
	ResetPreview,
	Style(super::style_edit::Edit),
	Size(i8),
	Width(i8),
	SidebarWidth(crate::settings::Sidebar, i8),
	Align,
	Hyphens,
	CodeWrap,
	CodeCopy,
	MarkdownFlavor(MarkdownFlavor),
	HardBreaks,
	HighlightMarks,
	Indent(u8),
	CjkType(markview_layout::style::CjkType),
	Language(Option<crate::lang::Lang>),
	FontFamily(FontRole, Option<String>),
	WindowLayout(WindowLayout),
	SingleInstance,
	RestoreSession,
	FooterPath,
	ScrollSpeed(i8),
	Reset(Box<ReaderSettings>),
}
impl Message {
	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Keep the flat command-to-settings-message mapping together for review against the command enum."
		)
	)]
	pub(super) fn from_command(command: Command) -> Option<Self> {
		Some(match command {
			Command::Settings(SettingsCommand::SettingsPreview) => {
				Self::TogglePreview
			}
			Command::Settings(SettingsCommand::Smaller) => Self::Size(-1),
			Command::Settings(SettingsCommand::Larger) => Self::Size(1),
			Command::Settings(SettingsCommand::Narrower) => Self::Width(-1),
			Command::Settings(SettingsCommand::Wider) => Self::Width(1),
			Command::Settings(SettingsCommand::Align) => Self::Align,
			Command::Settings(SettingsCommand::Hyphens) => Self::Hyphens,
			Command::Settings(SettingsCommand::CodeWrap) => Self::CodeWrap,
			Command::Settings(SettingsCommand::CodeCopy) => Self::CodeCopy,
			Command::Settings(SettingsCommand::MarkdownFlavor(value)) => {
				Self::MarkdownFlavor(value)
			}
			Command::Settings(SettingsCommand::HardBreaks) => Self::HardBreaks,
			Command::Settings(SettingsCommand::HighlightMarks) => {
				Self::HighlightMarks
			}
			Command::Settings(SettingsCommand::Indent(em)) => Self::Indent(em),
			Command::Settings(SettingsCommand::CjkType(value)) => {
				Self::CjkType(value)
			}
			Command::Settings(SettingsCommand::Language(value)) => {
				Self::Language(value)
			}
			Command::Settings(SettingsCommand::WindowLayout(value)) => {
				Self::WindowLayout(value)
			}
			Command::Settings(SettingsCommand::SingleInstance) => {
				Self::SingleInstance
			}
			Command::Settings(SettingsCommand::RestoreSession) => {
				Self::RestoreSession
			}
			Command::Settings(SettingsCommand::FooterPath) => Self::FooterPath,
			Command::Settings(SettingsCommand::ScrollSpeed(delta)) => {
				Self::ScrollSpeed(delta)
			}
			Command::Settings(SettingsCommand::SidebarWidth(
				sidebar,
				delta,
			)) => Self::SidebarWidth(sidebar, delta),
			_ => return None,
		})
	}
}
pub(super) enum Effect {
	Persist(Option<Setting>),
	Reflow,
	SidebarResize(crate::settings::Sidebar),
	ReloadStyles,
	CloseDropdown,
	SyncSession,
	Flush,
}
impl Component for Model {
	type Message = Message;
	type Effect = Effect;
	type View<'a> = &'a ReaderSettings;
	fn view(&self) -> &ReaderSettings {
		&self.values
	}
	#[cfg_attr(
		any(),
		markview::allow_complexity(
			reason = "Each match arm owns one settings transition and its ordered persistence, reflow or session effects; keep that correspondence adjacent for review."
		)
	)]
	fn update(&mut self, message: Message) -> Update<Effect> {
		let mut update = Update::new(Invalidation::Layout);
		let field = match message {
			Message::TogglePreview | Message::ResetPreview => {
				self.settings_preview =
					matches!(message, Message::TogglePreview)
						&& !self.settings_preview;
				return Update::new(Invalidation::Paint);
			}
			Message::Style(edit) => {
				let mut ids = self.values.style.clone().unwrap_or_default();
				if !edit.apply(&mut ids) {
					return Update::new(Invalidation::None);
				}
				self.values.style = Some(ids);
				return update.effect(Effect::Persist(Some(Setting::Theme)));
			}
			Message::Size(delta) => {
				let size = self.values.font_size + f32::from(delta);
				self.values.font_size = if delta < 0 {
					size.max(10.0)
				} else {
					size.min(40.0)
				};
				Setting::FontSize
			}
			Message::Width(delta) => {
				let width = self.values.width + f32::from(delta) * 60.0;
				self.values.width = if delta < 0 {
					width.max(240.0)
				} else {
					width.min(1600.0)
				};
				Setting::Width
			}
			Message::SidebarWidth(sidebar, delta) => {
				let width = self.values.sidebar_width(sidebar)
					+ f32::from(delta) * crate::settings::SIDEBAR_WIDTH_STEP;
				self.values.set_sidebar_width(sidebar, width);
				Setting::SidebarWidth(sidebar)
			}
			Message::Align => {
				self.values.justify = !self.values.justify;
				Setting::Justify
			}
			Message::Hyphens => {
				self.values.hyphenate = !self.values.hyphenate;
				Setting::Hyphenate
			}
			Message::CodeWrap => {
				self.values.codeblock_wrap = !self.values.codeblock_wrap;
				Setting::CodeblockWrap
			}
			Message::CodeCopy => {
				self.values.codeblock_copy = !self.values.codeblock_copy;
				Setting::CodeblockCopy
			}
			Message::FooterPath => {
				self.values.footer_path = !self.values.footer_path;
				Setting::FooterPath
			}
			Message::MarkdownFlavor(value) => {
				if self.values.markdown_flavor != value
					&& value == MarkdownFlavor::Obsidian
				{
					self.values.hard_breaks = false;
				}
				self.values.markdown_flavor = value;
				Setting::Dialect
			}
			Message::HardBreaks => {
				self.values.hard_breaks = !self.values.hard_breaks;
				Setting::Dialect
			}
			Message::HighlightMarks => {
				self.values.highlight_marks = !self.values.highlight_marks;
				Setting::Dialect
			}
			Message::Indent(em) => {
				self.values.paragraph_indent = f32::from(em);
				Setting::ParagraphIndent
			}
			Message::CjkType(value) => {
				self.values.cjk_type = value;
				Setting::CjkType
			}
			Message::Language(value) => {
				self.values.lang = value;
				Setting::Language
			}
			Message::FontFamily(role, family) => {
				self.values.set_font_family(role, family);
				Setting::FontFamily
			}
			Message::WindowLayout(value) => {
				self.values.window_layout = value;
				Setting::WindowLayout
			}
			Message::ScrollSpeed(delta) => {
				self.values.step_scroll_speed(delta);
				Setting::ScrollSpeed
			}
			Message::SingleInstance | Message::RestoreSession => {
				if matches!(message, Message::RestoreSession) {
					self.values.restore_session = !self.values.restore_session;
					self.values.single_instance |= self.values.restore_session;
				} else {
					self.values.single_instance = !self.values.single_instance;
					self.values.restore_session &= self.values.single_instance;
				}
				return update
					.effect(Effect::Persist(Some(Setting::RestoreSession)))
					.effect(Effect::Persist(Some(Setting::SingleInstance)))
					.effect(Effect::SyncSession)
					.effect(Effect::Flush);
			}
			Message::Reset(settings) => {
				self.values = *settings;
				self.settings_preview = false;
				return update
					.effect(Effect::Persist(None))
					.effect(Effect::SyncSession)
					.effect(Effect::Reflow);
			}
		};
		update.effects.push(Effect::Persist(Some(field)));
		match field {
			Setting::FontFamily => {
				update
					.effects
					.extend([Effect::ReloadStyles, Effect::CloseDropdown]);
			}
			Setting::WindowLayout => {
				update.effects.push(Effect::CloseDropdown);
			}
			Setting::Language | Setting::Dialect => {
				update
					.effects
					.extend([Effect::CloseDropdown, Effect::Reflow]);
			}
			Setting::SidebarWidth(sidebar) => {
				update.effects.push(Effect::SidebarResize(sidebar));
			}
			Setting::ScrollSpeed
			| Setting::FooterPath
			| Setting::CodeblockCopy => {}
			_ => update.effects.push(Effect::Reflow),
		}
		update
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_ui_core::Component;

	#[test]
	fn dialect_messages_persist_as_one_setting_and_reflow() {
		let mut model = Model {
			values: ReaderSettings {
				markdown_flavor: MarkdownFlavor::Gfm,
				hard_breaks: true,
				..Default::default()
			},
			settings_preview: false,
			..Default::default()
		};
		let update =
			model.update(Message::MarkdownFlavor(MarkdownFlavor::Obsidian));
		assert_eq!(model.values.markdown_flavor, MarkdownFlavor::Obsidian);
		assert!(!model.values.hard_breaks);
		assert!(update.effects.iter().any(|effect| {
			matches!(effect, Effect::Persist(Some(Setting::Dialect)))
		}));
		assert!(
			update
				.effects
				.iter()
				.any(|effect| matches!(effect, Effect::Reflow))
		);
		assert!(
			update
				.effects
				.iter()
				.any(|effect| matches!(effect, Effect::CloseDropdown))
		);

		let update = model.update(Message::HardBreaks);
		assert!(model.values.hard_breaks);
		assert!(model.values.effective_dialect().hard_breaks);
		assert!(update.effects.iter().any(|effect| {
			matches!(effect, Effect::Persist(Some(Setting::Dialect)))
		}));

		let update = model.update(Message::HighlightMarks);
		assert!(model.values.highlight_marks);
		assert!(update.effects.iter().any(|effect| {
			matches!(effect, Effect::Persist(Some(Setting::Dialect)))
		}));
		// Only highlights are forced by the Obsidian preset.
		let dialect = model.values.effective_dialect();
		assert!(dialect.hard_breaks);
		assert!(dialect.highlight_marks);

		model.update(Message::MarkdownFlavor(MarkdownFlavor::Obsidian));
		assert!(model.values.hard_breaks);
		model.update(Message::MarkdownFlavor(MarkdownFlavor::Gfm));
		assert!(model.values.effective_dialect().hard_breaks);
		model.update(Message::MarkdownFlavor(MarkdownFlavor::Obsidian));
		assert!(!model.values.effective_dialect().hard_breaks);
	}
}
