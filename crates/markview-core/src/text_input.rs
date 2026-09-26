//! Single-line editing, independent of windows, clipboard services and forms.
use crate::{
	scene::{Draw, Glyph, Paint, Rect},
	shaping::TextShaper,
	style::{ColorField as C, Condition},
};
use parley::{PositionedLayoutItem, editing::PlainEditor};
use std::{
	collections::VecDeque,
	sync::Arc,
	time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
	Left,
	Right,
	WordLeft,
	WordRight,
	Start,
	End,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
	Typing,
	Backspace,
	Delete,
	Separate,
}

#[derive(Clone)]
struct Snapshot {
	text: String,
	anchor: usize,
	focus: usize,
}

/// Owns committed text, a temporary IME composition and bounded undo history.
#[derive(Clone)]
pub struct TextInput {
	editor: PlainEditor<usize>,
	undo: VecDeque<Snapshot>,
	redo: Vec<Snapshot>,
	compose: Option<Snapshot>,
	group: Option<(EditKind, Instant)>,
	scroll: f32,
}
impl Default for TextInput {
	fn default() -> Self {
		Self {
			editor: PlainEditor::new(13.0),
			undo: VecDeque::new(),
			redo: Vec::new(),
			compose: None,
			group: None,
			scroll: 0.0,
		}
	}
}

/// Normalizes pasted/programmatic text without disturbing ordinary spaces.
pub fn single_line(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut chars = text.chars().peekable();
	while let Some(c) = chars.next() {
		if c == '\r' && chars.peek() == Some(&'\n') {
			chars.next();
		}
		match c {
			'\r' | '\n' | '\t' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
				out.push(' ')
			}
			c if !c.is_control() => out.push(c),
			_ => {}
		}
	}
	out
}

impl TextInput {
	pub fn text(&self) -> &str {
		self.compose
			.as_ref()
			.map_or(self.editor.raw_text(), |s| s.text.as_str())
	}
	pub fn is_composing(&self) -> bool {
		self.compose.is_some()
	}
	pub fn selected_text(&self) -> Option<&str> {
		self.editor.selected_text()
	}
	pub fn break_group(&mut self) {
		self.group = None;
	}
	pub fn set_text(&mut self, ui: &mut TextShaper, text: &str) {
		self.cancel_compose(ui);
		self.editor.set_text(&single_line(text));
		ui.input_driver(&mut self.editor).move_to_text_end();
		self.undo.clear();
		self.redo.clear();
		self.break_group();
		self.scroll = 0.0;
	}
	fn snapshot(&self) -> Snapshot {
		let selection = self.editor.raw_selection();
		Snapshot {
			text: self.editor.raw_text().into(),
			anchor: selection.anchor().index(),
			focus: selection.focus().index(),
		}
	}
	fn restore(&mut self, ui: &mut TextShaper, state: Snapshot) {
		self.editor.set_text(&state.text);
		let mut driver = ui.input_driver(&mut self.editor);
		driver.clear_compose();
		driver.select_byte_range(state.anchor, state.focus);
	}
	fn record(&mut self, before: Snapshot, kind: EditKind) {
		if before.text == self.editor.raw_text() {
			return;
		}
		let now = Instant::now();
		let merge = kind != EditKind::Separate
			&& self.group.is_some_and(|(last, at)| {
				last == kind && now.duration_since(at) < Duration::from_secs(1)
			});
		if !merge {
			self.undo.push_back(before);
			if self.undo.len() > 100 {
				self.undo.pop_front();
			}
		}
		self.redo.clear();
		self.group = Some((kind, now));
	}
	pub fn insert(&mut self, ui: &mut TextShaper, text: &str, kind: EditKind) {
		self.cancel_compose(ui);
		let text = single_line(text);
		if text.is_empty() {
			return;
		}
		let before = self.snapshot();
		let kind = if before.anchor != before.focus {
			EditKind::Separate
		} else {
			kind
		};
		ui.input_driver(&mut self.editor)
			.insert_or_replace_selection(&text);
		self.record(before, kind);
	}
	pub fn delete(&mut self, ui: &mut TextShaper, backwards: bool, word: bool) {
		self.cancel_compose(ui);
		let before = self.snapshot();
		if !word && before.anchor == before.focus {
			let range = if backwards {
				before
					.text
					.grapheme_indices(true)
					.map(|(i, _)| i)
					.rfind(|&i| i < before.focus)
					.map(|i| (i, before.focus))
			} else {
				before
					.text
					.grapheme_indices(true)
					.find(|(i, g)| *i + g.len() > before.focus)
					.map(|(i, g)| (before.focus, i + g.len()))
			};
			if let Some((a, b)) = range {
				ui.input_driver(&mut self.editor).select_byte_range(a, b);
			}
		}
		let mut driver = ui.input_driver(&mut self.editor);
		match (backwards, word) {
			(true, true) => driver.backdelete_word(),
			(false, true) => driver.delete_word(),
			(true, false) => driver.backdelete(),
			(false, false) => driver.delete(),
		}
		let kind = if word || before.anchor != before.focus {
			EditKind::Separate
		} else if backwards {
			EditKind::Backspace
		} else {
			EditKind::Delete
		};
		self.record(before, kind);
	}
	pub fn move_cursor(
		&mut self,
		ui: &mut TextShaper,
		motion: Motion,
		extend: bool,
	) {
		self.cancel_compose(ui);
		self.break_group();
		let previous = self.editor.raw_selection().focus().index();
		let mut d = ui.input_driver(&mut self.editor);
		match (motion, extend) {
			(Motion::Left, false) => d.move_left(),
			(Motion::Left, true) => d.select_left(),
			(Motion::Right, false) => d.move_right(),
			(Motion::Right, true) => d.select_right(),
			(Motion::WordLeft, false) => d.move_word_left(),
			(Motion::WordLeft, true) => d.select_word_left(),
			(Motion::WordRight, false) => d.move_word_right(),
			(Motion::WordRight, true) => d.select_word_right(),
			(Motion::Start, false) => d.move_to_text_start(),
			(Motion::Start, true) => d.select_to_text_start(),
			(Motion::End, false) => d.move_to_text_end(),
			(Motion::End, true) => d.select_to_text_end(),
		}
		let focus = self.editor.raw_selection().focus().index();
		let snapped = self.snap(focus, Some(focus >= previous));
		if snapped != focus {
			let anchor = if extend {
				self.editor.raw_selection().anchor().index()
			} else {
				snapped
			};
			ui.input_driver(&mut self.editor)
				.select_byte_range(anchor, snapped);
		}
	}
	// Fonts may shape one Unicode grapheme into several clusters.
	fn snap(&self, index: usize, forward: Option<bool>) -> usize {
		for (start, g) in self.editor.raw_text().grapheme_indices(true) {
			let end = start + g.len();
			if start < index && index < end {
				return if forward.unwrap_or(index - start > end - index) {
					end
				} else {
					start
				};
			}
		}
		index
	}
	pub fn select_all(&mut self, ui: &mut TextShaper) {
		self.cancel_compose(ui);
		self.break_group();
		ui.input_driver(&mut self.editor).select_all();
	}
	pub fn undo(&mut self, ui: &mut TextShaper, redo: bool) {
		self.cancel_compose(ui);
		self.break_group();
		let next = if redo {
			self.redo.pop()
		} else {
			self.undo.pop_back()
		};
		if let Some(next) = next {
			let current = self.snapshot();
			if redo {
				self.undo.push_back(current);
			} else {
				self.redo.push(current);
			}
			self.restore(ui, next);
		}
	}
	pub fn preedit(
		&mut self,
		ui: &mut TextShaper,
		text: &str,
		cursor: Option<(usize, usize)>,
	) {
		if text.is_empty() {
			self.cancel_compose(ui);
			return;
		}
		if self.compose.is_none() {
			self.break_group();
			self.compose = Some(self.snapshot());
		}
		let cursor = cursor.map(|(a, b)| {
			let offset = |i| text.get(..i).map_or(0, |s| single_line(s).len());
			(offset(a), offset(b))
		});
		ui.input_driver(&mut self.editor)
			.set_compose(&single_line(text), cursor);
	}
	pub fn commit(&mut self, ui: &mut TextShaper, text: &str) {
		self.cancel_compose(ui);
		self.insert(ui, text, EditKind::Separate);
		self.break_group();
	}
	pub fn cancel_compose(&mut self, ui: &mut TextShaper) {
		if let Some(before) = self.compose.take() {
			ui.input_driver(&mut self.editor).clear_compose();
			self.restore(ui, before);
			self.break_group();
		}
	}
	fn prepare(&mut self, ui: &mut TextShaper, rect: Rect) -> (f32, f32) {
		let mut driver = ui.input_driver(&mut self.editor);
		let layout = driver.layout();
		let height = layout.height();
		let width = layout.width();
		let caret = self.editor.ime_cursor_area();
		let available = (rect.w - 16.0).max(1.0);
		if (caret.x0 as f32) < self.scroll {
			self.scroll = caret.x0 as f32;
		}
		if (caret.x1 as f32) > self.scroll + available {
			self.scroll = (caret.x1 as f32) - available;
		}
		self.scroll =
			self.scroll.clamp(0.0, (width + 2.0 - available).max(0.0));
		(rect.x + 8.0 - self.scroll, rect.y + (rect.h - height) / 2.0)
	}
	/// `point` is in the same logical window coordinates as `rect`.
	pub fn point(
		&mut self,
		ui: &mut TextShaper,
		rect: Rect,
		point: (f32, f32),
		extend: bool,
		clicks: u8,
	) {
		self.cancel_compose(ui);
		self.break_group();
		let (x, y) = self.prepare(ui, rect);
		let mut d = ui.input_driver(&mut self.editor);
		if extend {
			d.extend_selection_to_point(point.0 - x, point.1 - y);
		} else if clicks >= 3 {
			d.select_all();
		} else if clicks == 2 {
			d.select_word_at_point(point.0 - x, point.1 - y);
		} else {
			d.move_to_point(point.0 - x, point.1 - y);
		}
		let selection = self.editor.raw_selection();
		let (a, b) = (selection.anchor().index(), selection.focus().index());
		let (anchor, focus) = (self.snap(a, None), self.snap(b, None));
		if (a, b) != (anchor, focus) {
			ui.input_driver(&mut self.editor)
				.select_byte_range(anchor, focus);
		}
	}
	pub fn ime_area(&mut self, ui: &mut TextShaper, rect: Rect) -> Rect {
		let (x, y) = self.prepare(ui, rect);
		let b = self.editor.ime_cursor_area();
		Rect {
			x: (x + (b.x0 as f32)).clamp(rect.x, rect.x + rect.w),
			y: y + (b.y0 as f32),
			w: 1.0,
			h: (b.y1 as f32) - (b.y0 as f32),
		}
	}
	pub fn draw(
		&mut self,
		ui: &mut TextShaper,
		rect: Rect,
		focused: bool,
		caret_visible: bool,
		placeholder: &str,
	) -> Vec<Draw> {
		let (x, y) = self.prepare(ui, rect);
		let color = Paint::Styled(Condition::Panel, C::Color);
		let accent = Paint::Styled(Condition::Button, C::FocusColor);
		let mut draws = vec![Draw::Rect(
			rect,
			Paint::Styled(Condition::Button, C::Background),
		)];
		let border = Paint::Styled(Condition::Button, C::BorderColor);
		for edge in [
			Rect { h: 1.0, ..rect },
			Rect {
				y: rect.y + rect.h - 1.0,
				h: 1.0,
				..rect
			},
			Rect { w: 1.0, ..rect },
			Rect {
				x: rect.x + rect.w - 1.0,
				w: 1.0,
				..rect
			},
		] {
			draws.push(Draw::Rect(edge, border));
		}
		if focused {
			draws.push(Draw::Rect(
				Rect {
					x: rect.x,
					y: rect.y + rect.h - 2.0,
					w: rect.w,
					h: 2.0,
				},
				accent,
			));
		}
		let mut text = Vec::new();
		if focused {
			for (b, _) in self.editor.selection_geometry() {
				text.push(Draw::Rect(
					Rect {
						x: x + (b.x0 as f32),
						y: y + (b.y0 as f32),
						w: (b.x1 as f32) - (b.x0 as f32),
						h: (b.y1 as f32) - (b.y0 as f32),
					},
					Paint::Styled(Condition::Button, C::ActiveBackground),
				));
			}
		}
		let layout = self.editor.try_layout().expect("input layout prepared");
		for line in layout.lines() {
			for item in line.items() {
				let PositionedLayoutItem::GlyphRun(glyphs) = item else {
					continue;
				};
				let run = glyphs.run();
				let coords: Arc<[i16]> = run.normalized_coords().into();
				for g in glyphs.positioned_glyphs() {
					text.push(Draw::Glyph(Glyph {
						font: run.font().clone(),
						coords: coords.clone(),
						id: g.id as u16,
						size: run.font_size(),
						x: x + g.x,
						y: y + g.y,
						synthetic_italic: false,
						paint: color,
					}));
				}
			}
		}
		if self.editor.raw_text().is_empty() {
			text.extend(ui.label(
				placeholder,
				13.0,
				rect.x + 8.0,
				rect.y + rect.h / 2.0 + 4.5,
				Paint::Styled(Condition::Panel, C::Muted),
			));
		}
		if let Some(range) = self.editor.raw_compose() {
			let selection = parley::editing::Selection::new(
				parley::editing::Cursor::from_byte_index(
					layout,
					range.start,
					parley::layout::Affinity::Downstream,
				),
				parley::editing::Cursor::from_byte_index(
					layout,
					range.end,
					parley::layout::Affinity::Upstream,
				),
			);
			for (b, _) in selection.geometry(layout) {
				text.push(Draw::Rect(
					Rect {
						x: x + (b.x0 as f32),
						y: y + (b.y1 as f32) - 1.0,
						w: (b.x1 as f32) - (b.x0 as f32),
						h: 1.0,
					},
					color,
				));
			}
		}
		if focused
			&& caret_visible
			&& let Some(b) = self.editor.cursor_geometry(1.0)
		{
			text.push(Draw::Rect(
				Rect {
					x: x + (b.x0 as f32),
					y: y + (b.y0 as f32),
					w: 1.0,
					h: (b.y1 as f32) - (b.y0 as f32),
				},
				color,
			));
		}
		draws.push(Draw::Clipped {
			rect: Rect {
				x: rect.x + 4.0,
				w: (rect.w - 8.0).max(0.0),
				..rect
			},
			draws: text,
		});
		draws
	}
}

#[cfg(test)]
mod tests;
