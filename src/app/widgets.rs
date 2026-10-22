//! Adapt the existing self-drawn controls to the common interaction protocol.
use super::{App, Button};
use crate::state::Command;
use markview_ui_core::{Input, Layout, Widget, WidgetId};
use std::hash::{Hash, Hasher};

/// One geometry product for paint, pointer input and keyboard traversal.
pub(super) struct UiLayout {
	pub buttons: Vec<Button>,
	pub focus: Vec<Button>,
	pub form: Option<super::chrome::components::Form>,
	pub hit: Layout<Command>,
}
/// Default control identities. Pages override row keys with their model IDs.
pub(crate) fn id(command: Command) -> WidgetId {
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	std::mem::discriminant(&command).hash(&mut hash);
	let scope = hash.finish();
	let mut hash = std::collections::hash_map::DefaultHasher::new();
	let key = match command {
		Command::ToggleDropdown(id, _) => format!("{id:?}"),
		Command::FocusInput(field) => format!("{field:?}"),
		Command::FontFamily(role, selection) => {
			format!("{role:?}:{selection:?}")
		}
		Command::Fonts(command) => format!("{command:?}"),
		_ => format!("{command:?}"),
	};
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
			focusable: self.action != Command::CopyPath
				&& !(self.action == Command::Tabs && self.label.is_empty()),
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
