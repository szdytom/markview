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
		let layout = if markview_platform::touch_layout() {
			WindowLayout::System
		} else {
			layout
		};
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
	pub sidebar_toggle: Option<Rect>,
	pub left_inset: f32,
	pub new_page: Option<Rect>,
	pub toolbar_x: f32,
	pub toolbar_button_size: f32,
	pub drag: Rect,
	pub width: f32,
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
		// Five controls follow the New page button on every platform, and the
		// toolbar's left edge reserves exactly their span.
		let count = 5.0;
		let toolbar_x =
			right - (count * toolbar_button_size + (count - 1.0) * CONTROL_GAP);
		let drag_width = if markview_platform::touch_layout() {
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
			left_inset: 0.,
			sidebar_toggle: None,
			tabs: Rect {
				x: left,
				y: 4.0,
				w: (drag.x - left - toolbar_button_size - 12.0).max(0.0),
				h: TOP - 4.0,
			},
			new_page: Some(Rect {
				x: drag.x - toolbar_button_size - 4.0,
				y: (TOP - toolbar_button_size) / 2.0,
				w: toolbar_button_size,
				h: toolbar_button_size,
			}),
			toolbar_x,
			toolbar_button_size,
			drag,
			width,
			height,
		}
	}
	pub fn with_left_inset(mut self, inset: f32) -> Self {
		self.left_inset = inset;
		let right = self.tabs.x + self.tabs.w;
		self.tabs.x = self.tabs.x.max(inset + 10.);
		self.tabs.w = (right - self.tabs.x).max(0.);
		self
	}
	pub fn with_sidebar(mut self, width: f32, reveal: f32) -> Self {
		let left = self.tabs.x;
		let right = left + self.tabs.w;
		self.left_inset = width * reveal;
		let mut x = left + (width - 36. - left) * reveal;
		if self.style != WindowLayout::Macos && !self.fullscreen {
			x = x.min(self.captions()[0].1.x - CONTROL_GAP - 28.);
		}
		self.sidebar_toggle = Some(Rect {
			x,
			y: 6.,
			w: 28.,
			h: 28.,
		});
		self.tabs.x = left + 36. + (width + 10. - left - 36.) * reveal;
		self.tabs.w = (right - self.tabs.x).max(0.);
		self
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
		let mut start = end.clamp(self.tabs.x, self.tabs.x + self.tabs.w);
		self.new_page = self
			.new_page
			.filter(|button| start + 8. + button.w < self.toolbar_x);
		if let Some(button) = &mut self.new_page {
			button.x = start + 8.0;
			start = button.x + button.w + 4.0;
		}
		self.drag.x = start;
		self.drag.w = (self.toolbar_x - start).max(0.0);
		self
	}
	pub fn cursor_at(self, x: f32, y: f32) -> Option<CursorIcon> {
		if markview_platform::client_resize()
			&& let Some(direction) = self.resize_at(x, y)
		{
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
		let fullscreen = markview_platform::touch_layout()
			|| self
				.window
				.as_ref()
				.is_some_and(|w| w.fullscreen().is_some());
		let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
		let mut layout = Layout::new(
			self.frame.layout,
			markview_platform::native_buttons()
				&& self.frame.layout == WindowLayout::Macos,
			width,
			height,
			fullscreen,
			maximized,
		);
		if !self.tab_strip.phone && self.folder.tree.is_some() {
			let drawer = self.folder_rect();
			let reveal = self.folder.reveal();
			layout = layout.with_sidebar(drawer.w, reveal);
		}
		let tab_end = self
			.tab_metrics
			.end(layout.tabs, self.tab_strip.scroll.offset());
		layout = layout.with_tab_end(tab_end);
		if self.tab_strip.phone {
			layout.new_page = None;
		}
		layout.focused = self.frame.focused;
		layout.hover = self.frame.hover;
		layout.pressed = self.frame.pressed;
		if let Some(native) = &self.native_frame {
			let (hover, pressed) = native.feedback();
			let hover = hover.then_some(Caption::Expand);
			let pressed = pressed.then_some(Caption::Expand);
			if hover.is_some() || pressed.is_some() {
				layout.hover = hover;
				layout.pressed = pressed;
			}
		}
		layout
	}
	pub(super) fn sync_native_frame(&self) {
		if let Some(native) = &self.native_frame {
			let layout = self.frame_layout();
			native.set_fullscreen(layout.fullscreen);
			self.native_tab_end.set(Some((
				layout.left_inset,
				self.tab_metrics
					.end(layout.tabs, self.tab_strip.scroll.offset()),
			)));
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
				let (x, y) = self.interaction.pointer.cursor();
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
		self.interaction.widgets.captured = None;
		self.interaction.pointer.release();
		self.interaction.pointer.set_auto_scroll_at(None);
		self.interaction.pointer.end_scrollbar();
		self.interaction.panels.set_grab(None);
	}
	pub(super) fn frame_press(&mut self) -> bool {
		// A new press cancels a caption release swallowed outside the window.
		self.frame.pressed = None;
		self.tab_metrics.sync(&mut self.ui, self.readers.entries());
		let (x, y) = self.interaction.pointer.cursor();
		let layout = self.frame_layout();
		if markview_platform::client_resize()
			&& let Some(direction) = layout.resize_at(x, y)
		{
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
				markview_platform::titlebar_double_click(w);
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
		let (x, y) = self.interaction.pointer.cursor();
		if self.frame_layout().caption_at(x, y) == Some(caption) {
			event_loop.window_action(self.window.as_deref(), caption);
		}
		self.redraw();
		true
	}
	pub(super) fn frame_cursor(&self) -> Option<CursorIcon> {
		self.frame_layout().cursor_at(
			self.interaction.pointer.cursor().0,
			self.interaction.pointer.cursor().1,
		)
	}
}

/// Translate the reader's actual chrome geometry at the native callback boundary.
pub(super) fn native_hit_test(
	style: WindowLayout,
	tabs: Option<(f32, f32)>,
	metrics: markview_platform::FrameMetrics,
	x: f32,
	y: f32,
) -> markview_platform::HitTest {
	use markview_platform::HitTest;
	let layout = Layout::new(
		style,
		false,
		metrics.width,
		metrics.height,
		metrics.fullscreen,
		metrics.maximized,
	);
	let layout = tabs.map_or(layout, |(inset, end)| {
		layout.with_left_inset(inset).with_tab_end(end)
	});
	if let Some(edge) = layout.resize_at(x, y) {
		HitTest::Resize(edge)
	} else if layout.caption_at(x, y) == Some(Caption::Expand) {
		HitTest::Maximize
	} else if layout.draggable(x, y) {
		HitTest::Caption
	} else {
		HitTest::Client
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn wide_sidebar_toggle_reserves_caption_space_throughout_its_animation() {
		for style in [WindowLayout::Windows, WindowLayout::Linux] {
			for window_width in [536., 600., 759.] {
				let width = super::super::chrome::folder::rect(
					window_width,
					800.,
					TOP,
					28.,
					600.,
				)
				.w;
				for step in 0..=20 {
					let layout = Layout::new(
						style,
						false,
						window_width,
						800.,
						false,
						false,
					)
					.with_sidebar(width, step as f32 / 20.);
					let button = layout.sidebar_toggle.unwrap();
					assert!(
						button.x + button.w + CONTROL_GAP
							<= layout.captions()[0].1.x
					);
					assert_eq!(layout.caption_at(button.x + 14., 20.), None);
				}
			}
		}
	}

	#[test]
	fn sidebar_toggle_and_tabs_share_continuous_motion_on_every_frame_style() {
		for style in [
			WindowLayout::Macos,
			WindowLayout::Windows,
			WindowLayout::Linux,
		] {
			for fullscreen in [false, true] {
				let base =
					Layout::new(style, false, 1200., 800., fullscreen, false);
				for width in [200., 260., 600.] {
					let closed = base.with_sidebar(width, 0.);
					let open = base.with_sidebar(width, 1.);
					assert_eq!(closed.sidebar_toggle.unwrap().x, base.tabs.x);
					assert_eq!(open.sidebar_toggle.unwrap().x, width - 36.);
					assert_eq!(open.tabs.x, width + 10.);
					for step in 0..=20 {
						let reveal = step as f32 / 20.;
						let layout = base.with_sidebar(width, reveal);
						let button = layout.sidebar_toggle.unwrap();
						assert_eq!(button.y, 6.);
						assert!(button.x >= base.tabs.x);
						assert!(button.x + button.w + 7.9 <= layout.tabs.x);
						assert!(
							(layout.tabs.x
								- (closed.tabs.x
									+ (open.tabs.x - closed.tabs.x) * reveal))
								.abs() < 0.001
						);
						assert!(
							!layout
								.with_tab_end(layout.tabs.x + 100.)
								.draggable(button.x + 14., 20.)
						);
					}
				}
			}
		}
	}

	#[test]
	fn file_column_keeps_tabs_controls_and_native_caption_hits_on_the_right() {
		use markview_platform::{FrameMetrics, HitTest};
		for style in [
			WindowLayout::Macos,
			WindowLayout::Windows,
			WindowLayout::Linux,
		] {
			for inset in [260., 400., 600.] {
				let layout =
					Layout::new(style, false, 1200., 800., false, false)
						.with_left_inset(inset)
						.with_tab_end(inset + 100.);
				assert!(layout.tabs.x >= inset + 10.);
				assert!(layout.new_page.unwrap().x >= inset);
				assert!(!layout.draggable(inset - 10., 20.));
				let metrics = FrameMetrics {
					width: 1200.,
					height: 800.,
					scale: 1.5,
					fullscreen: false,
					maximized: false,
				};
				assert_eq!(
					native_hit_test(
						style,
						Some((inset, inset + 100.)),
						metrics,
						inset - 10.,
						20.
					),
					HitTest::Client
				);
				assert_eq!(
					native_hit_test(
						style,
						Some((inset, inset + 100.)),
						metrics,
						layout.drag.x + 10.,
						20.
					),
					HitTest::Caption
				);
			}
		}
	}

	#[test]
	fn native_callback_matches_caption_drag_and_resize_geometry() {
		use markview_platform::{FrameMetrics, HitTest};
		for style in [
			WindowLayout::Windows,
			WindowLayout::Macos,
			WindowLayout::Linux,
		] {
			for scale in [1.0, 1.5, 2.0] {
				let metrics = FrameMetrics {
					width: 800.0,
					height: 500.0,
					scale,
					fullscreen: false,
					maximized: false,
				};
				let layout = Layout::new(
					style,
					false,
					metrics.width,
					metrics.height,
					false,
					false,
				)
				.with_tab_end(180.0);
				let hit = |x, y| {
					native_hit_test(style, Some((0., 180.0)), metrics, x, y)
				};
				assert_eq!(
					hit(1.0, 1.0),
					HitTest::Resize(ResizeDirection::NorthWest)
				);
				assert_eq!(hit(layout.tabs.x + 20.0, 20.0), HitTest::Client);
				assert_eq!(
					hit(layout.new_page.unwrap().x + 12.0, 20.0),
					HitTest::Client
				);
				assert_eq!(hit(layout.drag.x + 10.0, 20.0), HitTest::Caption);
				let (_, expand) = layout
					.captions()
					.into_iter()
					.find(|(caption, _)| *caption == Caption::Expand)
					.unwrap();
				assert_eq!(
					hit(expand.x + expand.w / 2.0, 20.0),
					HitTest::Maximize
				);
				assert_eq!(
					native_hit_test(
						style,
						Some((0., 180.0)),
						FrameMetrics {
							fullscreen: true,
							..metrics
						},
						layout.drag.x + 10.0,
						20.0
					),
					HitTest::Client
				);
				assert_eq!(
					native_hit_test(
						style,
						None,
						FrameMetrics {
							maximized: true,
							..metrics
						},
						1.0,
						1.0
					),
					HitTest::Client
				);
			}
		}
	}

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
			for width in [536.0, 800.0, 1200.0] {
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
					let button = layout.new_page.unwrap();
					assert_eq!(
						button.x,
						end.min(base.tabs.x + base.tabs.w) + 8.0
					);
					assert!(button.x + button.w < layout.toolbar_x);
					assert!(!layout.draggable(button.x + button.w / 2.0, 20.0));
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
							assert!(button.intersect(visible).is_none());
							assert!(
								!layout.draggable(
									visible.x + visible.w / 2.0,
									20.0
								)
							);
						}
					}
					if widths.len() <= 1 {
						assert_eq!(button.x, end + 8.0);
						assert!(
							layout.draggable(button.x + button.w + 5.0, 20.0)
						);
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
				let (top, bottom) = markview_platform::traffic_light_margins(
					f64::from(TOP),
					height,
					scale,
				);
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
				for width in [536.0, 800.0, 1200.0] {
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
