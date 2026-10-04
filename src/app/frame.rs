//! Shared window-chrome geometry and client-side gestures.
use super::{App, TOP};
use crate::{layout::Rect, settings::WindowLayout};
use markview_selection::Selection;
use std::time::{Duration, Instant};
use winit::{
	event::{ElementState, MouseButton, WindowEvent},
	window::{CursorIcon, ResizeDirection},
};

pub(super) const CONTROL_SIZE: f32 = 32.0;
pub(super) const CONTROL_GAP: f32 = 4.0;
const RIGHT_INSET: f32 = 8.0;
const CAPTION_WIDTH: f32 = 3.0 * CONTROL_SIZE + 2.0 * CONTROL_GAP;

/// Keep upper/lower caption margins between `10/11` and `1`, near `0.95`.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn traffic_light_margins(height: f64, scale: f64) -> (f64, f64) {
	let space = f64::from(TOP) - height;
	let top = (space * 0.95 / 1.95 * scale).round() / scale;
	let top = top.clamp(space / 2.1, space / 2.0);
	(top, space - top)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Caption {
	Close,
	Minimize,
	Expand,
}

pub(super) struct State {
	pub layout: WindowLayout,
	pub focused: bool,
	pub hover: Option<Caption>,
	pub pressed: Option<Caption>,
	pub last_click: Option<(Instant, (f32, f32))>,
}
impl State {
	pub fn new(layout: WindowLayout) -> Self {
		Self {
			layout: layout.resolved(),
			focused: true,
			hover: None,
			pressed: None,
			last_click: None,
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
	pub style: WindowLayout,
	pub native_buttons: bool,
	pub fullscreen: bool,
	pub maximized: bool,
	pub focused: bool,
	pub hover: Option<Caption>,
	pub pressed: Option<Caption>,
	pub tabs: Rect,
	pub toolbar_x: f32,
	pub toolbar_button_size: f32,
	pub drag: Rect,
	pub width: f32,
	#[cfg_attr(
		any(target_os = "macos", target_os = "android"),
		allow(dead_code)
	)]
	pub height: f32,
}
impl Layout {
	pub fn new(
		style: WindowLayout,
		native_buttons: bool,
		width: f32,
		height: f32,
		fullscreen: bool,
		maximized: bool,
	) -> Self {
		let style = style.resolved();
		let left = if style == WindowLayout::Macos
			&& (!fullscreen || native_buttons)
		{
			86.0
		} else {
			10.0
		};
		let right = if style != WindowLayout::Macos && !fullscreen {
			width - RIGHT_INSET - CAPTION_WIDTH - CONTROL_GAP
		} else {
			width - 16.0
		};
		let toolbar_button_size = if style == WindowLayout::Macos {
			28.0
		} else {
			CONTROL_SIZE
		};
		let count = if cfg!(target_os = "android") {
			5.0
		} else {
			4.0
		};
		let toolbar_x =
			right - (count * toolbar_button_size + (count - 1.0) * CONTROL_GAP);
		let drag_width = if cfg!(target_os = "android") {
			0.0
		} else if style == WindowLayout::Macos {
			48.0
		} else {
			40.0
		};
		let drag = Rect {
			x: toolbar_x - drag_width,
			y: 0.0,
			w: drag_width,
			h: TOP,
		};
		Self {
			style,
			native_buttons,
			fullscreen,
			maximized,
			focused: true,
			hover: None,
			pressed: None,
			tabs: Rect {
				x: left,
				y: 4.0,
				w: (drag.x - left - 4.0).max(0.0),
				h: TOP - 4.0,
			},
			toolbar_x,
			toolbar_button_size,
			drag,
			width,
			height,
		}
	}
	pub fn captions(self) -> [(Caption, Rect); 3] {
		let (order, start, width, gap, height) =
			if self.style == WindowLayout::Macos {
				(
					[Caption::Close, Caption::Minimize, Caption::Expand],
					8.0,
					24.0,
					0.0,
					TOP,
				)
			} else {
				(
					[Caption::Minimize, Caption::Expand, Caption::Close],
					self.width - RIGHT_INSET - CAPTION_WIDTH,
					CONTROL_SIZE,
					CONTROL_GAP,
					CONTROL_SIZE,
				)
			};
		std::array::from_fn(|i| {
			(
				order[i],
				Rect {
					x: start + i as f32 * (width + gap),
					y: (TOP - height) / 2.0,
					w: width,
					h: height,
				},
			)
		})
	}
	pub fn caption_at(self, x: f32, y: f32) -> Option<Caption> {
		if self.fullscreen || self.native_buttons {
			return None;
		}
		self.captions()
			.into_iter()
			.find(|(_, r)| r.contains(x, y))
			.map(|(caption, _)| caption)
	}
	pub fn draggable(self, x: f32, y: f32) -> bool {
		!self.fullscreen && self.drag.contains(x, y)
	}
	/// The unused tab-strip space belongs to the window caption.
	pub fn with_tab_end(mut self, end: f32) -> Self {
		let start = end.clamp(self.tabs.x, self.drag.x.max(self.tabs.x));
		self.drag.x = start;
		self.drag.w = (self.toolbar_x - start).max(0.0);
		self
	}
	pub fn cursor_at(self, x: f32, y: f32) -> Option<CursorIcon> {
		#[cfg(target_os = "linux")]
		if let Some(direction) = self.resize_at(x, y) {
			return Some(match direction {
				ResizeDirection::North | ResizeDirection::South => {
					CursorIcon::NsResize
				}
				ResizeDirection::East | ResizeDirection::West => {
					CursorIcon::EwResize
				}
				ResizeDirection::NorthEast | ResizeDirection::SouthWest => {
					CursorIcon::NeswResize
				}
				ResizeDirection::NorthWest | ResizeDirection::SouthEast => {
					CursorIcon::NwseResize
				}
			});
		}
		if self.caption_at(x, y).is_some() {
			Some(CursorIcon::Pointer)
		} else {
			self.draggable(x, y).then_some(CursorIcon::Default)
		}
	}
	#[cfg_attr(
		any(target_os = "macos", target_os = "android"),
		allow(dead_code)
	)]
	pub fn resize_at(self, x: f32, y: f32) -> Option<ResizeDirection> {
		if self.maximized
			|| self.fullscreen
			|| x < 0.0
			|| y < 0.0
			|| x > self.width
			|| y > self.height
		{
			return None;
		}
		let left = x < 6.0;
		let right = x > self.width - 6.0;
		let top = y < 4.0;
		let bottom = y > self.height - 6.0;
		match (left, right, top, bottom) {
			(true, _, true, _) => Some(ResizeDirection::NorthWest),
			(_, true, true, _) => Some(ResizeDirection::NorthEast),
			(true, _, _, true) => Some(ResizeDirection::SouthWest),
			(_, true, _, true) => Some(ResizeDirection::SouthEast),
			(true, _, _, _) => Some(ResizeDirection::West),
			(_, true, _, _) => Some(ResizeDirection::East),
			(_, _, true, _) => Some(ResizeDirection::North),
			(_, _, _, true) => Some(ResizeDirection::South),
			_ => None,
		}
	}
}

impl<P: super::SendEvent> App<P> {
	pub(super) fn frame_layout(&self) -> Layout {
		let (width, height, _) = self.dimensions();
		let fullscreen = cfg!(target_os = "android")
			|| self
				.window
				.as_ref()
				.is_some_and(|w| w.fullscreen().is_some());
		let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
		let mut layout = Layout::new(
			self.frame.layout,
			cfg!(target_os = "macos")
				&& self.frame.layout == WindowLayout::Macos,
			width,
			height,
			fullscreen,
			maximized,
		);
		let tab_end = self.tab_metrics.end(layout.tabs, self.tab_strip.scroll);
		layout = layout.with_tab_end(tab_end);
		layout.focused = self.frame.focused;
		layout.hover = self.frame.hover;
		layout.pressed = self.frame.pressed;
		#[cfg(windows)]
		if let Some(native) = &self.native_frame {
			let (hover, pressed) = native.feedback();
			if hover.is_some() || pressed.is_some() {
				layout.hover = hover;
				layout.pressed = pressed;
			}
		}
		layout
	}
	#[cfg(windows)]
	pub(super) fn sync_native_frame(&self) {
		if let Some(native) = &self.native_frame {
			let layout = self.frame_layout();
			native.set_fullscreen(layout.fullscreen);
			native.set_tab_end(
				self.tab_metrics.end(layout.tabs, self.tab_strip.scroll),
			);
		}
	}
	/// Window input precedes viewer, text-field and document input.
	pub(super) fn frame_event(
		&mut self,
		event_loop: &impl super::window::Loop,
		event: &WindowEvent,
	) -> bool {
		if matches!(
			event,
			WindowEvent::CursorMoved { .. }
				| WindowEvent::CursorLeft { .. }
				| WindowEvent::MouseInput { .. }
		) && self.gestures.suppress_mouse()
		{
			return true;
		}
		match event {
			WindowEvent::CursorMoved { position, .. } => {
				let scale = self.dimensions().2;
				let hover = self.frame_layout().caption_at(
					position.x as f32 / scale,
					position.y as f32 / scale,
				);
				if self.frame.hover != hover {
					self.frame.hover = hover;
					self.redraw();
				}
			}
			WindowEvent::CursorLeft { .. } => self.frame.hover = None,
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Pressed,
				..
			} => return self.frame_press(),
			WindowEvent::MouseInput {
				button: MouseButton::Left,
				state: ElementState::Released,
				..
			} => return self.frame_release(event_loop),
			WindowEvent::MouseInput {
				button: MouseButton::Right,
				state: ElementState::Pressed,
				..
			} => {
				let (x, y) = self.interaction.cursor;
				if self.frame_layout().draggable(x, y) {
					if let Some(window) = &self.window {
						window.show_window_menu(
							winit::dpi::LogicalPosition::new(x, y),
						);
					}
					return true;
				}
			}
			_ => {}
		}
		false
	}
	fn clear_frame_gesture(&mut self) {
		self.cancel_gestures();
		self.tab_strip.cancel_drag();
		self.interaction.reset_clicks();
		self.interaction.pressed = None;
		self.interaction.pointer_down = None;
		self.interaction.drag_at = None;
		self.interaction.scrollbar = None;
		self.interaction.panel_grab = None;
	}
	pub(super) fn frame_press(&mut self) -> bool {
		// A new press cancels a caption release swallowed outside the window.
		self.frame.pressed = None;
		self.tab_metrics.sync(&mut self.ui, self.readers.entries());
		let (x, y) = self.interaction.cursor;
		let layout = self.frame_layout();
		#[cfg(target_os = "linux")]
		if let Some(direction) = layout.resize_at(x, y) {
			self.clear_frame_gesture();
			if let Some(w) = &self.window
				&& let Err(error) = w.drag_resize_window(direction)
			{
				log::debug!("Cannot start window resize: {error}");
			}
			return true;
		}
		if let Some(caption) = layout.caption_at(x, y) {
			self.clear_frame_gesture();
			self.frame.pressed = Some(caption);
			self.frame.last_click = None;
			self.redraw();
			return true;
		}
		if !layout.draggable(x, y) {
			self.frame.last_click = None;
			return false;
		}
		self.clear_frame_gesture();
		let now = Instant::now();
		let double =
			self.frame.last_click.take().is_some_and(|(time, point)| {
				now.duration_since(time) <= Duration::from_millis(500)
					&& (point.0 - x).abs() < 5.0
					&& (point.1 - y).abs() < 5.0
			});
		if let Some(w) = &self.window {
			if double {
				#[cfg(target_os = "macos")]
				crate::platform::titlebar_double_click(w);
				#[cfg(not(target_os = "macos"))]
				w.set_maximized(!w.is_maximized());
			} else {
				self.frame.last_click = Some((now, (x, y)));
				if let Err(error) = w.drag_window() {
					log::debug!("Cannot start window drag: {error}");
				}
			}
		}
		true
	}
	pub(super) fn frame_release(
		&mut self,
		event_loop: &impl super::window::Loop,
	) -> bool {
		let Some(caption) = self.frame.pressed.take() else {
			return false;
		};
		let (x, y) = self.interaction.cursor;
		if self.frame_layout().caption_at(x, y) == Some(caption) {
			event_loop.window_action(self.window.as_deref(), caption);
		}
		self.redraw();
		true
	}
	pub(super) fn frame_cursor(&self) -> Option<CursorIcon> {
		self.frame_layout()
			.cursor_at(self.interaction.cursor.0, self.interaction.cursor.1)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn caption_buttons_use_a_hand_and_dragging_keeps_the_default_cursor() {
		for style in [
			WindowLayout::Macos,
			WindowLayout::Windows,
			WindowLayout::Linux,
		] {
			let layout = Layout::new(style, false, 800.0, 500.0, false, false);
			for (_, rect) in layout.captions() {
				assert_eq!(
					layout.cursor_at(rect.x + rect.w / 2.0, 20.0),
					Some(CursorIcon::Pointer)
				);
			}
			assert_eq!(
				layout.cursor_at(layout.drag.x + 10.0, 20.0),
				Some(CursorIcon::Default)
			);
			assert_eq!(layout.cursor_at(200.0, 100.0), None);
			let full = Layout::new(style, false, 800.0, 500.0, true, false);
			assert_eq!(full.cursor_at(740.0, 20.0), None);
		}
	}
	#[test]
	fn unused_tab_strip_is_draggable_without_stealing_tabs_or_controls() {
		for style in [
			WindowLayout::Macos,
			WindowLayout::Windows,
			WindowLayout::Linux,
		] {
			for width in [500.0, 800.0, 1200.0] {
				let base =
					Layout::new(style, false, width, 300.0, false, false);
				for widths in
					[vec![], vec![(100.0, 60.0)], vec![(240.0, 80.0); 20]]
				{
					let tabs = super::super::tab_strip::TabLayout::new(
						base.tabs, &widths, 0.0,
					);
					let end = tabs
						.rects
						.last()
						.map_or(base.tabs.x, |rect| rect.x + rect.w);
					let layout = base.with_tab_end(end);
					assert!(
						layout.draggable(
							layout.drag.x + layout.drag.w / 2.0,
							20.0
						)
					);
					assert!(!layout.draggable(layout.toolbar_x + 16.0, 20.0));
					assert!(!layout.draggable(layout.drag.x + 1.0, TOP + 1.0));
					for rect in &tabs.rects {
						if let Some(visible) = rect.intersect(tabs.viewport) {
							assert!(
								!layout.draggable(
									visible.x + visible.w / 2.0,
									20.0
								)
							);
						}
					}
					if widths.len() <= 1 {
						assert!(layout.draggable(end + 1.0, 20.0));
					}
					let full =
						Layout::new(style, false, width, 300.0, true, false)
							.with_tab_end(end);
					assert!(!full.draggable(full.drag.x + 1.0, 20.0));
				}
			}
		}
	}
	#[test]
	fn traffic_light_margins_follow_the_requested_ratio_at_each_dpi() {
		for height in [12.0, 14.0, 16.0, 18.0, 20.0] {
			for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
				let (top, bottom) = traffic_light_margins(height, scale);
				assert!(
					(1.0 / 1.1 - 1e-9..=1.0 + 1e-9).contains(&(top / bottom))
				);
			}
		}
	}
	#[test]
	fn chrome_keeps_controls_and_dragging_outside_overflowing_tabs() {
		for style in [
			WindowLayout::Macos,
			WindowLayout::Windows,
			WindowLayout::Linux,
		] {
			for native in [false, true] {
				for width in [500.0, 800.0, 1200.0] {
					let layout =
						Layout::new(style, native, width, 300.0, false, false);
					assert!(layout.tabs.w >= 150.0);
					assert!(layout.tabs.x + layout.tabs.w < layout.drag.x);
					assert_eq!(layout.drag.x + layout.drag.w, layout.toolbar_x);
					for (caption, rect) in layout.captions() {
						assert!(rect.intersect(layout.tabs).is_none());
						assert_eq!(
							layout.caption_at(rect.x + rect.w / 2.0, 20.0),
							(!native).then_some(caption)
						);
					}
				}
			}
		}
	}
	#[test]
	fn fullscreen_hides_captions_and_maximization_disables_resize_edges() {
		for style in [
			WindowLayout::Macos,
			WindowLayout::Windows,
			WindowLayout::Linux,
		] {
			let normal = Layout::new(style, false, 500.0, 300.0, false, false);
			assert_eq!(
				normal.resize_at(1.0, 1.0),
				Some(ResizeDirection::NorthWest)
			);
			assert_eq!(
				normal.resize_at(499.0, 299.0),
				Some(ResizeDirection::SouthEast)
			);
			assert!(normal.resize_at(250.0, 100.0).is_none());
			let full = Layout::new(style, false, 500.0, 300.0, true, false);
			assert!(full.caption_at(20.0, 20.0).is_none());
			assert!(full.resize_at(1.0, 1.0).is_none());
			let max = Layout::new(style, false, 500.0, 300.0, false, true);
			assert!(max.resize_at(1.0, 1.0).is_none());
		}
	}
}
