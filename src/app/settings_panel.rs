//! Settings state transitions; persistence and reflow belong to the host.
use crate::state::SettingsCommand;
use crate::{
	scroll::ScrollArea,
	settings::{FontRole, ReaderSettings, Setting, TabStyle, WindowLayout},
	state::Command,
};
use markview_ui_core::{Component, Invalidation, Update};
use std::{ops::Deref, time::Instant};

#[derive(Default)]
pub(super) struct Model {
	pub values: ReaderSettings,
	pub settings_preview: bool,
	/// The settings page's own list offset.
	settings_scroll: ScrollArea,
}
impl Model {
	pub(super) fn new(values: ReaderSettings) -> Self {
		Self {
			values,
			..Default::default()
		}
	}

	pub(super) fn settings_scroll(&self) -> &ScrollArea {
		&self.settings_scroll
	}

	pub(super) fn settings_scroll_mut(&mut self) -> &mut ScrollArea {
		&mut self.settings_scroll
	}

	/// Clears the page's transient state when it is opened afresh.
	pub(super) fn reset_page(&mut self) {
		self.settings_scroll.reset();
		self.settings_preview = false;
	}

	pub(super) fn advance_scroll(&mut self, now: Instant) {
		self.settings_scroll.advance(now);
	}

	pub(super) fn scroll_deadline(&self, now: Instant) -> Option<Instant> {
		self.settings_scroll.deadline(now)
	}

	pub(super) fn scroll_animating(&self) -> bool {
		self.settings_scroll.animating()
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
	Indent(u8),
	CjkType(markview_layout::style::CjkType),
	Language(Option<crate::lang::Lang>),
	FontFamily(FontRole, Option<String>),
	WindowLayout(WindowLayout),
	TabStyle(TabStyle),
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
			Command::Settings(SettingsCommand::TabStyle(value)) => {
				Self::TabStyle(value)
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
			Message::FooterPath => {
				self.values.footer_path = !self.values.footer_path;
				Setting::FooterPath
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
			Message::TabStyle(value) => {
				self.values.tab_style = value;
				Setting::TabStyle
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
			Setting::WindowLayout | Setting::TabStyle => {
				update.effects.push(Effect::CloseDropdown);
			}
			Setting::Language => {
				update
					.effects
					.extend([Effect::CloseDropdown, Effect::Reflow]);
			}
			Setting::SidebarWidth(sidebar) => {
				update.effects.push(Effect::SidebarResize(sidebar));
			}
			Setting::ScrollSpeed | Setting::FooterPath => {}
			_ => update.effects.push(Effect::Reflow),
		}
		update
	}
}
