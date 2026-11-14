//! Routes surface actions to native services without putting I/O in the view.
use super::*;
use markview_interaction::{Point, ViewAction, ViewInput};
use winit::keyboard::{Key, NamedKey};

impl<P: super::super::SendEvent> App<P> {
	pub(in crate::app) fn preview_captured(&self) -> bool {
		self.preview
			.floating()
			.is_some_and(|floating| floating.state.captured())
	}
	pub(in crate::app) fn preview_input(&mut self, input: ViewInput) -> bool {
		if !self.preview_allowed() {
			return false;
		}
		let point = match input {
			ViewInput::Move(p)
			| ViewInput::Release(p)
			| ViewInput::Press { point: p, .. }
			| ViewInput::Wheel { point: p, .. } => Some(p),
			_ => None,
		};
		let Some(floating) = self.preview.floating() else {
			return false;
		};
		if point.is_some_and(|point| !floating.rect.contains(point.x, point.y))
			&& !floating.state.captured()
		{
			return false;
		}
		if matches!(input, ViewInput::Press { .. }) {
			self.cancel_gestures();
			self.readers.session.cancel_scroll_animation();
			self.interaction.pointer.reset_wheel();
			self.abandon_dm();
		}
		let floating = self.preview.floating_mut().unwrap();
		let update = floating.input(input);
		let hover = floating.state.hover.cursor;
		if matches!(input, ViewInput::Press { .. }) {
			floating.state.focused = true;
			self.interaction.widgets.focus = None;
		}
		if let Some(action) = update.action {
			self.preview_action(action);
		}
		if let Some(window) = &self.window {
			use winit::cursor::CursorIcon;
			window.set_cursor(
				match hover {
					markview_interaction::Cursor::Pointer => {
						CursorIcon::Pointer
					}
					markview_interaction::Cursor::Text => CursorIcon::Text,
					_ => CursorIcon::Default,
				}
				.into(),
			);
		}
		self.redraw();
		true
	}

	pub(in crate::app) fn preview_horizontal_by(&mut self, delta: f32) -> bool {
		let point = self.interaction.pointer.cursor();
		let Some(floating) = self.preview.floating_mut() else {
			return false;
		};
		if !floating.rect.contains(point.0, point.1) {
			return false;
		}
		floating.horizontal_by(Point::new(point.0, point.1), delta);
		self.redraw();
		true
	}

	fn preview_action(&mut self, action: ViewAction) {
		match action {
			ViewAction::Link(target) => {
				self.dismiss_preview();
				self.open_link_target(&target, false);
			}
			ViewAction::Image { source, rect } => {
				self.dismiss_preview();
				self.open_viewer_image(source, rect);
			}
			ViewAction::ScrollTo(_) | ViewAction::HorizontalTo { .. } => {
				unreachable!(
					"the view applies local scrolling before producing host actions"
				)
			}
		}
	}

	pub(in crate::app) fn advance_preview_selection(
		&mut self,
		seconds: f32,
	) -> bool {
		let Some(floating) = self.preview.floating_mut() else {
			return false;
		};
		floating.advance_selection(seconds)
	}

	fn preview_command(&mut self, key: &Key) -> bool {
		let Key::Character(character) = key else {
			return false;
		};
		match character.to_lowercase().as_str() {
			"c" => {
				self.copy_selection();
				true
			}
			"a" => {
				self.preview_input(ViewInput::SelectAll);
				true
			}
			_ => false,
		}
	}

	pub(in crate::app) fn preview_key(&mut self, key: &Key) -> bool {
		if !self.preview_allowed() {
			return false;
		}
		if matches!(key, Key::Named(NamedKey::Escape))
			&& self.preview.open().is_some()
		{
			self.dismiss_preview();
			self.redraw();
			return true;
		}
		if !self
			.preview
			.floating()
			.is_some_and(|floating| floating.state.focused)
		{
			return false;
		}
		let modifiers = self.interaction.pointer.modifiers();
		if modifiers.control_key() || modifiers.meta_key() {
			return self.preview_command(key);
		}
		let Some(floating) = self.preview.floating_mut() else {
			return false;
		};
		let body = floating.body();
		let delta = match key {
			Key::Named(NamedKey::ArrowUp) => -40.0,
			Key::Named(NamedKey::ArrowDown) => 40.0,
			Key::Named(NamedKey::PageUp) => -body.h,
			Key::Named(NamedKey::PageDown) => body.h,
			Key::Character(text) if text == " " => body.h,
			Key::Named(NamedKey::Home) => -f32::MAX,
			Key::Named(NamedKey::End) => f32::MAX,
			_ => return false,
		};
		floating.input(ViewInput::Wheel {
			point: Point::new(body.x + 1.0, body.y + 1.0),
			delta,
		});
		self.redraw();
		true
	}
}
