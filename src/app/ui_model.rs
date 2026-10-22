//! The chrome's parent component routes typed page messages and effects.
use super::{export_panel, font_panel, settings_panel};
use crate::settings::ExportSettings;
use markview_ui_core::{Component, Update};

pub(super) struct Model {
	pub settings: settings_panel::Model,
	pub export: ExportSettings,
	pub fonts: font_panel::FontPanel,
}
impl std::ops::Deref for Model {
	type Target = settings_panel::Model;
	fn deref(&self) -> &Self::Target {
		&self.settings
	}
}
impl std::ops::DerefMut for Model {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.settings
	}
}
pub(super) enum Message {
	Settings(settings_panel::Message),
	Export(export_panel::Message),
	Fonts(font_panel::Message),
}
pub(super) enum Effect {
	Settings(settings_panel::Effect),
	PersistExport,
	Fonts(font_panel::Effect),
}
pub(super) struct View<'a> {
	pub settings: &'a settings_panel::Model,
	pub export: &'a ExportSettings,
	pub fonts: font_panel::View<'a>,
}
impl Component for Model {
	type Message = Message;
	type Effect = Effect;
	type View<'a> = View<'a>;
	fn view(&self) -> View<'_> {
		View {
			settings: &self.settings,
			export: self.export.view(),
			fonts: self.fonts.view(),
		}
	}
	fn update(&mut self, message: Message) -> Update<Effect> {
		match message {
			Message::Settings(message) => {
				self.settings.update(message).map(Effect::Settings)
			}
			Message::Export(message) => {
				self.export.update(message).map(|()| Effect::PersistExport)
			}
			Message::Fonts(message) => {
				self.fonts.update(message).map(Effect::Fonts)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::settings::{ExportFormat, ReaderSettings};
	use markview_ui_core::{Invalidation, Runtime};

	#[test]
	fn page_updates_share_a_queue_and_keep_reader_and_export_state_independent()
	{
		let mut runtime = Runtime::new(Model {
			settings: settings_panel::Model::new(ReaderSettings::default()),
			export: ExportSettings::default(),
			fonts: Default::default(),
		});
		runtime.send(Message::Settings(settings_panel::Message::TogglePreview));
		let preview = runtime.flush();
		assert_eq!(preview.invalidation, Invalidation::Paint);
		assert!(runtime.component.settings.settings_preview);
		assert!(preview.effects.is_empty());
		let reader_size = runtime.view().settings.font_size;
		let export_size = runtime.view().export.font_size;
		runtime.send(Message::Export(export_panel::Message::Size(1)));
		runtime.send(Message::Export(export_panel::Message::Format(
			ExportFormat::Png,
		)));
		let update = runtime.flush();
		assert_eq!(update.invalidation, Invalidation::Layout);
		assert!(
			update
				.effects
				.iter()
				.all(|effect| matches!(effect, Effect::PersistExport))
		);
		assert_eq!(runtime.view().export.font_size, export_size + 1.0);
		assert_eq!(runtime.view().settings.font_size, reader_size);
		runtime.send(Message::Settings(settings_panel::Message::Size(1)));
		assert!(runtime.flush().effects.iter().any(|effect| matches!(
			effect,
			Effect::Settings(settings_panel::Effect::Reflow)
		)));
		assert_eq!(runtime.view().settings.font_size, reader_size + 1.0);
		assert_eq!(runtime.view().export.format, ExportFormat::Png);
	}

	#[test]
	fn settings_and_export_offsets_belong_to_their_own_pages() {
		use crate::state::InteractionState;
		let mut runtime = Runtime::new(Model {
			settings: settings_panel::Model::new(ReaderSettings::default()),
			export: ExportSettings::default(),
			fonts: Default::default(),
		});
		*runtime.component.settings.settings_scroll_mut() = 40.0.into();
		let mut interaction = InteractionState::default();
		*interaction.panels.export_scroll_mut() = 80.0.into();
		*interaction.panels.styles_scroll_mut() = 20.0.into();

		// A settings update leaves the export and styles offsets alone.
		runtime.send(Message::Settings(settings_panel::Message::Size(1)));
		runtime.flush();
		assert_eq!(runtime.component.settings.settings_scroll().offset(), 40.0);
		assert_eq!(interaction.panels.export_scroll().offset(), 80.0);
		assert_eq!(interaction.panels.styles_scroll().offset(), 20.0);

		// An export update moves the export settings, not either page offset.
		runtime.send(Message::Export(export_panel::Message::Size(1)));
		runtime.flush();
		assert_eq!(runtime.component.settings.settings_scroll().offset(), 40.0);
		assert_eq!(interaction.panels.export_scroll().offset(), 80.0);
		assert_eq!(interaction.panels.styles_scroll().offset(), 20.0);
	}

	#[test]
	fn a_panel_round_trip_preserves_the_styles_offset() {
		use crate::state::{InteractionState, PanelPage, PanelTab};
		let mut interaction = InteractionState::default();
		interaction.show_panel(PanelPage::Settings(PanelTab::Styles));
		*interaction.panels.styles_scroll_mut() = 50.0.into();
		interaction.show_panel(PanelPage::Settings(PanelTab::Fonts));
		interaction.show_panel(PanelPage::Settings(PanelTab::Styles));
		assert_eq!(interaction.panels.styles_scroll().offset(), 50.0);
	}
}
