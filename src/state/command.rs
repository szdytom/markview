//! Commands grouped by the state their handlers own.
use super::{DropdownId, PanelTab, SidebarTab, TextField};
use crate::app::font_panel;
use markview_ui_core::WidgetId;

/// A command names one domain; the handler for that domain owns the state it
/// changes. The domains stay flat so adding a command touches one enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
	Document(DocumentCommand),
	Settings(SettingsCommand),
	Export(ExportCommand),
	Chrome(ChromeCommand),
	Search(SearchCommand),
	Fonts(font_panel::Command),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocumentCommand {
	OpenUrl,
	NewPage,
	Open,
	/// Copy the active document's absolute path to the clipboard.
	CopyPath,
	/// Hide the remote-image banner, keeping the current fetch limit.
	RemoteDismiss,
	/// Lift the remote-image limit for the current document revision.
	RemoteLoadAll,
	/// Open or close the table-of-contents drawer.
	Outline,
	/// Scroll the document to the heading of one outline entry.
	OutlineGoto(usize),
	OutlineToggle(usize),
	OutlineExpandAll,
	OutlineCollapseAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsCommand {
	RetrySettingsLoad,
	WindowLayout(crate::settings::WindowLayout),
	TabStyle(crate::settings::TabStyle),
	Smaller,
	Larger,
	Narrower,
	Wider,
	Align,
	Hyphens,
	CodeWrap,
	MarkdownFlavor(crate::settings::MarkdownFlavor),
	HardBreaks,
	HighlightMarks,
	SingleInstance,
	RestoreSession,
	/// Show the open document's path in the idle footer and copy it on click.
	FooterPath,
	/// Step the reader's scroll-speed multiplier by whole steps.
	ScrollSpeed(i8),
	SidebarWidth(crate::settings::Sidebar, i8),
	/// First-line paragraph indent in whole em units.
	Indent(u8),
	CjkType(markview_layout::style::CjkType),
	/// The interface language; `None` follows the system again.
	Language(Option<crate::lang::Lang>),
	/// Pick the family a font role shapes with; `None` restores the
	/// stylesheet's own candidate chain. Selections address the catalogue
	/// generation that supplied the chooser's shared name.
	FontFamily(
		crate::settings::FontRole,
		Option<crate::app::font_panel::Selection>,
	),
	SettingsPreview,
	Reset,
	OpenConfig,
	SystemTheme,
	Styles,
	StyleToggle(usize),
	StyleUp(usize),
	StyleDown(usize),
	StylesFolder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportCommand {
	/// Open or close the export panel.
	Export,
	ExportFormat(crate::settings::ExportFormat),
	/// Step the export's text size by whole pixels.
	ExportSize(i8),
	/// First-line indent preset, in em units.
	ExportIndent(u8),
	/// Paper preset index.
	ExportPaper(u8),
	/// Paper orientation; `true` is landscape.
	ExportOrientation(bool),
	/// Margin preset index.
	ExportMargin(u8),
	/// PNG scale preset index.
	ExportScale(u8),
	/// Export once, then keep re-exporting whenever the document changes.
	ExportAndWatch,
	/// Show the export's stylesheet chooser in place of the export panel.
	ExportStyles,
	ExportStyleToggle(usize),
	ExportStyleUp(usize),
	ExportStyleDown(usize),
	/// Write the document with the current export settings.
	ExportRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChromeCommand {
	FocusInput(TextField),
	OpenFolder,
	Folder,
	FolderRoot,
	FolderEntry(usize),
	FolderRefresh,
	FolderCollapseAll,
	/// Reveal the active document's folder in the file manager.
	RevealFolder,
	/// Open a control's option list on the option in force, or close it again.
	ToggleDropdown(DropdownId, usize),
	Settings,
	CopyDiagnostics,
	OpenProject,
	/// Show one page of the settings panel.
	SettingsTab(PanelTab),
	Tabs,
	SidebarTab(SidebarTab),
	SelectTab(usize),
	CloseTab(usize),
	/// Dismiss the local-file confirmation without opening anything.
	ModalDismiss,
	/// Open the directory containing the file the confirmation names.
	ModalOpenFolder,
	/// Hand the confirmed local file to the operating system.
	ModalConfirm,
}

// The variants keep their pre-split spellings, so the shared prefix stays.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchCommand {
	SearchOpen,
	SearchCase,
	SearchWord,
	SearchPrevious,
	SearchNext,
	SearchClose,
}

impl Command {
	/// The flat spelling the external debug snapshot has always exposed.
	///
	/// The domain enums are an implementation detail there: `Fonts` was
	/// already a flat variant before the split, so it keeps its prefix.
	#[cfg(any(debug_assertions, test))]
	pub(crate) fn debug_name(self) -> String {
		match self {
			Self::Document(command) => format!("{command:?}"),
			Self::Settings(command) => format!("{command:?}"),
			Self::Export(command) => format!("{command:?}"),
			Self::Chrome(command) => format!("{command:?}"),
			Self::Search(command) => format!("{command:?}"),
			Self::Fonts(command) => format!("Fonts({command:?})"),
		}
	}

	pub(crate) fn widget_id(self) -> WidgetId {
		let domain = std::mem::discriminant(&self);
		match self {
			Self::Document(command) => command.widget_id(&domain),
			Self::Settings(command) => command.widget_id(&domain),
			Self::Export(command) => command.widget_id(&domain),
			Self::Chrome(command) => command.widget_id(&domain),
			Self::Search(command) => command.widget_id(&domain),
			Self::Fonts(command) => crate::app::widgets::identity(
				&domain,
				&command,
				format!("{command:?}"),
			),
		}
	}

	/// Whether the command addresses a position or availability in the
	/// settings catalogue, which may still be loading.
	pub(crate) fn settings_dependent(self) -> bool {
		use font_panel::Command as Font;
		matches!(
			self,
			Self::Settings(
				SettingsCommand::StyleToggle(_)
					| SettingsCommand::StyleUp(_)
					| SettingsCommand::StyleDown(_)
					| SettingsCommand::SystemTheme
					| SettingsCommand::FontFamily(_, _)
			) | Self::Export(
				ExportCommand::ExportStyleToggle(_)
					| ExportCommand::ExportStyleUp(_)
					| ExportCommand::ExportStyleDown(_)
			) | Self::Chrome(ChromeCommand::ToggleDropdown(
				DropdownId::Font(_),
				_,
			)) | Self::Fonts(
				Font::DownloadMissing
					| Font::DownloadAll
					| Font::DownloadOne(_)
					| Font::RedownloadOne(_)
			)
		)
	}

	/// The pre-dispatch check that keeps a command off an unavailable
	/// settings catalogue.
	pub(crate) fn settings_action_enabled(self, load_blocked: bool) -> bool {
		!load_blocked || !self.settings_dependent()
	}
}

impl DocumentCommand {
	fn widget_id(self, domain: &std::mem::Discriminant<Command>) -> WidgetId {
		crate::app::widgets::identity(domain, &self, format!("{self:?}"))
	}
}

impl SettingsCommand {
	fn widget_id(self, domain: &std::mem::Discriminant<Command>) -> WidgetId {
		let key = match self {
			Self::FontFamily(role, selection) => {
				format!("{role:?}:{selection:?}")
			}
			_ => format!("{self:?}"),
		};
		crate::app::widgets::identity(domain, &self, key)
	}
}

impl ExportCommand {
	fn widget_id(self, domain: &std::mem::Discriminant<Command>) -> WidgetId {
		crate::app::widgets::identity(domain, &self, format!("{self:?}"))
	}
}

impl ChromeCommand {
	fn widget_id(self, domain: &std::mem::Discriminant<Command>) -> WidgetId {
		let key = match self {
			Self::FocusInput(field) => format!("{field:?}"),
			Self::ToggleDropdown(id, _) => format!("{id:?}"),
			_ => format!("{self:?}"),
		};
		crate::app::widgets::identity(domain, &self, key)
	}
}

impl SearchCommand {
	fn widget_id(self, domain: &std::mem::Discriminant<Command>) -> WidgetId {
		crate::app::widgets::identity(domain, &self, format!("{self:?}"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The strings below are the pre-split `Debug` spellings the Android
	/// harness matches, so a new domain must not change them.
	#[test]
	fn debug_names_keep_the_flat_pre_split_spelling() {
		assert_eq!(
			Command::Document(DocumentCommand::Outline).debug_name(),
			"Outline"
		);
		assert_eq!(
			Command::Document(DocumentCommand::OpenUrl).debug_name(),
			"OpenUrl"
		);
		assert_eq!(
			Command::Chrome(ChromeCommand::Settings).debug_name(),
			"Settings"
		);
		assert_eq!(
			Command::Chrome(ChromeCommand::FocusInput(TextField::Url))
				.debug_name(),
			"FocusInput(Url)"
		);
		assert_eq!(
			Command::Settings(SettingsCommand::TabStyle(
				crate::settings::TabStyle::Underline
			))
			.debug_name(),
			"TabStyle(Underline)"
		);
		assert_eq!(
			Command::Search(SearchCommand::SearchNext).debug_name(),
			"SearchNext"
		);
		assert_eq!(
			Command::Export(ExportCommand::Export).debug_name(),
			"Export"
		);
		assert_eq!(
			Command::Fonts(font_panel::Command::Choosers).debug_name(),
			"Fonts(Choosers)"
		);
		assert_eq!(
			Command::Fonts(font_panel::Command::OpenFolder).debug_name(),
			"Fonts(OpenFolder)"
		);
		let id = DropdownId::TabStyle;
		assert_eq!(
			Command::Chrome(ChromeCommand::ToggleDropdown(id, 2)).debug_name(),
			format!("ToggleDropdown({id:?}, 2)")
		);
		let font = DropdownId::Font(crate::settings::FontRole::Serif);
		assert_eq!(
			Command::Chrome(ChromeCommand::ToggleDropdown(font, 2))
				.debug_name(),
			"ToggleDropdown(Font(Serif), 2)"
		);
	}
}
