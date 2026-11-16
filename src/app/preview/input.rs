//! Routes surface actions to native services without putting I/O in the view.
use super::*;
use markview_interaction::{Point, ViewAction, ViewInput};
use winit::keyboard::{Key, NamedKey};

impl<P: super::super::SendEvent> App<P> {
	pub(in crate::app) fn refresh_preview_hover(
		&mut self,
	) -> Option<winit::cursor::CursorIcon> {
		if !self.preview_allowed() {
			return None;
		}
		let (x, y) = self.interaction.pointer.cursor();
		let floating = self.preview.floating_mut()?;
		if !floating.rect.contains(x, y) && !floating.state.captured() {
			return None;
		}
		let hover = floating
			.view()
			.hover(Point::new(x, y), floating.state.captured());
		let changed = hover != floating.state.hover
			|| self.interaction.pointer.hover().is_some()
			|| self.interaction.pointer.hover_image().is_some()
			|| self.interaction.pointer.hover_control().is_some()
			|| self.interaction.widgets.hovered.is_some();
		let cursor = hover.cursor;
		floating.state.hover = hover;
		self.interaction.pointer.clear_hover();
		self.interaction.widgets.hovered = None;
		if changed {
			self.redraw();
		}
		use winit::cursor::CursorIcon;
		Some(match cursor {
			markview_interaction::Cursor::Default => CursorIcon::Default,
			markview_interaction::Cursor::Text => CursorIcon::Text,
			markview_interaction::Cursor::Pointer => CursorIcon::Pointer,
		})
	}

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
		if self.defer_file_preview_input(input) {
			return true;
		}

		if matches!(input, ViewInput::Press { .. }) {
			self.cancel_gestures();
			self.readers.session.cancel_scroll_animation();
			self.interaction.pointer.reset_wheel();
			self.abandon_dm();
		}
		let floating = self.preview.floating_mut().unwrap();
		let update = floating.input(input);
		if matches!(input, ViewInput::Press { .. }) {
			floating.state.focused = true;
			self.interaction.widgets.focus = None;
		}
		if let Some(action) = update.action {
			self.preview_action(action);
		}
		if let Some(cursor) = self.refresh_preview_hover()
			&& let Some(window) = &self.window
		{
			window.set_cursor(cursor.into());
		}
		self.redraw();
		true
	}

	fn defer_file_preview_input(&mut self, input: ViewInput) -> bool {
		if !matches!(input, ViewInput::Press { .. } | ViewInput::Release(_)) {
			return false;
		}
		if let Some(observed) = &self.preview.observed {
			observed.invalidate_if_outdated();
		}
		self.refresh_preview_file()
			|| (self.preview.open().is_some_and(|open| open.path.is_some())
				&& self
					.preview
					.pending
					.as_ref()
					.is_some_and(|pending| !pending.presented))
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
				if self.preview.open().is_some_and(|open| open.path.is_some()) {
					self.activate_file_preview(target);
				} else {
					self.dismiss_preview();
					self.open_link_target(&target, false);
				}
			}
			ViewAction::Image { source, rect } => {
				if self.preview.open().is_some_and(|open| open.path.is_some()) {
					self.activate_file_preview_image(&source, rect);
				} else {
					self.dismiss_preview();
					self.open_viewer_image(source, rect);
				}
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
