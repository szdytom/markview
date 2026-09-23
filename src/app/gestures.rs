//! Route captured touch and touchpad gestures through shared scrolling and zoom.
use std::time::{Duration, Instant};
use winit::event::{Touch, TouchPhase};

mod recognizer;
use recognizer::{Gesture, Motion, Recognizer};

use super::App;
use crate::state::{Command, WheelAxis, WheelStep};

type Point = (f32, f32);

#[derive(Clone, Debug, PartialEq)]
enum Tap {
	Command(Command),
	Link(String),
	ClearSelection,
}

#[derive(Clone, Copy, Debug)]
enum Surface {
	None,
	Document,
	Overflow(usize, usize),
	Panel,
	Outline,
	Tabs,
}

#[derive(Clone)]
struct Capture {
	surface: Surface,
	tap: Option<Tap>,
}

#[derive(Default)]
pub(super) struct GestureState {
	gesture: Recognizer<Capture>,
	mouse_after: Option<Instant>,
	motion: Option<(Surface, Motion)>,
	coasting: bool,
	trackpad: Option<(Surface, Instant)>,
}

impl GestureState {
	pub(super) fn deadline(&self, now: Instant) -> Option<Instant> {
		self.coasting.then_some(now + Duration::from_millis(16))
	}

	pub(super) fn suppress_mouse(&self) -> bool {
		!self.gesture.contacts.is_empty()
			|| self.mouse_after.is_some_and(|until| Instant::now() < until)
	}
}

impl App {
	pub(super) fn cancel_gestures(&mut self) {
		if !self.gestures.gesture.contacts.is_empty() {
			self.interaction.pressed = None;
		}
		self.gestures.gesture = Recognizer::default();
		self.gestures.motion = None;
		self.gestures.coasting = false;
		self.gestures.trackpad = None;
	}

	fn touch_tap(&mut self) -> Option<Tap> {
		let (x, y) = self.interaction.cursor;
		let buttons = self.buttons();
		let hit = buttons
			.iter()
			.find(|button| button.rect.contains(x, y))
			.or_else(|| {
				buttons
					.iter()
					.filter(|button| {
						let rect = button.rect;
						(x - rect.x - rect.w / 2.0).abs()
							<= rect.w.max(44.0) / 2.0
							&& (y - rect.y - rect.h / 2.0).abs()
								<= rect.h.max(44.0) / 2.0
					})
					.min_by(|a, b| {
						let distance = |rect: markview_core::scene::Rect| {
							(x - (rect.x + rect.w / 2.0))
								.hypot(y - (rect.y + rect.h / 2.0))
						};
						distance(a.rect).total_cmp(&distance(b.rect))
					})
			});
		if let Some(button) = hit {
			return Some(Tap::Command(button.action));
		}

		if self.interaction.modal.is_some() {
			return None;
		}
		if self.interaction.panel_open() {
			return (!self.pointer_in_panel()).then_some(Tap::Command(
				if self.interaction.export_open() {
					Command::Export
				} else {
					Command::Settings
				},
			));
		}
		if self.pointer_in_outline() {
			return None;
		}
		if let Some(index) = self.tab_close_at_cursor() {
			return Some(Tap::Command(Command::CloseTab(index)));
		}
		if let Some(index) = self.tab_at_cursor() {
			return Some(Tap::Command(Command::SelectTab(index)));
		}
		self.view_geometry()
			.clip()
			.contains(x, y)
			.then(|| self.link_at(x, y).map_or(Tap::ClearSelection, Tap::Link))
	}

	fn touch_surface(&mut self) -> Surface {
		if self.interaction.modal.is_some() {
			return Surface::None;
		}
		if self.interaction.panel_open() {
			return if self.pointer_in_panel() {
				Surface::Panel
			} else {
				Surface::None
			};
		}
		if self.pointer_in_outline() {
			return Surface::Outline;
		}
		let (x, y) = self.interaction.cursor;
		if self.tab_layout().viewport.contains(x, y) {
			return Surface::Tabs;
		}
		let geometry = self.view_geometry();
		if !geometry.clip().contains(x, y) {
			return Surface::None;
		}
		let (dx, dy) = geometry.document_point(x, y);
		for (bi, block) in
			self.readers.session.snapshot.blocks.iter().enumerate()
		{
			for (oi, overflow) in block.layout.overflow.iter().enumerate() {
				if overflow.rect.contains(dx, dy - block.y) {
					return Surface::Overflow(bi, oi);
				}
			}
		}
		Surface::Document
	}

	pub(super) fn handle_touch(&mut self, touch: Touch) {
		let scale = self.dimensions().2;
		let point = (
			touch.location.x as f32 / scale,
			touch.location.y as f32 / scale,
		);
		let id = (touch.device_id, touch.id);
		if touch.phase != TouchPhase::Started
			&& !self.gestures.gesture.contacts.contains_key(&id)
		{
			return;
		}
		self.gestures.mouse_after =
			Some(Instant::now() + Duration::from_millis(500));
		self.interaction.cursor = point;
		if touch.phase == TouchPhase::Started {
			self.gestures.motion = None;
			self.gestures.coasting = false;
			self.gestures.trackpad = None;
			self.readers.session.cancel_scroll_animation();
			self.tab_strip.cancel_drag();
			self.interaction.pointer_down = None;
			self.interaction.drag_at = None;
			self.interaction.scrollbar = None;
			self.interaction.panel_grab = None;
			self.interaction.focus_visible = false;
			self.interaction.reset_clicks();
			self.readers.session.select_all_pending = false;
			let capture = Capture {
				surface: self.touch_surface(),
				tap: self.touch_tap(),
			};
			self.interaction.pressed = match &capture.tap {
				Some(Tap::Command(command)) => Some(*command),
				_ => None,
			};
			self.gestures.motion =
				Some((capture.surface, Motion::new(Instant::now())));
			self.gestures.gesture.begin(id, point, capture);
			if self.gestures.gesture.drag.is_none() {
				self.gestures.motion = None;
				self.interaction.pressed = None;
			}
		} else {
			match self.gestures.gesture.update(id, touch.phase, point) {
				Some(Gesture::Tap(capture)) => {
					self.interaction.pressed = None;
					if capture.tap == self.touch_tap() {
						match capture.tap {
							Some(Tap::Command(command)) => self.action(command),
							Some(Tap::Link(link)) => {
								self.open_link(&link, false)
							}
							Some(Tap::ClearSelection) => {
								self.interaction.clear_selection();
								self.interaction.close_outline();
							}
							None => {}
						}
					}
				}
				Some(Gesture::Pan(capture, delta)) => {
					self.interaction.pressed = None;
					if let Some((_, motion)) = &mut self.gestures.motion {
						motion.sample(delta, Instant::now());
					}
					self.pan_gesture(capture.surface, delta);
				}
				None => {}
			}
		}
		if touch.phase == TouchPhase::Ended
			&& self.gestures.gesture.contacts.is_empty()
		{
			self.gestures.coasting = self
				.gestures
				.motion
				.as_mut()
				.is_some_and(|(_, motion)| motion.release(Instant::now()));
		} else if touch.phase == TouchPhase::Cancelled {
			self.gestures.motion = None;
			self.gestures.coasting = false;
			self.gestures.trackpad = None;
		}
		if self.gestures.gesture.contacts.is_empty() {
			self.interaction.pressed = None;
			self.interaction.cursor = (f32::NEG_INFINITY, f32::NEG_INFINITY);
		}
		self.refresh_hover();
		self.redraw();
	}

	/// Pixel scrolling already carries the OS speed and, on macOS, momentum.
	pub(super) fn trackpad_scroll(
		&mut self,
		dx: f32,
		dy: f32,
		phase: TouchPhase,
	) {
		let now = Instant::now();
		if phase == TouchPhase::Cancelled {
			self.cancel_gestures();
			self.interaction.wheel = Default::default();
			return;
		}
		let new = phase == TouchPhase::Started
			|| self.gestures.trackpad.is_none_or(|(_, last)| {
				now.duration_since(last) > Duration::from_millis(140)
			});
		if new {
			self.cancel_gestures();
			self.readers.session.cancel_scroll_animation();
			self.interaction.wheel = Default::default();
			let surface = self.touch_surface();
			self.gestures.trackpad = Some((surface, now));
			self.gestures.motion = Some((surface, Motion::new(now)));
		}
		let surface = self
			.gestures
			.trackpad
			.as_mut()
			.map(|(surface, last)| {
				*last = now;
				*surface
			})
			.unwrap();
		self.gestures.coasting = false;
		let shift = self.interaction.modifiers.shift_key();
		let step = self.interaction.wheel.feed(dx, dy, now, shift, phase);
		let delta = match step {
			WheelStep::Pending => (0.0, 0.0),
			WheelStep::Travel(WheelAxis::Vertical, _, dy) => {
				if matches!(surface, Surface::Tabs) {
					(dy, 0.0)
				} else {
					(0.0, dy)
				}
			}
			WheelStep::Travel(WheelAxis::Horizontal, dx, dy) => {
				let pan = if dx.abs() >= dy.abs() { dx } else { dy };
				if matches!(surface, Surface::Tabs | Surface::Overflow(..)) {
					(pan, 0.0)
				} else {
					(0.0, dy)
				}
			}
		};
		if let Some((_, motion)) = &mut self.gestures.motion {
			motion.sample(delta, now);
		}
		self.pan_gesture(surface, delta);
		if phase == TouchPhase::Ended && !cfg!(target_os = "macos") {
			self.gestures.coasting = self
				.gestures
				.motion
				.as_mut()
				.is_some_and(|(_, motion)| motion.release(now));
		}
		self.redraw();
	}

	pub(super) fn advance_gestures(&mut self, now: Instant) {
		if !self.gestures.coasting {
			return;
		}
		let next =
			self.gestures.motion.as_mut().and_then(|(surface, motion)| {
				motion.advance(now).map(|delta| (*surface, delta))
			});
		if let Some((surface, delta)) = next {
			let before = self.touch_offset(surface);
			self.pan_gesture(surface, delta);
			self.gestures.coasting = before != self.touch_offset(surface);
			self.redraw();
		} else {
			self.gestures.coasting = false;
		}
	}

	fn touch_offset(&mut self, surface: Surface) -> Point {
		match surface {
			Surface::None => (0.0, 0.0),
			Surface::Document => (0.0, self.readers.session.scroll),
			Surface::Overflow(bi, oi) => (
				*self
					.readers
					.session
					.horizontal
					.get(&(bi, oi))
					.unwrap_or(&0.0),
				self.readers.session.scroll,
			),
			Surface::Tabs => (self.tab_strip.scroll, 0.0),
			Surface::Outline => (0.0, self.interaction.outline_scroll),
			Surface::Panel => (
				0.0,
				self.panel_scroll_range().map_or(0.0, |(scroll, _)| scroll),
			),
		}
	}

	fn pan_gesture(&mut self, surface: Surface, (dx, dy): Point) {
		match surface {
			Surface::None => {}
			Surface::Panel => self.scroll_panel(-dy),
			Surface::Outline => self.scroll_outline(-dy),
			Surface::Tabs => {
				let layout = self.tab_layout();
				self.tab_strip.scroll =
					(layout.scroll - dx).clamp(0.0, layout.max_scroll);
			}
			Surface::Overflow(bi, oi) if dx != 0.0 => {
				if let Some(overflow) = self
					.readers
					.session
					.snapshot
					.blocks
					.get(bi)
					.and_then(|block| block.layout.overflow.get(oi))
				{
					let offset = self
						.readers
						.session
						.horizontal
						.entry((bi, oi))
						.or_default();
					*offset = (*offset - dx).clamp(
						0.0,
						(overflow.content_width - overflow.rect.w).max(0.0),
					);
				}
			}
			Surface::Document | Surface::Overflow(..) => self.scroll_by(-dy),
		}
	}
}

#[cfg(test)]
mod tests;
