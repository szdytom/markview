//! Window integration for reusable single-line input fields.
use super::{App, SendEvent};
use crate::{
	layout::{Draw, Rect, TextShaper},
	state::{Command, TextField},
};
use markview_core::text_input::{EditKind, Motion, TextInput};
use std::time::{Duration, Instant};
use winit::{
	dpi::{LogicalPosition, LogicalSize},
	event::{ElementState, Ime, MouseButton, WindowEvent},
	keyboard::{Key, ModifiersState, NamedKey},
};

const BLINK: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq)]
struct ImeArea {
	position: LogicalPosition<u32>,
	size: LogicalSize<u32>,
	scale: f32,
}

pub(super) struct InputState {
	focused: Option<TextField>,
	window_active: bool,
	window_visible: bool,
	ime_enabled: bool,
	last_ime_area: Option<ImeArea>,
	dragging: bool,
	caret: bool,
	pub(super) deadline: Option<Instant>,
}
impl Default for InputState {
	fn default() -> Self {
		Self {
			focused: None,
			window_active: true,
			window_visible: true,
			ime_enabled: false,
			last_ime_area: None,
			dragging: false,
			caret: true,
			deadline: None,
		}
	}
}
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
	Consumed,
	Submit,
	Cancel,
	Traverse,
	Application,
}

trait InputClipboard {
	fn read(&mut self) -> anyhow::Result<String>;
	fn write(&mut self, text: String) -> anyhow::Result<()>;
}
impl InputClipboard for crate::platform::Clipboard {
	fn read(&mut self) -> anyhow::Result<String> {
		self.read()
	}
	fn write(&mut self, text: String) -> anyhow::Result<()> {
		self.write(text)
	}
}

/// Platform shortcut policy is separate from the editor and its business owner.
fn key(
	input: &mut TextInput,
	ui: &mut TextShaper,
	clipboard: &mut impl InputClipboard,
	key: &Key,
	text: Option<&str>,
	mods: ModifiersState,
) -> Outcome {
	let mac = cfg!(target_os = "macos");
	let primary = if mac {
		mods.super_key()
	} else {
		mods.control_key() && !mods.alt_key()
	};
	let word = if mac {
		mods.alt_key()
	} else {
		mods.control_key() && !mods.alt_key()
	};
	let shift = mods.shift_key();
	if input.is_composing() {
		if *key == Key::Named(NamedKey::Escape) {
			input.cancel_compose(ui);
		}
		return Outcome::Consumed;
	}
	if primary && let Key::Character(c) = key {
		match c.to_lowercase().as_str() {
			"a" => input.select_all(ui),
			"c" | "x" => {
				if let Some(text) = input.selected_text() {
					match clipboard.write(text.to_owned()) {
						Ok(()) if c.eq_ignore_ascii_case("x") => {
							input.break_group();
							input.delete(ui, false, false);
							input.break_group();
						}
						Ok(()) => {}
						Err(error) => {
							log::warn!("Cannot copy input text: {error}")
						}
					}
				}
			}
			"v" => match clipboard.read() {
				Ok(text) => input.insert(ui, &text, EditKind::Separate),
				Err(error) => log::warn!("Cannot paste input text: {error}"),
			},
			"z" => input.undo(ui, shift),
			"y" if !mac => input.undo(ui, true),
			"q" | "o" | "e" | "," | "t" => return Outcome::Application,
			_ => {}
		}
		return Outcome::Consumed;
	}
	let motion = match key {
		Key::Named(NamedKey::ArrowLeft) => Some(if mac && primary {
			Motion::Start
		} else if word {
			Motion::WordLeft
		} else {
			Motion::Left
		}),
		Key::Named(NamedKey::ArrowRight) => Some(if mac && primary {
			Motion::End
		} else if word {
			Motion::WordRight
		} else {
			Motion::Right
		}),
		Key::Named(NamedKey::Home) => Some(Motion::Start),
		Key::Named(NamedKey::End) => Some(Motion::End),
		_ => None,
	};
	if let Some(motion) = motion {
		input.move_cursor(ui, motion, shift);
		return Outcome::Consumed;
	}
	match key {
		Key::Named(NamedKey::Tab) => return Outcome::Traverse,
		Key::Named(NamedKey::Enter) => return Outcome::Submit,
		Key::Named(NamedKey::Escape) => return Outcome::Cancel,
		Key::Named(NamedKey::Backspace | NamedKey::Delete) => {
			let backwards = *key == Key::Named(NamedKey::Backspace);
			if mac && primary {
				input.move_cursor(
					ui,
					if backwards {
						Motion::Start
					} else {
						Motion::End
					},
					true,
				);
			}
			input.delete(ui, backwards, word);
		}
		_ if !primary && !mods.super_key() => {
			if let Some(text) = text {
				input.insert(ui, text, EditKind::Typing);
			}
		}
		_ => {}
	}
	Outcome::Consumed
}

impl<P: SendEvent> App<P> {
	fn input(&mut self, id: TextField) -> (&mut TextInput, &mut TextShaper) {
		match id {
			TextField::Search => {
				(&mut self.readers.session.search.input, &mut self.ui)
			}
			TextField::ExportTitle => {
				(&mut self.readers.session.export_title, &mut self.ui)
			}
		}
	}
	pub(super) fn input_geometry(
		&mut self,
		id: TextField,
	) -> Option<(Rect, Rect)> {
		if id == TextField::Search {
			return self.readers.session.search.open.then(|| {
				let (w, h, _) = self.dimensions();
				(
					self.search_input_rect(),
					Rect {
						x: 0.0,
						y: h - super::search::HEIGHT,
						w,
						h: super::search::HEIGHT,
					},
				)
			});
		}
		let form = self.panel_form()?;
		let rect = form
			.buttons
			.iter()
			.find(|b| b.action == Command::FocusInput(id))?
			.rect;
		Some((rect, form.viewport))
	}
	pub(super) fn input_at_cursor(&mut self) -> Option<TextField> {
		if self.interaction.modal.is_some()
			|| self.interaction.dropdown.is_some()
		{
			return None;
		}
		let (x, y) = self.interaction.cursor;
		if self.readers.session.search.open
			&& self.search_input_rect().contains(x, y)
		{
			return Some(TextField::Search);
		}
		let form = self.panel_form()?;
		let (x, y) = self.interaction.cursor;
		if !form.viewport.contains(x, y) {
			return None;
		}
		form.buttons.iter().find_map(|b| match b.action {
			Command::FocusInput(id) if b.rect.contains(x, y) => Some(id),
			_ => None,
		})
	}
	pub(super) fn blur_input(&mut self) {
		if let Some(id) = self.text_input.focused.take() {
			let (input, ui) = self.input(id);
			input.cancel_compose(ui);
			input.break_group();
			if let Some(window) = &self.window {
				window.set_ime_allowed(false);
			}
		}
		self.text_input.ime_enabled = false;
		self.text_input.last_ime_area = None;
		self.text_input.dragging = false;
		self.text_input.deadline = None;
	}
	pub(super) fn clear_input_focus(&mut self) {
		self.blur_input();
		if matches!(self.interaction.focus, Some(Command::FocusInput(_))) {
			self.interaction.focus = None;
		}
	}
	fn reset_input_blink(&mut self) {
		self.text_input.caret = true;
		self.text_input.deadline = Some(Instant::now() + BLINK);
		self.redraw();
	}
	pub(super) fn sync_input(&mut self) {
		let focus = self.interaction.focus;
		let desired = if self.text_input.window_active
			&& self.text_input.window_visible
			&& !self.dialog_open
			&& self.interaction.modal.is_none()
			&& self.interaction.dropdown.is_none()
		{
			match focus {
				Some(Command::FocusInput(id))
					if self.input_geometry(id).is_some() =>
				{
					Some(id)
				}
				_ => None,
			}
		} else {
			None
		};
		if desired != self.text_input.focused {
			self.blur_input();
			self.text_input.focused = desired;
			if desired.is_some() {
				if let Some(window) = &self.window {
					window.set_ime_allowed(true);
				}
				self.reset_input_blink();
			}
		}
		if let Some(id) = desired
			&& let Some((rect, viewport)) = self.input_geometry(id)
		{
			if rect.intersect(viewport).is_none() {
				self.text_input.deadline = None;
			} else if self.text_input.deadline.is_none() {
				self.reset_input_blink();
			}
		}
	}
	// Wayland commits every cursor-area request and can answer with IME events.
	// Publish only changed geometry, after the entire input batch is processed.
	fn take_ime_area(&mut self) -> Option<ImeArea> {
		if !self.text_input.ime_enabled {
			return None;
		}
		let id = self.text_input.focused?;
		let (rect, viewport) = self.input_geometry(id)?;
		let (input, ui) = self.input(id);
		let area = input.ime_area(ui, rect);
		let area = ImeArea {
			position: LogicalPosition::new(
				area.x.max(0.0).round() as u32,
				area.y
					.clamp(viewport.y, viewport.y + viewport.h)
					.max(0.0)
					.round() as u32,
			),
			size: LogicalSize::new(
				area.w.max(1.0).round() as u32,
				area.h.max(1.0).round() as u32,
			),
			scale: self.dimensions().2,
		};
		if self.text_input.last_ime_area == Some(area) {
			return None;
		}
		self.text_input.last_ime_area = Some(area);
		Some(area)
	}
	pub(super) fn flush_ime_area(&mut self) {
		self.sync_input();
		if let Some(area) = self.take_ime_area()
			&& let Some(window) = &self.window
		{
			window.set_ime_cursor_area(area.position, area.size);
		}
	}

	pub(super) fn input_tick(&mut self, now: Instant) {
		self.sync_input();
		if self.text_input.deadline.is_none_or(|at| at > now) {
			return;
		}
		if self.text_input.dragging {
			self.drag_input();
		} else {
			self.text_input.caret = !self.text_input.caret;
		}
		self.text_input.deadline = Some(
			now + if self.text_input.dragging {
				Duration::from_millis(16)
			} else {
				BLINK
			},
		);
		self.redraw();
	}
	fn drag_input(&mut self) {
		if let Some(id) = self.text_input.focused
			&& let Some((rect, _)) = self.input_geometry(id)
		{
			let point = self.interaction.cursor;
			let (input, ui) = self.input(id);
			input.point(ui, rect, (point.0, rect.y + rect.h / 2.0), true, 1);
		}
	}
	pub(super) fn input_event(&mut self, event: &WindowEvent) -> bool {
		self.sync_input();
		match event {
			WindowEvent::Occluded(occluded) => {
				self.text_input.window_visible = !*occluded;
				self.sync_input();
			}
			WindowEvent::Focused(active) => {
				self.text_input.window_active = *active;
				self.sync_input();
			}
			WindowEvent::Ime(ime) => {
				if let Some(id) = self.text_input.focused {
					match ime {
						Ime::Enabled => {
							self.text_input.ime_enabled = true;
							self.text_input.last_ime_area = None;
						}
						Ime::Disabled => {
							self.text_input.ime_enabled = false;
							self.text_input.last_ime_area = None;
							let (input, ui) = self.input(id);
							input.cancel_compose(ui);
						}
						Ime::Preedit(text, cursor) => {
							let (input, ui) = self.input(id);
							input.preedit(ui, text, *cursor);
						}
						Ime::Commit(text) => {
							let (input, ui) = self.input(id);
							input.commit(ui, text);
							if id == TextField::Search {
								self.search_changed();
							}
						}
					}
					self.reset_input_blink();
					self.sync_input();
				}
				return true;
			}
			WindowEvent::KeyboardInput { event, .. }
				if self.text_input.focused.is_some() =>
			{
				if event.state != ElementState::Pressed {
					return true;
				}
				let outcome =
					self.input_key(&event.logical_key, event.text.as_deref());
				return !matches!(
					outcome,
					Outcome::Traverse | Outcome::Application
				);
			}
			WindowEvent::CursorMoved { position, .. }
				if self.text_input.dragging =>
			{
				let scale = self.dimensions().2 as f64;
				self.interaction.cursor =
					((position.x / scale) as f32, (position.y / scale) as f32);
				self.drag_input();
				self.reset_input_blink();
				self.text_input.deadline =
					Some(Instant::now() + Duration::from_millis(16));
				return true;
			}
			WindowEvent::MouseInput {
				state: ElementState::Pressed,
				button: MouseButton::Left,
				..
			} => {
				if let Some(id) = self.input_at_cursor() {
					self.blur_input();
					self.cancel_gestures();
					self.interaction.focus = Some(Command::FocusInput(id));
					self.interaction.focus_visible = false;
					self.interaction.pressed = None;
					self.sync_input();
					let (rect, _) = self
						.input_geometry(id)
						.expect("hit input has geometry");
					let point = self.interaction.cursor;
					let clicks = self.interaction.click_count(Instant::now());
					let extend = self.interaction.modifiers.shift_key();
					let (input, ui) = self.input(id);
					input.point(ui, rect, point, extend, clicks);
					self.text_input.dragging = true;
					self.reset_input_blink();
					return true;
				}
				self.blur_input();
				if matches!(
					self.interaction.focus,
					Some(Command::FocusInput(_))
				) {
					self.interaction.focus = None;
				}
			}
			WindowEvent::MouseInput {
				state: ElementState::Released,
				button: MouseButton::Left,
				..
			} if self.text_input.dragging => {
				self.text_input.dragging = false;
				self.reset_input_blink();
				return true;
			}
			WindowEvent::CursorLeft { .. } => {
				self.text_input.dragging = false;
			}
			_ => {}
		}
		false
	}
	fn input_key(&mut self, logical: &Key, text: Option<&str>) -> Outcome {
		let id = self.text_input.focused.expect("focused input owns key");
		let mods = self.interaction.modifiers;
		let before = (id == TextField::Search)
			.then(|| self.readers.session.search.input.text().to_owned());
		let outcome = match id {
			TextField::Search => key(
				&mut self.readers.session.search.input,
				&mut self.ui,
				&mut self.clipboard,
				logical,
				text,
				mods,
			),
			TextField::ExportTitle => key(
				&mut self.readers.session.export_title,
				&mut self.ui,
				&mut self.clipboard,
				logical,
				text,
				mods,
			),
		};
		if id == TextField::Search {
			if before.as_deref()
				!= Some(self.readers.session.search.input.text())
			{
				self.search_changed();
			}
			match outcome {
				Outcome::Submit => {
					self.navigate_search(mods.shift_key());
					return Outcome::Consumed;
				}
				Outcome::Cancel => {
					self.close_search();
					return Outcome::Consumed;
				}
				_ => {}
			}
		}
		match outcome {
			Outcome::Submit | Outcome::Cancel | Outcome::Application => {
				self.blur_input();
				self.interaction.focus = None;
				self.redraw();
			}
			Outcome::Traverse => {
				self.blur_input();
			}
			Outcome::Consumed => self.reset_input_blink(),
		}
		outcome
	}
	pub(super) fn draw_search_input(&mut self) -> Vec<Draw> {
		self.sync_input();
		let rect = self.search_input_rect();
		let placeholder = self.preferences.values.lang().search_placeholder();
		let focused = self.text_input.focused == Some(TextField::Search);
		self.readers.session.search.input.draw(
			&mut self.ui,
			rect,
			focused,
			self.text_input.caret,
			placeholder,
		)
	}

	pub(super) fn draw_inputs(&mut self) -> Vec<Draw> {
		self.sync_input();
		let Some(form) = self.panel_form() else {
			return vec![];
		};
		let mut out = Vec::new();
		let placeholder =
			self.preferences.values.lang().export_title_placeholder();
		for button in form.buttons {
			if let Command::FocusInput(id) = button.action
				&& button.rect.intersect(form.viewport).is_some()
			{
				let focused = self.text_input.focused == Some(id);
				let caret = self.text_input.caret;
				let (input, ui) = self.input(id);
				out.extend(input.draw(
					ui,
					button.rect,
					focused,
					caret,
					placeholder,
				));
			}
		}
		vec![Draw::Clipped {
			rect: form.viewport,
			draws: out,
		}]
	}
}

#[cfg(test)]
mod tests;
