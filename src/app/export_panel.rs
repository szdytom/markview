//! Export preferences are a component independent of reader preferences.
use super::export::{MARGIN_PRESETS, PAPER_PRESETS, SCALE_PRESETS};
use crate::state::ExportCommand;
use crate::{
	settings::{ExportFormat, ExportSettings},
	state::Command,
};
use markview_ui_core::{Component, Invalidation, Update};

pub enum Message {
	Style(super::style_edit::Edit),
	Format(ExportFormat),
	Size(i8),
	Indent(u8),
	Paper(u8),
	Orientation(bool),
	Margin(u8),
	Scale(u8),
}
impl Message {
	pub(super) fn from_command(command: Command) -> Option<Self> {
		Some(match command {
			Command::Export(ExportCommand::ExportFormat(value)) => {
				Self::Format(value)
			}
			Command::Export(ExportCommand::ExportSize(delta)) => {
				Self::Size(delta)
			}
			Command::Export(ExportCommand::ExportIndent(value)) => {
				Self::Indent(value)
			}
			Command::Export(ExportCommand::ExportPaper(value)) => {
				Self::Paper(value)
			}
			Command::Export(ExportCommand::ExportOrientation(value)) => {
				Self::Orientation(value)
			}
			Command::Export(ExportCommand::ExportMargin(value)) => {
				Self::Margin(value)
			}
			Command::Export(ExportCommand::ExportScale(value)) => {
				Self::Scale(value)
			}
			_ => return None,
		})
	}
}
impl Component for ExportSettings {
	type Message = Message;
	type Effect = ();
	type View<'a> = &'a Self;
	fn view(&self) -> &Self {
		self
	}
	fn update(&mut self, message: Message) -> Update<()> {
		match message {
			Message::Style(edit) => {
				if !edit.apply(&mut self.style) {
					return Update::new(Invalidation::None);
				}
			}
			Message::Format(value) => self.format = value,
			Message::Size(delta) => {
				self.font_size = (self.font_size + f32::from(delta)).clamp(
					crate::export::config::MIN_FONT_SIZE_PX,
					crate::export::config::MAX_FONT_SIZE_PX,
				)
			}
			Message::Indent(em) => self.paragraph_indent = f32::from(em),
			Message::Paper(index) => {
				let Some((_, paper)) = PAPER_PRESETS.get(index as usize) else {
					return Update::new(Invalidation::None);
				};
				self.paper = (*paper).into();
			}
			Message::Orientation(value) => self.landscape = value,
			Message::Margin(index) => {
				let Some((margin, _)) = MARGIN_PRESETS.get(index as usize)
				else {
					return Update::new(Invalidation::None);
				};
				self.margin = *margin;
			}
			Message::Scale(index) => {
				let Some((scale, _)) = SCALE_PRESETS.get(index as usize) else {
					return Update::new(Invalidation::None);
				};
				self.scale = *scale;
			}
		}
		Update::new(Invalidation::Layout).effect(())
	}
}
