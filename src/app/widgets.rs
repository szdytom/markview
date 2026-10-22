//! Adapt the existing self-drawn controls to the common interaction protocol.
use super::{App, Button};
use crate::state::{ChromeCommand, DocumentCommand};
use crate::state::{Command, InteractionState};
use markview_ui_core::{Input, Layout, Widget, WidgetId};
use std::{
	hash::{Hash, Hasher},
	ops::Deref,
};

/// What one paint pass reads: the interaction state and the placed control
/// geometry from the last layout.
///
/// The geometry is a draw product, so it travels with the paint pass rather
/// than the interaction state. The deref keeps every other read unchanged.
#[derive(Clone, Copy)]
pub(in crate::app) struct Drawn<'a> {
	pub(in crate::app) interaction: &'a InteractionState,
	pub(in crate::app) controls: Option<&'a Layout<Command>>,
}
impl<'a> Drawn<'a> {
	pub(in crate::app) fn new(
		interaction: &'a InteractionState,
		controls: Option<&'a Layout<Command>>,
	) -> Self {
		Self {
			interaction,
			controls,
		}
	}
}
impl Deref for Drawn<'_> {
	type Target = InteractionState;
	fn deref(&self) -> &InteractionState {
		self.interaction
	}
}

/// One geometry product for paint, pointer input and keyboard traversal.
pub(super) struct UiLayout {
	pub buttons: Vec<Button>,
	pub focus: Vec<Button>,
	pub form: Option<super::chrome::components::Form>,
	pub hit: Layout<Command>,
}
/// Default control identities. Pages override row keys with their model IDs.
pub(crate) fn id(command: Command) -> WidgetId {
	command.widget_id()
}

/// A command's identity: the outer domain and the domain variant together
/// scope the hash, and `key` carries the variant and payload.
///
/// Hashing both discriminants keeps `scope` unique across the whole flat
/// command surface, as the pre-split enum was. Some call sites compare scopes
/// alone; domains keep independent variant indices, so hashing the inner
/// discriminant by itself would make unrelated commands share a scope.
///
/// The key is rendered from `Debug`, so identity is only stable within one
/// run; nesting a command in its domain changes the text, not the mapping
/// between a press and the release it activates.
pub(crate) fn identity<T: std::fmt::Debug>(
	domain: &std::mem::Discriminant<Command>,
	command: &T,
	key: String,
) -> WidgetId {
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	domain.hash(&mut hash);
	std::mem::discriminant(command).hash(&mut hash);
	let scope = hash.finish();
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	key.hash(&mut hash);
	WidgetId::new(scope, hash.finish())
}

pub(super) fn row_id<K: Hash + ?Sized>(scope: u64, key: &K) -> WidgetId {
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	key.hash(&mut hash);
	WidgetId::new(scope, hash.finish())
}

impl Button {
	pub(super) fn widget(&self) -> Widget<Command> {
		Widget {
			id: self.id,
			rect: self.rect,
			clip: self.clip,
			enabled: self.enabled,
			focusable: self.action
				!= Command::Document(DocumentCommand::CopyPath)
				&& !(self.action == Command::Chrome(ChromeCommand::Tabs)
					&& self.label.is_empty()),
			message: self.action,
		}
	}
	pub(super) fn contains(&self, point: (f32, f32)) -> bool {
		self.widget()
			.hit_rect()
			.is_some_and(|rect| rect.contains(point.0, point.1))
	}
}

pub(super) fn layout(buttons: &[Button]) -> Layout<Command> {
	Layout {
		widgets: buttons.iter().map(Button::widget).collect(),
	}
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn press_widget(&mut self, buttons: &[Button]) {
		let layout = layout(buttons);
		self.interaction
			.widgets
			.handle(&layout, Input::Press(self.interaction.pointer.cursor()));
	}
	pub(super) fn release_widget(&mut self) -> Option<Command> {
		let buttons = self.buttons();
		self.interaction.widgets.handle(
			&layout(&buttons),
			Input::Release(self.interaction.pointer.cursor()),
		)
	}
	pub(super) fn navigate_widgets(
		&mut self,
		backward: bool,
	) -> Option<Command> {
		let buttons = self.focus_buttons();
		let layout = layout(&buttons);
		self.interaction.tab_focus(&layout, backward)
	}
}

#[cfg(test)]
pub(crate) fn command_layout(commands: &[Command]) -> Layout<Command> {
	Layout {
		widgets: commands
			.iter()
			.map(|command| Widget {
				id: command.widget_id(),
				rect: Default::default(),
				clip: None,
				enabled: true,
				focusable: true,
				message: *command,
			})
			.collect(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		app::font_panel,
		settings::{ExportFormat, FontRole, Sidebar, TabStyle, WindowLayout},
		state::{
			DropdownId, ExportCommand, PanelTab, SearchCommand,
			SettingsCommand, SidebarTab, TextField,
		},
	};
	use markview_layout::style::CjkType;
	use markview_ui_core::Interaction;

	/// One representative of every command in the flat pre-split surface.
	fn every_command() -> Vec<Command> {
		vec![
			Command::Search(SearchCommand::SearchOpen),
			Command::Settings(SettingsCommand::RetrySettingsLoad),
			Command::Search(SearchCommand::SearchCase),
			Command::Search(SearchCommand::SearchWord),
			Command::Search(SearchCommand::SearchPrevious),
			Command::Search(SearchCommand::SearchNext),
			Command::Search(SearchCommand::SearchClose),
			Command::Chrome(ChromeCommand::FocusInput(TextField::Search)),
			Command::Settings(SettingsCommand::WindowLayout(
				WindowLayout::System,
			)),
			Command::Settings(SettingsCommand::TabStyle(TabStyle::Underline)),
			Command::Document(DocumentCommand::OpenUrl),
			Command::Document(DocumentCommand::NewPage),
			Command::Document(DocumentCommand::Open),
			Command::Chrome(ChromeCommand::OpenFolder),
			Command::Chrome(ChromeCommand::Folder),
			Command::Chrome(ChromeCommand::FolderRoot),
			Command::Chrome(ChromeCommand::FolderEntry(0)),
			Command::Chrome(ChromeCommand::FolderRefresh),
			Command::Chrome(ChromeCommand::FolderCollapseAll),
			Command::Chrome(ChromeCommand::RevealFolder),
			Command::Settings(SettingsCommand::Smaller),
			Command::Settings(SettingsCommand::Larger),
			Command::Settings(SettingsCommand::Narrower),
			Command::Settings(SettingsCommand::Wider),
			Command::Settings(SettingsCommand::Align),
			Command::Settings(SettingsCommand::Hyphens),
			Command::Settings(SettingsCommand::CodeWrap),
			Command::Settings(SettingsCommand::SingleInstance),
			Command::Settings(SettingsCommand::RestoreSession),
			Command::Settings(SettingsCommand::FooterPath),
			Command::Settings(SettingsCommand::ScrollSpeed(1)),
			Command::Settings(SettingsCommand::SidebarWidth(
				Sidebar::Folder,
				1,
			)),
			Command::Settings(SettingsCommand::Indent(1)),
			Command::Settings(SettingsCommand::CjkType(CjkType::Sc)),
			Command::Settings(SettingsCommand::Language(None)),
			Command::Settings(SettingsCommand::FontFamily(
				FontRole::Serif,
				None,
			)),
			Command::Chrome(ChromeCommand::ToggleDropdown(
				DropdownId::Language,
				0,
			)),
			Command::Export(ExportCommand::Export),
			Command::Export(ExportCommand::ExportFormat(ExportFormat::Pdf)),
			Command::Export(ExportCommand::ExportSize(1)),
			Command::Export(ExportCommand::ExportIndent(1)),
			Command::Export(ExportCommand::ExportPaper(1)),
			Command::Export(ExportCommand::ExportOrientation(true)),
			Command::Export(ExportCommand::ExportMargin(1)),
			Command::Export(ExportCommand::ExportScale(1)),
			Command::Export(ExportCommand::ExportAndWatch),
			Command::Export(ExportCommand::ExportStyles),
			Command::Export(ExportCommand::ExportStyleToggle(0)),
			Command::Export(ExportCommand::ExportStyleUp(0)),
			Command::Export(ExportCommand::ExportStyleDown(0)),
			Command::Export(ExportCommand::ExportRun),
			Command::Chrome(ChromeCommand::Settings),
			Command::Settings(SettingsCommand::SettingsPreview),
			Command::Chrome(ChromeCommand::CopyDiagnostics),
			Command::Document(DocumentCommand::CopyPath),
			Command::Chrome(ChromeCommand::OpenProject),
			Command::Settings(SettingsCommand::Reset),
			Command::Settings(SettingsCommand::OpenConfig),
			Command::Settings(SettingsCommand::SystemTheme),
			Command::Settings(SettingsCommand::Styles),
			Command::Settings(SettingsCommand::StyleToggle(0)),
			Command::Settings(SettingsCommand::StyleUp(0)),
			Command::Settings(SettingsCommand::StyleDown(0)),
			Command::Settings(SettingsCommand::StylesFolder),
			Command::Chrome(ChromeCommand::SettingsTab(PanelTab::Generic)),
			Command::Fonts(font_panel::Command::OpenFolder),
			Command::Chrome(ChromeCommand::Tabs),
			Command::Chrome(ChromeCommand::SidebarTab(SidebarTab::Tabs)),
			Command::Chrome(ChromeCommand::SelectTab(0)),
			Command::Chrome(ChromeCommand::CloseTab(0)),
			Command::Chrome(ChromeCommand::ModalDismiss),
			Command::Chrome(ChromeCommand::ModalOpenFolder),
			Command::Chrome(ChromeCommand::ModalConfirm),
			Command::Document(DocumentCommand::RemoteDismiss),
			Command::Document(DocumentCommand::RemoteLoadAll),
			Command::Document(DocumentCommand::Outline),
			Command::Document(DocumentCommand::OutlineGoto(0)),
			Command::Document(DocumentCommand::OutlineToggle(0)),
			Command::Document(DocumentCommand::OutlineExpandAll),
			Command::Document(DocumentCommand::OutlineCollapseAll),
		]
	}

	fn scope(command: Command) -> u64 {
		command.widget_id().scope
	}

	#[test]
	fn every_flat_command_has_a_unique_scope() {
		let commands = every_command();
		assert_eq!(commands.len(), 80);
		let mut scopes = std::collections::HashSet::new();
		for command in &commands {
			assert!(
				scopes.insert(scope(*command)),
				"{command:?} shares a scope with another command"
			);
		}
		for command in &commands {
			assert_eq!(command.widget_id(), command.widget_id());
		}
	}

	#[test]
	fn nested_domains_do_not_share_scopes_with_unrelated_commands() {
		// `app/chrome/outline.rs::normalize` compares scopes with
		// `OutlineGoto` and `OutlineToggle`; `clear_folder_rows` in
		// `app/folder.rs` compares against `FolderEntry`. Domain variant
		// indices overlap, so these pairs only stay distinct while the outer
		// command discriminant is hashed too.
		for (nested, unrelated) in [
			(
				Command::Document(DocumentCommand::OutlineGoto(0)),
				Command::Settings(SettingsCommand::Align),
			),
			(
				Command::Document(DocumentCommand::OutlineToggle(0)),
				Command::Settings(SettingsCommand::Hyphens),
			),
			(
				Command::Chrome(ChromeCommand::FolderEntry(0)),
				Command::Settings(SettingsCommand::Larger),
			),
		] {
			assert_ne!(scope(nested), scope(unrelated));
		}
	}

	#[test]
	fn press_and_release_resolve_the_same_widget() {
		let commands = every_command();
		let layout = Layout {
			widgets: commands
				.iter()
				.enumerate()
				.map(|(index, command)| Widget {
					id: command.widget_id(),
					rect: markview_ui_core::Rect {
						x: index as f32 * 10.0,
						y: 0.0,
						w: 10.0,
						h: 10.0,
					},
					clip: None,
					enabled: true,
					focusable: true,
					message: *command,
				})
				.collect(),
		};
		for (index, command) in commands.iter().enumerate() {
			let point = (index as f32 * 10.0 + 5.0, 5.0);
			let mut interaction = Interaction::default();
			interaction.handle(&layout, Input::Press(point));
			assert_eq!(interaction.captured, Some(command.widget_id()));
			assert_eq!(
				interaction.handle(&layout, Input::Release(point)),
				Some(*command)
			);
		}
	}

	#[test]
	fn dropdown_identity_ignores_the_highlighted_option() {
		let first = Command::Chrome(ChromeCommand::ToggleDropdown(
			DropdownId::Language,
			0,
		));
		let second = Command::Chrome(ChromeCommand::ToggleDropdown(
			DropdownId::Language,
			2,
		));
		assert_eq!(first.widget_id(), second.widget_id());
	}
}
