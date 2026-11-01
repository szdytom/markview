//! Window integration for reusable single-line input fields.
use super::{App, SendEvent};
use crate::state::{ChromeCommand, DocumentCommand};
use crate::{
	layout::{Draw, Rect, TextShaper},
	state::{Command, TextField},
};
use markview_layout::text_input::{EditKind, Motion, TextInput};
use std::time::{Duration, Instant};
use winit::{
	dpi::{LogicalPosition, LogicalSize},
	event::{ElementState, Ime, MouseButton, WindowEvent},
	keyboard::{Key, ModifiersState, NamedKey},
};

use crate::state::Selection;
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

fn start_input_scroll(
	g: &super::chrome::empty::Geometry,
	input: Rect,
	id: TextField,
) -> Option<f32> {
	let delta =
		if input.y < g.clip.y || (id == TextField::Files && g.clip.h < 400.0) {
			input.y - g.clip.y - 16.0
		} else {
			(input.y + input.h - g.clip.y - g.clip.h).max(0.0)
		};
	let scroll = (g.scroll + delta).clamp(0.0, g.max_scroll);
	(scroll != g.scroll).then_some(scroll)
}

trait InputClipboard {
	fn read(&mut self) -> anyhow::Result<String>;
	fn write(&mut self, text: String) -> anyhow::Result<()>;
}
impl InputClipboard for markview_platform::Clipboard {
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
	let mac = markview_platform::command_key();
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
		return primary_key(input, ui, clipboard, c, mac, shift);
	}
	let motion = input_motion(key, mac, primary, word);
	if let Some(motion) = motion {
		input.move_cursor(ui, motion, shift);
		return Outcome::Consumed;
	}
	edit_key(
		input,
		ui,
		key,
		text,
		mods,
		InputKeyModifiers { mac, primary, word },
	)
}

fn primary_key(
	input: &mut TextInput,
	ui: &mut TextShaper,
	clipboard: &mut impl InputClipboard,
	c: &str,
	mac: bool,
	shift: bool,
) -> Outcome {
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
		"q" | "o" | "e" | "," | "t" | "n" => return Outcome::Application,
		_ => {}
	}
	Outcome::Consumed
}

fn input_motion(
	key: &Key,
	mac: bool,
	primary: bool,
	word: bool,
) -> Option<Motion> {
	match key {
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
	}
}

struct InputKeyModifiers {
	mac: bool,
	primary: bool,
	word: bool,
}

fn edit_key(
	input: &mut TextInput,
	ui: &mut TextShaper,
	key: &Key,
	text: Option<&str>,
	mods: ModifiersState,
	flags: InputKeyModifiers,
) -> Outcome {
	let InputKeyModifiers { mac, primary, word } = flags;
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
			TextField::Url => (&mut self.url_input, &mut self.ui),
			TextField::Files => (&mut self.folder.start.input, &mut self.ui),
			TextField::Search => {
				(&mut self.readers.session.search.input, &mut self.ui)
			}
			TextField::ExportTitle => {
				(&mut self.readers.session.export_title, &mut self.ui)
			}
		}
	}
	fn start_input_geometry(&mut self, id: TextField) -> Option<(Rect, Rect)> {
		if self.readers.session.path.is_some()
			|| !self.readers.session.snapshot.blocks.is_empty()
		{
			return None;
		}
		if id == TextField::Files && self.folder.root().is_none() {
			return None;
		}
		let mut g = self.chrome().start_geometry();
		let rect = |g: &super::chrome::empty::Geometry| {
			if id == TextField::Files {
				g.search
			} else {
				g.url
			}
		};
		let input = rect(&g);
		if self.focused_input() == Some(id)
			&& let Some(scroll) = start_input_scroll(&g, input, id)
		{
			self.folder.start.scroll = scroll;
			g = self.chrome().start_geometry();
			self.redraw();
		}
		Some((rect(&g), g.clip))
	}

	pub(super) fn input_geometry(
		&mut self,
		id: TextField,
	) -> Option<(Rect, Rect)> {
		if matches!(id, TextField::Url | TextField::Files) {
			if self.interaction.panel_open()
				|| self.interaction.overlays.outline.open()
				|| self.interaction.overlays.modal_open()
				|| self.interaction.overlays.viewer_open()
			{
				return None;
			}
			return self.start_input_geometry(id);
		}

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
			.find(|b| {
				b.action == Command::Chrome(ChromeCommand::FocusInput(id))
			})?
			.rect;
		Some((rect, form.viewport))
	}
	pub(super) fn input_at_cursor(&mut self) -> Option<TextField> {
		if self.interaction.overlays.modal_open()
			|| self.interaction.overlays.dropdown_open()
			|| self.interaction.overlays.viewer_open()
		{
			return None;
		}
		let (x, y) = self.interaction.pointer.cursor();
		if self.readers.session.search.open
			&& self.search_input_rect().contains(x, y)
		{
			return Some(TextField::Search);
		}
		for field in [TextField::Url, TextField::Files] {
			if let Some((rect, clip)) = self.input_geometry(field)
				&& rect.contains(x, y)
				&& clip.contains(x, y)
			{
				return Some(field);
			}
		}
		let form = self.panel_form()?;
		let (x, y) = self.interaction.pointer.cursor();
		if !form.viewport.contains(x, y) {
			return None;
		}
		form.buttons.iter().find_map(|b| match b.action {
			Command::Chrome(ChromeCommand::FocusInput(id))
				if b.rect.contains(x, y) =>
			{
				Some(id)
			}
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
	pub(super) fn focused_input(&self) -> Option<TextField> {
		[
			TextField::Url,
			TextField::Files,
			TextField::Search,
			TextField::ExportTitle,
		]
		.into_iter()
		.find(|field| {
			self.interaction.widgets.focus
				== Some(
					Command::Chrome(ChromeCommand::FocusInput(*field))
						.widget_id(),
				)
		})
	}
	pub(super) fn clear_input_focus(&mut self) {
		self.blur_input();
		if self.focused_input().is_some() {
			self.interaction.widgets.focus = None;
		}
	}
	fn reset_input_blink(&mut self) {
		self.text_input.caret = true;
		self.text_input.deadline = Some(Instant::now() + BLINK);
		self.redraw();
	}
	pub(super) fn sync_input(&mut self) {
		let desired = self.desired_input_focus();
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
	fn desired_input_focus(&mut self) -> Option<TextField> {
		let focus = self.focused_input();
		if self.text_input.window_active
			&& self.text_input.window_visible
			&& !self.dialog_open
			&& !self.interaction.overlays.modal_open()
			&& !self.interaction.overlays.dropdown_open()
			&& !self.interaction.overlays.viewer_open()
		{
			match focus {
				Some(id) if self.input_geometry(id).is_some() => Some(id),
				_ => None,
			}
		} else {
			None
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
			let point = self.interaction.pointer.cursor();
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
				self.handle_input_ime(ime);
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
				self.interaction.pointer.move_to((
					(position.x / scale) as f32,
					(position.y / scale) as f32,
				));
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
					self.interaction.widgets.focus = Some(
						Command::Chrome(ChromeCommand::FocusInput(id))
							.widget_id(),
					);
					self.interaction.widgets.focus_visible = false;
					self.interaction.widgets.captured = None;
					self.sync_input();
					let (rect, _) = self
						.input_geometry(id)
						.expect("hit input has geometry");
					let point = self.interaction.pointer.cursor();
					let clicks = self.interaction.click_count(Instant::now());
					let extend =
						self.interaction.pointer.modifiers().shift_key();
					let (input, ui) = self.input(id);
					input.point(ui, rect, point, extend, clicks);
					self.text_input.dragging = true;
					self.reset_input_blink();
					return true;
				}
				self.blur_input();
				if self.focused_input().is_some() {
					self.interaction.widgets.focus = None;
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
	fn handle_input_ime(&mut self, ime: &Ime) {
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
					if id == TextField::Files {
						self.start_search_changed();
					}
				}
			}
			self.reset_input_blink();
			self.sync_input();
		}
	}

	fn input_key(&mut self, logical: &Key, text: Option<&str>) -> Outcome {
		let id = self.text_input.focused.expect("focused input owns key");
		let mods = self.interaction.pointer.modifiers();
		let before = self.input(id).0.text().to_owned();
		let outcome = match id {
			TextField::Url => key(
				&mut self.url_input,
				&mut self.ui,
				&mut self.clipboard,
				logical,
				text,
				mods,
			),
			TextField::Files => key(
				&mut self.folder.start.input,
				&mut self.ui,
				&mut self.clipboard,
				logical,
				text,
				mods,
			),
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
		if id == TextField::Url && outcome == Outcome::Submit {
			self.action(Command::Document(DocumentCommand::OpenUrl));
			return Outcome::Consumed;
		}
		if id == TextField::Files && self.files_input_outcome(&before, &outcome)
		{
			return Outcome::Consumed;
		}

		if id == TextField::Search {
			if before != self.readers.session.search.input.text() {
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
				self.interaction.widgets.focus = None;
				self.redraw();
			}
			Outcome::Traverse => {
				self.blur_input();
			}
			Outcome::Consumed => self.reset_input_blink(),
		}
		outcome
	}
	fn files_input_outcome(&mut self, before: &str, outcome: &Outcome) -> bool {
		if before != self.folder.start.input.text() {
			self.start_search_changed();
		}
		match outcome {
			Outcome::Submit => {
				if !self.folder.start.pending
					&& let Some(file) = self.folder.start.matches.first()
				{
					self.action(Command::Chrome(ChromeCommand::StartFile(
						file.id,
					)));
				}
				true
			}
			Outcome::Cancel => {
				self.clear_input_focus();
				true
			}
			_ => false,
		}
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

	#[cfg(test)]
	pub(super) fn draw_inputs(&mut self) -> Vec<Draw> {
		let form = self.panel_form();
		self.draw_inputs_with_form(form.as_ref())
	}
	pub(super) fn draw_inputs_with_form(
		&mut self,
		form: Option<&super::chrome::components::Form>,
	) -> Vec<Draw> {
		self.sync_input();
		let mut start_inputs = Vec::new();
		for field in [TextField::Url, TextField::Files] {
			if let Some((rect, viewport)) = self.input_geometry(field) {
				let lang = self.preferences.values.lang();
				let placeholder = if field == TextField::Files {
					lang.empty_search_placeholder()
				} else {
					lang.empty_url_placeholder()
				};
				let focused = self.text_input.focused == Some(field);
				let caret = self.text_input.caret;
				let (input, ui) = self.input(field);
				start_inputs.push(Draw::Clipped {
					rect: viewport,
					draws: input.draw(ui, rect, focused, caret, placeholder),
				});
			}
		}
		if !start_inputs.is_empty() {
			return start_inputs;
		}
		let Some(form) = form else {
			return vec![];
		};
		let mut out = Vec::new();
		let placeholder =
			self.preferences.values.lang().export_title_placeholder();
		for button in &form.buttons {
			if let Command::Chrome(ChromeCommand::FocusInput(id)) =
				button.action
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
