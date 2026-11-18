//! Routes surface actions to native services without putting I/O in the view.
use super::*;
use markview_interaction::{Point, ViewAction, ViewInput};
use winit::keyboard::{Key, NamedKey};

impl<P: super::super::SendEvent> App<P> {
	pub(in crate::app) fn input_document(
		&mut self,
	) -> Option<&mut crate::app::document_view::FloatingDocument> {
		self.preview_document_for_input(false)
	}
	fn preview_document_for_input(
		&mut self,
		keyboard: bool,
	) -> Option<&mut crate::app::document_view::FloatingDocument> {
		if self.interaction.overlays.quick_open.open() {
			if !self.frame.focused
				|| self.interaction.overlays.modal_open()
				|| self.interaction.overlays.viewer_open()
				|| self.interaction.widgets.captured.is_some()
				|| self.interaction.pointer.scrollbar().is_some()
			{
				return None;
			}
			let preview = &mut self.interaction.overlays.quick_open.preview;
			return if keyboard && preview.reflow.is_some() {
				preview.reflow.as_mut()
			} else {
				preview.document.as_mut()
			};
		}
		if self.preview_allowed() {
			self.preview.floating_mut()
		} else {
			None
		}
	}
	pub(in crate::app) fn refresh_preview_hover(
		&mut self,
	) -> Option<winit::cursor::CursorIcon> {
		let (x, y) = self.interaction.pointer.cursor();
		let floating = self.input_document()?;
		if !floating.rect.contains(x, y) && !floating.state.captured() {
			if floating.state.hover != Default::default() {
				floating.state.hover = Default::default();
				self.redraw();
			}
			return None;
		}
		let hover = floating
			.view()
			.hover(Point::new(x, y), floating.state.captured());
		let cursor = hover.cursor;
		let hover_changed = hover != floating.state.hover;
		floating.state.hover = hover;
		let changed = hover_changed
			|| self.interaction.pointer.hover().is_some()
			|| self.interaction.pointer.hover_image().is_some()
			|| self.interaction.pointer.hover_control().is_some()
			|| self.interaction.widgets.hovered.is_some();
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
		let picker = &self.interaction.overlays.quick_open;
		let doc = if picker.open() {
			picker.preview.document.as_ref()
		} else {
			self.preview.floating()
		};
		doc.is_some_and(|doc| doc.state.captured())
	}
	pub(in crate::app) fn preview_input(&mut self, input: ViewInput) -> bool {
		let picker = self.interaction.overlays.quick_open.open();
		let point = Self::preview_point(&input);
		let Some(floating) = self.input_document() else {
			return false;
		};
		if point.is_some_and(|point| !floating.rect.contains(point.x, point.y))
			&& !floating.state.captured()
		{
			return false;
		}
		let pressed = matches!(input, ViewInput::Press { .. });
		if pressed {
			self.cancel_preview_press(picker);
		}
		if self.picker_input(&input, pressed) {
			return true;
		}
		if !picker && self.defer_file_preview_input(&input) {
			return true;
		}
		let floating = self.input_document().unwrap();
		let update = floating.input(input);
		if pressed {
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

	fn preview_point(input: &ViewInput) -> Option<Point> {
		match input {
			ViewInput::Move(p)
			| ViewInput::Release(p)
			| ViewInput::Press { point: p, .. }
			| ViewInput::Scroll { point: p, .. } => Some(*p),
			_ => None,
		}
	}

	/// Abandons every gesture and scroll a press on the preview interrupts.
	fn cancel_preview_press(&mut self, picker: bool) {
		if let Some(floating) = self.input_document() {
			floating.scroll.cancel();
		}
		self.cancel_gestures();
		self.readers.session.cancel_scroll_animation();
		self.interaction.pointer.reset_wheel();
		self.abandon_dm();
		if picker {
			let preview = &mut self.interaction.overlays.quick_open.preview;
			preview.select_all_pending = false;
			self.clear_input_focus();
		}
	}

	/// Keeps the picker's preview request and scroll follow in step with the
	/// input. Returns whether the picker consumed the input.
	fn picker_input(&mut self, input: &ViewInput, pressed: bool) -> bool {
		if !self.interaction.overlays.quick_open.open() {
			return false;
		}
		if matches!(input, ViewInput::Press { .. } | ViewInput::Release(_)) {
			self.interaction.overlays.quick_open.preview.check_file();
			self.refresh_quick_open_preview();
			if self.input_document().is_none() {
				return true;
			}
		}
		if pressed || matches!(input, ViewInput::Scroll { .. }) {
			self.interaction.overlays.quick_open.preview.follow_end = false;
		}
		false
	}

	fn defer_file_preview_input(&mut self, input: &ViewInput) -> bool {
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

	fn preview_action(&mut self, action: ViewAction) {
		let file = self.interaction.overlays.quick_open.open()
			|| self.preview.open().is_some_and(|open| open.path.is_some());
		match action {
			ViewAction::Link(target) => {
				if file {
					self.activate_file_preview(target);
				} else {
					self.dismiss_preview();
					self.open_link_target(&target, false);
				}
			}
			ViewAction::Image { source, rect } => {
				if file {
					self.activate_file_preview_image(&source, rect);
				} else {
					self.dismiss_preview();
					self.open_viewer_image(source, rect);
				}
			}
			ViewAction::Scroll(_)
			| ViewAction::JumpTo(_)
			| ViewAction::HorizontalTo { .. } => {
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
		let Some(floating) = self.input_document() else {
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
				let preview = &mut self.interaction.overlays.quick_open.preview;
				if preview.select_all_pending {
					preview.defer_copy(self.clipboard.stamp());
				} else {
					self.copy_selection();
				}
				true
			}
			"a" => {
				self.interaction.overlays.quick_open.preview.select_all();
				if let Some(doc) = self.preview_document_for_input(true) {
					doc.input(ViewInput::SelectAll);
				}
				self.redraw();
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
		self.focused_preview_key(key)
	}

	pub(in crate::app) fn focused_preview_key(&mut self, key: &Key) -> bool {
		if !self
			.preview_document_for_input(true)
			.is_some_and(|doc| doc.focused())
		{
			return false;
		}
		let modifiers = self.interaction.pointer.modifiers();
		if modifiers.control_key() || modifiers.meta_key() {
			return self.preview_command(key);
		}
		let clock = self.animations.scrolls.clone();
		let Some(floating) = self.preview_document_for_input(true) else {
			return false;
		};
		let body = floating.body();
		let now = Instant::now();
		let request = match key {
			Key::Named(NamedKey::ArrowUp) => clock.step(-40.0, now),
			Key::Named(NamedKey::ArrowDown) => clock.step(40.0, now),
			Key::Named(NamedKey::PageUp) => clock.step(-body.h, now),
			Key::Named(NamedKey::PageDown) => clock.step(body.h, now),
			Key::Character(text) if text == " " => clock.step(body.h, now),
			Key::Named(NamedKey::Home) => clock.to(0.0, now),
			Key::Named(NamedKey::End) => clock.to(f32::MAX, now),
			_ => return false,
		};
		floating.input(ViewInput::Scroll {
			point: Point::new(body.x + 1.0, body.y + 1.0),
			request,
		});
		let preview = &mut self.interaction.overlays.quick_open.preview;
		preview.follow_end =
			preview.pending && matches!(key, Key::Named(NamedKey::End));
		self.redraw();
		true
	}
}
