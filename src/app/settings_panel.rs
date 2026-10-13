//! Settings state transitions; persistence and reflow belong to the host.
use crate::{
	settings::{FontRole, ReaderSettings, Setting, TabStyle, WindowLayout},
	state::Command,
};
use markview_ui_core::{Component, Invalidation, Update};

pub(super) struct Model {
	pub values: ReaderSettings,
	pub preview: bool,
}
pub(super) struct View<'a> {
	pub values: &'a ReaderSettings,
}

pub(super) enum Message {
	TogglePreview,
	ResetPreview,
	Style(super::style_edit::Edit),
	Size(i8),
	Width(i8),
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
			Command::SettingsPreview => Self::TogglePreview,
			Command::Smaller => Self::Size(-1),
			Command::Larger => Self::Size(1),
			Command::Narrower => Self::Width(-1),
			Command::Wider => Self::Width(1),
			Command::Align => Self::Align,
			Command::Hyphens => Self::Hyphens,
			Command::CodeWrap => Self::CodeWrap,
			Command::Indent(em) => Self::Indent(em),
			Command::CjkType(value) => Self::CjkType(value),
			Command::Language(value) => Self::Language(value),
			Command::WindowLayout(value) => Self::WindowLayout(value),
			Command::TabStyle(value) => Self::TabStyle(value),
			Command::SingleInstance => Self::SingleInstance,
			Command::RestoreSession => Self::RestoreSession,
			Command::ScrollSpeed(delta) => Self::ScrollSpeed(delta),
			_ => return None,
		})
	}
}
pub(super) enum Effect {
	Preview(bool),
	Persist(Option<Setting>),
	Reflow,
	ReloadStyles,
	CloseDropdown,
	SyncSession,
	Flush,
}
impl Component for Model {
	type Message = Message;
	type Effect = Effect;
	type View<'a> = View<'a>;
	fn view(&self) -> View<'_> {
		View {
			values: &self.values,
		}
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
				self.preview =
					matches!(message, Message::TogglePreview) && !self.preview;
				return Update::new(Invalidation::Paint)
					.effect(Effect::Preview(self.preview));
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
				self.preview = false;
				return update
					.effect(Effect::Preview(false))
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
			Setting::ScrollSpeed => {}
			_ => update.effects.push(Effect::Reflow),
		}
		update
	}
}
